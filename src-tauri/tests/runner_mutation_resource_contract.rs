#![cfg(windows)]

use serde::Serialize;
use std::fs::{self, File};
use std::io::Read;
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
    ExecutionPlanV1, ExistingDestinationPolicyV1, PreparedRequestKindV1, PreparedRequestV1,
    StagePlanV1, MAX_PATH_OPERANDS,
};
use wingman_lib::transport::OneShotBrokerV1;
use wingman_lib::windows_path::validate_path_value;

const MIB: usize = 1024 * 1024;
const RUN_COUNT: usize = 3;
const MUTATION_TREE_ENTRY_LIMIT: usize = 100_000;
const MUTATION_RUNNER_MEMORY_CEILING_BYTES: usize = 80 * MIB;

#[derive(Clone, Copy, Serialize)]
struct ResourceSampleV1 {
    elapsed_ms: f64,
    peak_working_set_mib: f64,
    peak_private_bytes_mib: f64,
}

#[derive(Serialize)]
struct MutationResourceReportV1 {
    run_count: usize,
    path_operand_limit: usize,
    recursive_entry_limit: usize,
    memory_ceiling_mib: usize,
    mkdir_exact_operands: Vec<ResourceSampleV1>,
    mkdir_operands_plus_one: Vec<ResourceSampleV1>,
    touch_exact_operands: Vec<ResourceSampleV1>,
    touch_operands_plus_one: Vec<ResourceSampleV1>,
    copy_exact_entries: Vec<ResourceSampleV1>,
    copy_entries_plus_one: Vec<ResourceSampleV1>,
    move_exact_entries: Vec<ResourceSampleV1>,
    move_entries_plus_one: Vec<ResourceSampleV1>,
    remove_exact_entries: Vec<ResourceSampleV1>,
    remove_entries_plus_one: Vec<ResourceSampleV1>,
}

struct ObservedRunnerV1 {
    sample: ResourceSampleV1,
    output: Output,
}

