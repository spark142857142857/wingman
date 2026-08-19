#![cfg(windows)]

use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;
use windows_sys::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use wingman_lib::interpreter::{
    ExecutionPlanV1, PreparedRequestKindV1, PreparedRequestV1, RedirectModeV1, StagePlanV1,
    ValidatedRedirectPlanV1,
};
use wingman_lib::runner_find::MAX_FIND_ENTRIES;
use wingman_lib::runner_grep::MAX_RECURSIVE_GREP_ENTRIES;
use wingman_lib::runner_ls::{MAX_LS_ENTRIES, MAX_LS_NAME_BYTES};
use wingman_lib::runner_readonly::{MAX_SORT_BYTES, MAX_SORT_RECORDS};
use wingman_lib::transport::OneShotBrokerV1;
use wingman_lib::windows_path::validate_path_value;

const MIB: usize = 1024 * 1024;
const BYTE_LIMIT_RECORD_BYTES: usize = 64 * 1024;
const BYTE_LIMIT_RECORDS: usize = MAX_SORT_BYTES / BYTE_LIMIT_RECORD_BYTES;
const RUN_COUNT: usize = 3;
const SORT_RUNNER_MEMORY_CEILING_BYTES: usize = 96 * MIB;
const TRAVERSAL_RUNNER_MEMORY_CEILING_BYTES: usize = 144 * MIB;
const LS_LONG_NAME_REPETITIONS: usize = 240;

#[derive(Clone, Copy, Serialize)]
struct ResourceSampleV1 {
    elapsed_ms: f64,
    peak_working_set_mib: f64,
    peak_private_bytes_mib: f64,
}

#[derive(Serialize)]
struct ResourceReportV1 {
    run_count: usize,
    sort_byte_limit: usize,
    sort_record_limit: usize,
    memory_ceiling_mib: usize,
    accepted_exact_byte_limit: Vec<ResourceSampleV1>,
    rejected_byte_limit_plus_one_record: Vec<ResourceSampleV1>,
    rejected_record_limit_plus_one: Vec<ResourceSampleV1>,
}

#[derive(Serialize)]
struct TraversalResourceReportV1 {
    run_count: usize,
    find_entry_limit: usize,
    recursive_grep_entry_limit: usize,
    ls_entry_limit: usize,
    ls_name_byte_limit: usize,
    memory_ceiling_mib: usize,
    accepted_find_entry_limit: Vec<ResourceSampleV1>,
    rejected_find_entry_limit_plus_one: Vec<ResourceSampleV1>,
    accepted_recursive_grep_entry_limit: Vec<ResourceSampleV1>,
    rejected_recursive_grep_entry_limit_plus_one: Vec<ResourceSampleV1>,
    accepted_ls_entry_limit: Vec<ResourceSampleV1>,
    rejected_ls_entry_limit_plus_one: Vec<ResourceSampleV1>,
    accepted_ls_name_byte_limit: Vec<ResourceSampleV1>,
    rejected_ls_name_byte_limit_plus_one: Vec<ResourceSampleV1>,
}

struct ObservedRunnerV1 {
    sample: ResourceSampleV1,
    output: Output,
}

