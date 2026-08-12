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
use wingman_lib::runner_readonly::{MAX_SORT_BYTES, MAX_SORT_RECORDS};
use wingman_lib::transport::OneShotBrokerV1;
use wingman_lib::windows_path::validate_path_value;

const MIB: usize = 1024 * 1024;
const BYTE_LIMIT_RECORD_BYTES: usize = 64 * 1024;
const BYTE_LIMIT_RECORDS: usize = MAX_SORT_BYTES / BYTE_LIMIT_RECORD_BYTES;
const RUN_COUNT: usize = 3;
const SORT_RUNNER_MEMORY_CEILING_BYTES: usize = 96 * MIB;

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