#[test]
#[ignore = "release resource gate: copies, moves, and removes a 100,000-entry tree"]
fn mutation_resource_limits_are_bounded_and_atomic() {
    let sandbox = sandbox();
    fs::create_dir(&sandbox).expect("create mutation resource sandbox");

    let result = std::panic::catch_unwind(|| {
        let mkdir_root = sandbox.join("mkdir-operands");
        let touch_root = sandbox.join("touch-operands");
        fs::create_dir(&mkdir_root).expect("create mkdir operand root");
        fs::create_dir(&touch_root).expect("create touch operand root");

        let mkdir_paths = numbered_paths(&mkdir_root, MAX_PATH_OPERANDS);
        let mkdir_exact = repeat_runner(&sandbox, mkdir_request(&mkdir_paths), |observed| {
            validate_success(observed);
            assert_all_exist(&mkdir_paths, Path::is_dir);
            remove_paths(&mkdir_paths);
        });
        let mkdir_over_paths = numbered_paths(&mkdir_root, MAX_PATH_OPERANDS + 1);
        let mkdir_over = repeat_runner(&sandbox, mkdir_request(&mkdir_over_paths), |observed| {
            validate_invalid_wire_request(observed);
            assert_all_absent(&mkdir_over_paths);
        });

        let touch_paths = numbered_paths(&touch_root, MAX_PATH_OPERANDS);
        let touch_exact = repeat_runner(&sandbox, touch_request(&touch_paths), |observed| {
            validate_success(observed);
            assert_all_exist(&touch_paths, Path::is_file);
            remove_paths(&touch_paths);
        });
        let touch_over_paths = numbered_paths(&touch_root, MAX_PATH_OPERANDS + 1);
        let touch_over = repeat_runner(&sandbox, touch_request(&touch_over_paths), |observed| {
            validate_invalid_wire_request(observed);
            assert_all_absent(&touch_over_paths);
        });

        let source = sandbox.join("source-tree");
        let copy_destination = sandbox.join("copy-tree");
        let move_destination = sandbox.join("move-tree");
        fs::create_dir(&source).expect("create recursive mutation source");
        create_flat_tree(&source, MUTATION_TREE_ENTRY_LIMIT - 1);

        let mut copy_exact = Vec::with_capacity(RUN_COUNT);
        let mut remove_exact = Vec::with_capacity(RUN_COUNT);
        for _ in 0..RUN_COUNT {
            let copied = run_runner_observed(&sandbox, copy_request(&source, &copy_destination));
            validate_success(&copied);
            assert_flat_tree(&copy_destination, MUTATION_TREE_ENTRY_LIMIT);
            assert_no_staging_entries(&sandbox);
            copy_exact.push(copied.sample);

            let removed = run_runner_observed(&sandbox, remove_request(&copy_destination));
            validate_success(&removed);
            assert!(!copy_destination.exists());
            assert_no_staging_entries(&sandbox);
            remove_exact.push(removed.sample);
        }

        let move_exact = repeat_runner(
            &sandbox,
            move_request(&source, &move_destination),
            |observed| {
                validate_success(observed);
                assert!(!source.exists());
                assert_flat_tree(&move_destination, MUTATION_TREE_ENTRY_LIMIT);
                fs::rename(&move_destination, &source).expect("restore moved resource tree");
                assert_no_staging_entries(&sandbox);
            },
        );

        File::create(source.join(format!("e-{:06}.txt", MUTATION_TREE_ENTRY_LIMIT - 1)))
            .expect("add recursive mutation overflow entry");
        assert_flat_tree(&source, MUTATION_TREE_ENTRY_LIMIT + 1);

        let copy_over = repeat_runner(
            &sandbox,
            copy_request(&source, &copy_destination),
            |observed| {
                validate_resource_rejection(observed, b"wingman cp: ");
                assert_flat_tree(&source, MUTATION_TREE_ENTRY_LIMIT + 1);
                assert!(!copy_destination.exists());
                assert_no_staging_entries(&sandbox);
            },
        );
        let move_over = repeat_runner(
            &sandbox,
            move_request(&source, &move_destination),
            |observed| {
                validate_resource_rejection(observed, b"wingman mv: ");
                assert_flat_tree(&source, MUTATION_TREE_ENTRY_LIMIT + 1);
                assert!(!move_destination.exists());
                assert_no_staging_entries(&sandbox);
            },
        );
        let remove_over = repeat_runner(&sandbox, remove_request(&source), |observed| {
            validate_resource_rejection(observed, b"wingman rm: ");
            assert_flat_tree(&source, MUTATION_TREE_ENTRY_LIMIT + 1);
            assert_no_staging_entries(&sandbox);
        });

        let report = MutationResourceReportV1 {
            run_count: RUN_COUNT,
            path_operand_limit: MAX_PATH_OPERANDS,
            recursive_entry_limit: MUTATION_TREE_ENTRY_LIMIT,
            memory_ceiling_mib: MUTATION_RUNNER_MEMORY_CEILING_BYTES / MIB,
            mkdir_exact_operands: mkdir_exact,
            mkdir_operands_plus_one: mkdir_over,
            touch_exact_operands: touch_exact,
            touch_operands_plus_one: touch_over,
            copy_exact_entries: copy_exact,
            copy_entries_plus_one: copy_over,
            move_exact_entries: move_exact,
            move_entries_plus_one: move_over,
            remove_exact_entries: remove_exact,
            remove_entries_plus_one: remove_over,
        };
        for sample in report_samples(&report) {
            assert_sample_below(sample, MUTATION_RUNNER_MEMORY_CEILING_BYTES);
        }
        eprintln!(
            "WINGMAN_RUNNER_MUTATION_RESOURCE_V1={}",
            serde_json::to_string(&report).expect("serialize mutation resource report")
        );
    });

    fs::remove_dir_all(&sandbox).expect("remove mutation resource sandbox");
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
}