#[test]
#[ignore = "release resource gate: materializes and redirects the 64 MiB sort limit"]
fn sort_resource_limit_stays_bounded_and_fails_closed() {
    let sandbox = sandbox();
    fs::create_dir(&sandbox).expect("create resource sandbox");
    let byte_limit_input = sandbox.join("sort-byte-limit.txt");
    let record_limit_input = sandbox.join("sort-record-limit.txt");
    let output = sandbox.join("sort-output.txt");

    let result = std::panic::catch_unwind(|| {
        create_byte_limit_input(&byte_limit_input);
        create_record_limit_input(&record_limit_input);

        let accepted_request = sort_request(&byte_limit_input, &output);
        let mut accepted = Vec::with_capacity(RUN_COUNT);
        for _ in 0..RUN_COUNT {
            let observed = run_runner_observed(&sandbox, accepted_request.clone());
            validate_accepted_byte_limit(&observed, &output);
            accepted.push(observed.sample);
        }

        append_byte_limit_record(&byte_limit_input);
        let rejected_byte_request = sort_request(&byte_limit_input, &output);
        let mut rejected_byte = Vec::with_capacity(RUN_COUNT);
        for _ in 0..RUN_COUNT {
            let observed = run_runner_observed(&sandbox, rejected_byte_request.clone());
            validate_rejected_limit(&observed, &output);
            rejected_byte.push(observed.sample);
        }

        let rejected_record_request = sort_request(&record_limit_input, &output);
        let mut rejected_record = Vec::with_capacity(RUN_COUNT);
        for _ in 0..RUN_COUNT {
            let observed = run_runner_observed(&sandbox, rejected_record_request.clone());
            validate_rejected_limit(&observed, &output);
            rejected_record.push(observed.sample);
        }

        for sample in accepted
            .iter()
            .chain(rejected_byte.iter())
            .chain(rejected_record.iter())
        {
            assert!(
                sample.peak_private_bytes_mib * MIB as f64
                    <= SORT_RUNNER_MEMORY_CEILING_BYTES as f64,
                "runner private bytes exceeded the release ceiling: {} MiB",
                sample.peak_private_bytes_mib
            );
            assert!(
                sample.peak_working_set_mib * MIB as f64 <= SORT_RUNNER_MEMORY_CEILING_BYTES as f64,
                "runner working set exceeded the release ceiling: {} MiB",
                sample.peak_working_set_mib
            );
        }

        let report = ResourceReportV1 {
            run_count: RUN_COUNT,
            sort_byte_limit: MAX_SORT_BYTES,
            sort_record_limit: MAX_SORT_RECORDS,
            memory_ceiling_mib: SORT_RUNNER_MEMORY_CEILING_BYTES / MIB,
            accepted_exact_byte_limit: accepted,
            rejected_byte_limit_plus_one_record: rejected_byte,
            rejected_record_limit_plus_one: rejected_record,
        };
        eprintln!(
            "WINGMAN_RUNNER_RESOURCE_V1={}",
            serde_json::to_string(&report).expect("serialize resource report")
        );
    });

    fs::remove_dir_all(&sandbox).expect("remove resource sandbox");
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
}

#[test]
#[ignore = "release resource gate: creates up to 262,145 directory entries"]
fn traversal_and_listing_resource_limits_are_bounded() {
    let sandbox = sandbox();
    fs::create_dir(&sandbox).expect("create traversal resource sandbox");
    let root = sandbox.join("entries");
    let output = sandbox.join("output.txt");

    let result = std::panic::catch_unwind(|| {
        fs::create_dir(&root).expect("create traversal root");
        create_numbered_files(&root, 0, MAX_FIND_ENTRIES - 1, true);

        let accepted_find_request = find_request(&root, &output);
        let accepted_find = observe_repeated(&sandbox, accepted_find_request, |observed| {
            validate_success(observed, &output, MAX_FIND_ENTRIES);
        });

        create_numbered_files(&root, MAX_FIND_ENTRIES - 1, 1, true);
        assert_eq!(MAX_FIND_ENTRIES, MAX_RECURSIVE_GREP_ENTRIES);
        let grep_request = recursive_grep_request(&root, &output);
        let accepted_grep = observe_repeated(&sandbox, grep_request, |observed| {
            validate_success(observed, &output, MAX_RECURSIVE_GREP_ENTRIES);
        });

        create_numbered_files(&root, MAX_RECURSIVE_GREP_ENTRIES, 1, true);
        let rejected_find = observe_repeated(&sandbox, find_request(&root, &output), |observed| {
            validate_untouched_rejection(
                observed,
                &output,
                b"wingman find: recursive traversal resource limit exceeded\r\n",
            );
        });
        let rejected_grep = observe_repeated(
            &sandbox,
            recursive_grep_request(&root, &output),
            |observed| {
                validate_truncated_rejection_suffix(
                    observed,
                    &output,
                    b": recursive traversal resource limit exceeded\r\n",
                );
            },
        );

        fs::remove_dir_all(&root).expect("remove traversal root");
        fs::create_dir(&root).expect("recreate listing root");
        create_numbered_files(&root, 0, MAX_LS_ENTRIES, false);
        let accepted_ls_entries =
            observe_repeated(&sandbox, ls_request(&root, &output), |observed| {
                validate_success(observed, &output, MAX_LS_ENTRIES);
            });
        create_numbered_files(&root, MAX_LS_ENTRIES, 1, false);
        let rejected_ls_entries =
            observe_repeated(&sandbox, ls_request(&root, &output), |observed| {
                validate_untouched_rejection(
                    observed,
                    &output,
                    b"wingman ls: directory listing resource limit exceeded\r\n",
                );
            });

        fs::remove_dir_all(&root).expect("remove entry-limit listing root");
        fs::create_dir(&root).expect("recreate name-byte listing root");
        let exact_name_count = create_exact_ls_name_byte_limit(&root);
        let accepted_ls_name_bytes =
            observe_repeated(&sandbox, ls_request(&root, &output), |observed| {
                validate_success(observed, &output, exact_name_count);
            });
        File::create(root.join("overflow-name")).expect("create name-byte overflow entry");
        let rejected_ls_name_bytes =
            observe_repeated(&sandbox, ls_request(&root, &output), |observed| {
                validate_untouched_rejection(
                    observed,
                    &output,
                    b"wingman ls: directory listing resource limit exceeded\r\n",
                );
            });

        let report = TraversalResourceReportV1 {
            run_count: RUN_COUNT,
            find_entry_limit: MAX_FIND_ENTRIES,
            recursive_grep_entry_limit: MAX_RECURSIVE_GREP_ENTRIES,
            ls_entry_limit: MAX_LS_ENTRIES,
            ls_name_byte_limit: MAX_LS_NAME_BYTES,
            memory_ceiling_mib: TRAVERSAL_RUNNER_MEMORY_CEILING_BYTES / MIB,
            accepted_find_entry_limit: accepted_find,
            rejected_find_entry_limit_plus_one: rejected_find,
            accepted_recursive_grep_entry_limit: accepted_grep,
            rejected_recursive_grep_entry_limit_plus_one: rejected_grep,
            accepted_ls_entry_limit: accepted_ls_entries,
            rejected_ls_entry_limit_plus_one: rejected_ls_entries,
            accepted_ls_name_byte_limit: accepted_ls_name_bytes,
            rejected_ls_name_byte_limit_plus_one: rejected_ls_name_bytes,
        };
        for sample in traversal_samples(&report) {
            assert_sample_below(
                sample,
                TRAVERSAL_RUNNER_MEMORY_CEILING_BYTES,
                "traversal/listing release ceiling",
            );
        }
        eprintln!(
            "WINGMAN_RUNNER_TRAVERSAL_RESOURCE_V1={}",
            serde_json::to_string(&report).expect("serialize traversal resource report")
        );
    });

    fs::remove_dir_all(&sandbox).expect("remove traversal resource sandbox");
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
}

fn observe_repeated(
    cwd: &Path,
    request: PreparedRequestV1,
    validate: impl Fn(&ObservedRunnerV1),
) -> Vec<ResourceSampleV1> {
    let output = match &request.kind {
        PreparedRequestKindV1::Execute { plan } => plan
            .redirect
            .as_ref()
            .map(|redirect| PathBuf::from(&redirect.path.original))
            .expect("resource request redirects output"),
        _ => panic!("resource request executes a plan"),
    };
    let mut samples = Vec::with_capacity(RUN_COUNT);
    for _ in 0..RUN_COUNT {
        fs::write(&output, b"sentinel").expect("seed resource output sentinel");
        let observed = run_runner_observed(cwd, request.clone());
        validate(&observed);
        samples.push(observed.sample);
    }
    samples
}