fn repeat_runner(
    cwd: &Path,
    request: PreparedRequestV1,
    validate: impl Fn(&ObservedRunnerV1),
) -> Vec<ResourceSampleV1> {
    let mut samples = Vec::with_capacity(RUN_COUNT);
    for _ in 0..RUN_COUNT {
        let observed = run_runner_observed(cwd, request.clone());
        validate(&observed);
        samples.push(observed.sample);
    }
    samples
}

fn report_samples(report: &MutationResourceReportV1) -> Vec<&ResourceSampleV1> {
    [
        &report.mkdir_exact_operands,
        &report.mkdir_operands_plus_one,
        &report.touch_exact_operands,
        &report.touch_operands_plus_one,
        &report.copy_exact_entries,
        &report.copy_entries_plus_one,
        &report.move_exact_entries,
        &report.move_entries_plus_one,
        &report.remove_exact_entries,
        &report.remove_entries_plus_one,
    ]
    .into_iter()
    .flat_map(|samples| samples.iter())
    .collect()
}

fn run_runner_observed(cwd: &Path, request: PreparedRequestV1) -> ObservedRunnerV1 {
    let request_id = Uuid::new_v4().as_simple().to_string();
    let pipe_name = format!(
        r"\\.\pipe\wingman-runner-mutation-resource-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let broker = OneShotBrokerV1::bind(&pipe_name, request_id.clone(), request)
        .expect("bind mutation resource broker");
    let server = thread::spawn(move || broker.serve());
    let started = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(&request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start mutation resource runner");
    server
        .join()
        .expect("mutation resource broker thread")
        .expect("serve mutation resource request");

    let mut peak_working_set = 0usize;
    let mut peak_private_bytes = 0usize;
    let status = loop {
        let counters = process_memory(&child);
        peak_working_set = peak_working_set.max(counters.PeakWorkingSetSize);
        peak_private_bytes = peak_private_bytes.max(counters.PrivateUsage);
        if let Some(status) = child.try_wait().expect("poll mutation resource runner") {
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
        .expect("mutation resource stdout")
        .read_to_end(&mut stdout)
        .expect("read mutation resource stdout");
    child
        .stderr
        .take()
        .expect("mutation resource stderr")
        .read_to_end(&mut stderr)
        .expect("read mutation resource stderr");

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
        "read mutation runner memory: {}",
        std::io::Error::last_os_error()
    );
    counters
}

fn assert_sample_below(sample: &ResourceSampleV1, ceiling: usize) {
    assert!(
        sample.peak_private_bytes_mib * MIB as f64 <= ceiling as f64,
        "mutation runner private bytes exceeded release ceiling: {} MiB",
        sample.peak_private_bytes_mib
    );
    assert!(
        sample.peak_working_set_mib * MIB as f64 <= ceiling as f64,
        "mutation runner working set exceeded release ceiling: {} MiB",
        sample.peak_working_set_mib
    );
}

fn mkdir_request(paths: &[PathBuf]) -> PreparedRequestV1 {
    execute_request(StagePlanV1::CreateDirectories {
        paths: path_specs(paths),
        parents: false,
    })
}

fn touch_request(paths: &[PathBuf]) -> PreparedRequestV1 {
    execute_request(StagePlanV1::TouchFiles {
        paths: path_specs(paths),
    })
}

fn copy_request(source: &Path, destination: &Path) -> PreparedRequestV1 {
    execute_request(StagePlanV1::CopyPath {
        source: path_spec(source),
        destination: path_spec(destination),
        recursive: true,
        existing_destination: ExistingDestinationPolicyV1::Replace,
    })
}

fn move_request(source: &Path, destination: &Path) -> PreparedRequestV1 {
    execute_request(StagePlanV1::MovePath {
        source: path_spec(source),
        destination: path_spec(destination),
        existing_destination: ExistingDestinationPolicyV1::Replace,
    })
}

fn remove_request(path: &Path) -> PreparedRequestV1 {
    execute_request(StagePlanV1::RemovePaths {
        paths: vec![path_spec(path)],
        recursive: true,
        force: true,
    })
}

fn execute_request(stage: StagePlanV1) -> PreparedRequestV1 {
    PreparedRequestV1 {
        protocol: "wingman.run".to_string(),
        version: 1,
        kind: PreparedRequestKindV1::Execute {
            plan: ExecutionPlanV1 {
                stages: vec![stage],
                redirect: None,
            },
        },
    }
}

fn path_specs(paths: &[PathBuf]) -> Vec<wingman_lib::windows_path::ValidatedPathSpecV1> {
    paths.iter().map(|path| path_spec(path)).collect()
}

fn path_spec(path: &Path) -> wingman_lib::windows_path::ValidatedPathSpecV1 {
    validate_path_value(&path.to_string_lossy()).expect("validate mutation resource path")
}

fn numbered_paths(root: &Path, count: usize) -> Vec<PathBuf> {
    (0..count)
        .map(|index| root.join(format!("operand-{index:03}")))
        .collect()
}

fn create_flat_tree(root: &Path, children: usize) {
    for index in 0..children {
        File::create(root.join(format!("e-{index:06}.txt"))).expect("create mutation tree entry");
    }
}

fn assert_flat_tree(root: &Path, expected_entries: usize) {
    assert!(root.is_dir());
    assert_eq!(
        fs::read_dir(root)
            .expect("read mutation tree")
            .map(|entry| entry.expect("read mutation tree entry"))
            .count()
            + 1,
        expected_entries
    );
}

fn assert_all_exist(paths: &[PathBuf], predicate: impl Fn(&Path) -> bool) {
    assert!(paths.iter().all(|path| predicate(path)));
}

fn assert_all_absent(paths: &[PathBuf]) {
    assert!(paths.iter().all(|path| !path.exists()));
}

fn remove_paths(paths: &[PathBuf]) {
    for path in paths {
        if path.is_dir() {
            fs::remove_dir(path).expect("remove mutation resource directory");
        } else {
            fs::remove_file(path).expect("remove mutation resource file");
        }
    }
}

fn assert_no_staging_entries(sandbox: &Path) {
    let staging = fs::read_dir(sandbox)
        .expect("read mutation resource sandbox")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".wingman-stage-")
        })
        .count();
    assert_eq!(staging, 0);
}