fn traversal_samples(report: &TraversalResourceReportV1) -> Vec<&ResourceSampleV1> {
    [
        &report.accepted_find_entry_limit,
        &report.rejected_find_entry_limit_plus_one,
        &report.accepted_recursive_grep_entry_limit,
        &report.rejected_recursive_grep_entry_limit_plus_one,
        &report.accepted_ls_entry_limit,
        &report.rejected_ls_entry_limit_plus_one,
        &report.accepted_ls_name_byte_limit,
        &report.rejected_ls_name_byte_limit_plus_one,
    ]
    .into_iter()
    .flat_map(|samples| samples.iter())
    .collect()
}

fn assert_sample_below(sample: &ResourceSampleV1, ceiling: usize, label: &str) {
    assert!(
        sample.peak_private_bytes_mib * MIB as f64 <= ceiling as f64,
        "runner private bytes exceeded {label}: {} MiB",
        sample.peak_private_bytes_mib
    );
    assert!(
        sample.peak_working_set_mib * MIB as f64 <= ceiling as f64,
        "runner working set exceeded {label}: {} MiB",
        sample.peak_working_set_mib
    );
}

fn create_numbered_files(root: &Path, start: usize, count: usize, with_match: bool) {
    for index in start..start + count {
        let path = root.join(format!("e-{index:06}.txt"));
        let mut file = File::create(path).expect("create numbered resource entry");
        if with_match {
            file.write_all(b"needle\n")
                .expect("write recursive grep resource entry");
        }
    }
}

fn create_exact_ls_name_byte_limit(root: &Path) -> usize {
    let prefix = "界".repeat(LS_LONG_NAME_REPETITIONS);
    let fixed_name_bytes = prefix.len() + 7;
    let fixed_count = MAX_LS_NAME_BYTES / fixed_name_bytes;
    let remainder = MAX_LS_NAME_BYTES - fixed_count * fixed_name_bytes;
    assert!(fixed_count < 1_000_000);
    for index in 0..fixed_count {
        let name = format!("{prefix}-{index:06}");
        assert_eq!(name.len(), fixed_name_bytes);
        File::create(root.join(name)).expect("create fixed-size Unicode listing entry");
    }

    let mut count = fixed_count;
    if remainder > 0 {
        const SUFFIX: &str = "-limit";
        assert!(remainder >= SUFFIX.len());
        let cjk_count = (remainder - SUFFIX.len()) / "界".len();
        let ascii_count = remainder - SUFFIX.len() - cjk_count * "界".len();
        let final_name = format!(
            "{}{SUFFIX}{}",
            "界".repeat(cjk_count),
            "x".repeat(ascii_count)
        );
        assert_eq!(final_name.len(), remainder);
        assert!(final_name.encode_utf16().count() <= 255);
        File::create(root.join(final_name)).expect("create exact remainder listing entry");
        count += 1;
    }

    let actual_name_bytes = fs::read_dir(root)
        .expect("read exact name-byte listing root")
        .map(|entry| {
            entry
                .expect("read exact name-byte listing entry")
                .file_name()
                .to_string_lossy()
                .len()
        })
        .sum::<usize>();
    assert_eq!(actual_name_bytes, MAX_LS_NAME_BYTES);
    assert!(count < MAX_LS_ENTRIES);
    count
}

fn find_request(root: &Path, output: &Path) -> PreparedRequestV1 {
    execute_request(
        StagePlanV1::FindPaths {
            path: path_spec(root),
            entry_type: None,
            name_pattern: None,
            ignore_case: false,
            min_depth: 0,
            max_depth: None,
        },
        output,
    )
}

fn recursive_grep_request(root: &Path, output: &Path) -> PreparedRequestV1 {
    execute_request(
        StagePlanV1::SearchText {
            pattern: "needle".to_string(),
            paths: vec![path_spec(root)],
            ignore_case: false,
            line_numbers: false,
            invert_match: false,
            fixed_strings: true,
            recursive: true,
        },
        output,
    )
}

fn ls_request(root: &Path, output: &Path) -> PreparedRequestV1 {
    execute_request(
        StagePlanV1::ListEntries {
            path: Some(path_spec(root)),
            include_hidden: true,
            long: false,
            human_readable: false,
        },
        output,
    )
}

fn execute_request(stage: StagePlanV1, output: &Path) -> PreparedRequestV1 {
    PreparedRequestV1 {
        protocol: "wingman.run".to_string(),
        version: 1,
        kind: PreparedRequestKindV1::Execute {
            plan: ExecutionPlanV1 {
                stages: vec![stage],
                redirect: Some(ValidatedRedirectPlanV1 {
                    mode: RedirectModeV1::Overwrite,
                    path: path_spec(output),
                }),
            },
        },
    }
}

fn validate_success(observed: &ObservedRunnerV1, output: &Path, expected_records: usize) {
    assert_eq!(observed.output.status.code(), Some(0));
    assert!(observed.output.stdout.is_empty());
    assert!(observed.output.stderr.is_empty());
    assert_eq!(count_output_records(output), expected_records);
}

fn validate_untouched_rejection(observed: &ObservedRunnerV1, output: &Path, diagnostic: &[u8]) {
    assert_eq!(observed.output.status.code(), Some(1));
    assert!(observed.output.stdout.is_empty());
    assert_eq!(observed.output.stderr, diagnostic);
    assert_eq!(
        fs::read(output).expect("read untouched redirect"),
        b"sentinel"
    );
}

fn validate_truncated_rejection_suffix(
    observed: &ObservedRunnerV1,
    output: &Path,
    diagnostic_suffix: &[u8],
) {
    assert_eq!(observed.output.status.code(), Some(1));
    assert!(observed.output.stdout.is_empty());
    assert!(observed.output.stderr.starts_with(b"wingman grep: '"));
    assert!(observed.output.stderr.ends_with(diagnostic_suffix));
    assert!(observed.output.stderr.len() <= 4 * 1024 + 2);
    assert_eq!(count_bytes(&observed.output.stderr, b'\n'), 1);
    assert_eq!(
        fs::metadata(output)
            .expect("inspect rejected redirect")
            .len(),
        0
    );
}

fn count_output_records(path: &Path) -> usize {
    let mut file = File::open(path).expect("open resource output");
    let mut buffer = [0u8; 64 * 1024];
    let mut records = 0usize;
    loop {
        let read = file.read(&mut buffer).expect("read resource output");
        if read == 0 {
            return records;
        }
        records += count_bytes(&buffer[..read], b'\n');
    }
}

fn count_bytes(bytes: &[u8], needle: u8) -> usize {
    bytes.iter().filter(|value| **value == needle).count()
}

fn create_byte_limit_input(path: &Path) {
    let mut writer = BufWriter::new(File::create(path).expect("create byte-limit input"));
    let mut record = vec![b'x'; BYTE_LIMIT_RECORD_BYTES];
    for index in (0..BYTE_LIMIT_RECORDS).rev() {
        let prefix = format!("{index:04}:");
        record[..prefix.len()].copy_from_slice(prefix.as_bytes());
        writer.write_all(&record).expect("write byte-limit record");
        writer
            .write_all(b"\n")
            .expect("terminate byte-limit record");
    }
    writer.flush().expect("flush byte-limit input");
    assert_eq!(
        fs::metadata(path).unwrap().len(),
        (MAX_SORT_BYTES + BYTE_LIMIT_RECORDS) as u64
    );
}

fn append_byte_limit_record(path: &Path) {
    let mut writer = BufWriter::new(
        OpenOptions::new()
            .append(true)
            .open(path)
            .expect("open byte-limit input for append"),
    );
    writer
        .write_all(&vec![b'z'; BYTE_LIMIT_RECORD_BYTES])
        .expect("append over-limit record");
    writer
        .write_all(b"\n")
        .expect("terminate over-limit record");
    writer.flush().expect("flush over-limit record");
}

fn create_record_limit_input(path: &Path) {
    let mut writer = BufWriter::new(File::create(path).expect("create record-limit input"));
    for _ in 0..=MAX_SORT_RECORDS {
        writer.write_all(b"x\n").expect("write record-limit input");
    }
    writer.flush().expect("flush record-limit input");
}