fn validate_success(observed: &ObservedRunnerV1) {
    assert_eq!(observed.output.status.code(), Some(0));
    assert!(observed.output.stdout.is_empty());
    assert!(observed.output.stderr.is_empty());
}

fn validate_invalid_wire_request(observed: &ObservedRunnerV1) {
    assert_eq!(observed.output.status.code(), Some(2));
    assert!(observed.output.stdout.is_empty());
    assert_eq!(
        observed.output.stderr,
        b"wingman-runner: prepared request was rejected\r\n"
    );
}

fn validate_resource_rejection(observed: &ObservedRunnerV1, prefix: &[u8]) {
    assert_eq!(observed.output.status.code(), Some(2));
    assert!(observed.output.stdout.is_empty());
    assert!(observed.output.stderr.starts_with(prefix));
    assert!(
        observed
            .output
            .stderr
            .ends_with(b": recursive copy exceeds a resource limit\r\n")
            || observed
                .output
                .stderr
                .ends_with(b": recursive removal exceeds a resource limit\r\n")
    );
    assert_eq!(
        observed
            .output
            .stderr
            .iter()
            .filter(|byte| **byte == b'\n')
            .count(),
        1
    );
    assert!(observed.output.stderr.len() <= 4 * 1024 + 2);
}

fn sandbox() -> PathBuf {
    std::env::temp_dir().join(format!(
        "wingman-runner-mutation-resource-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    ))
}