fn sort_request(input: &Path, output: &Path) -> PreparedRequestV1 {
    PreparedRequestV1 {
        protocol: "wingman.run".to_string(),
        version: 1,
        kind: PreparedRequestKindV1::Execute {
            plan: ExecutionPlanV1 {
                stages: vec![StagePlanV1::SortLines {
                    path: Some(path_spec(input)),
                    reverse: false,
                    numeric: false,
                    unique: false,
                }],
                redirect: Some(ValidatedRedirectPlanV1 {
                    mode: RedirectModeV1::Overwrite,
                    path: path_spec(output),
                }),
            },
        },
    }
}

fn run_runner_observed(cwd: &Path, request: PreparedRequestV1) -> ObservedRunnerV1 {
    let request_id = Uuid::new_v4().as_simple().to_string();
    let pipe_name = format!(
        r"\\.\pipe\wingman-runner-resource-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let broker = OneShotBrokerV1::bind(&pipe_name, request_id.clone(), request)
        .expect("bind resource broker");
    let server = thread::spawn(move || broker.serve());
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(&request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start resource runner");
    server
        .join()
        .expect("resource broker thread")
        .expect("serve resource request");

    let mut peak_working_set = 0usize;
    let mut peak_private_bytes = 0usize;
    let status = loop {
        let counters = process_memory(&child);
        peak_working_set = peak_working_set.max(counters.PeakWorkingSetSize);
        peak_private_bytes = peak_private_bytes.max(counters.PrivateUsage);
        if let Some(status) = child.try_wait().expect("poll resource runner") {
            break status;
        }
        thread::sleep(Duration::from_millis(2));
    };
    let elapsed = started.elapsed();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    child
        .stdout
        .take()
        .expect("resource stdout")
        .read_to_end(&mut stdout)
        .expect("read resource stdout");
    child
        .stderr
        .take()
        .expect("resource stderr")
        .read_to_end(&mut stderr)
        .expect("read resource stderr");

    ObservedRunnerV1 {
        sample: ResourceSampleV1 {
            elapsed_ms: elapsed.as_secs_f64() * 1_000.0,
            peak_working_set_mib: peak_working_set as f64 / MIB as f64,
            peak_private_bytes_mib: peak_private_bytes as f64 / MIB as f64,
        },
        output: Output {
            status,
            stdout,
            stderr,
        },
    }
}

fn process_memory(child: &std::process::Child) -> PROCESS_MEMORY_COUNTERS_EX {
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    let succeeded = unsafe {
        GetProcessMemoryInfo(
            child.as_raw_handle(),
            &mut counters as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS,
            counters.cb,
        )
    };
    assert_ne!(
        succeeded,
        0,
        "read runner memory: {}",
        std::io::Error::last_os_error()
    );
    counters
}

fn validate_accepted_byte_limit(observed: &ObservedRunnerV1, output: &Path) {
    assert_eq!(observed.output.status.code(), Some(0));
    assert!(observed.output.stdout.is_empty());
    assert!(observed.output.stderr.is_empty());
    assert_eq!(
        fs::metadata(output).unwrap().len(),
        (MAX_SORT_BYTES + BYTE_LIMIT_RECORDS * 2) as u64
    );
}

fn validate_rejected_limit(observed: &ObservedRunnerV1, output: &Path) {
    assert_eq!(observed.output.status.code(), Some(1));
    assert!(observed.output.stdout.is_empty());
    assert_eq!(
        observed.output.stderr,
        b"wingman sort: materialization resource limit exceeded\r\n"
    );
    assert_eq!(fs::metadata(output).unwrap().len(), 0);
}

fn path_spec(path: &Path) -> wingman_lib::windows_path::ValidatedPathSpecV1 {
    validate_path_value(&path.to_string_lossy()).expect("validate resource path")
}

fn sandbox() -> PathBuf {
    std::env::temp_dir().join(format!(
        "wingman-runner-resource-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    ))
}
