#![cfg(windows)]

use serde::Serialize;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;
use wingman_lib::interpreter::{
    ExecutionPlanV1, PreparedRequestKindV1, PreparedRequestV1, RedirectModeV1, StagePlanV1,
    ValidatedRedirectPlanV1,
};
use wingman_lib::transport::OneShotBrokerV1;
use wingman_lib::windows_path::validate_path_value;

const MIB: usize = 1024 * 1024;
const TEXT_CORPUS_BYTES: usize = 100 * MIB;
const TEXT_RECORD_BYTES: usize = 128;
const TEXT_RECORD_COUNT: usize = TEXT_CORPUS_BYTES / TEXT_RECORD_BYTES;
const FIND_ENTRY_COUNT: usize = 20_000;
const FIND_DIRECTORY_COUNT: usize = 100;
const FIND_FILES_PER_DIRECTORY: usize = 199;
const SORT_RECORD_COUNT: usize = 200_000;
const SORT_RECORD_BYTES: usize = 32;
const RUN_COUNT: usize = 3;
const GREP_CACHED_TARGET_MS: f64 = 1_000.0;
const FIND_CACHED_TARGET_MS: f64 = 535.0;
const REDIRECTED_CAT_CACHED_TARGET_MS: f64 = 3_700.0;
const REDIRECTED_SORT_CACHED_TARGET_MS: f64 = 790.0;
const UNCACHED_FIXTURE_ENV: &str = "WINGMAN_UNCACHED_FIXTURE_ROOT";
const UNCACHED_OPERATION_ENV: &str = "WINGMAN_UNCACHED_OPERATION";
const UNCACHED_FIXTURE_MARKER: &str = ".wingman-uncached-fixture-v1";

#[derive(Serialize)]
struct CachedTimingReportV1 {
    cache_state: &'static str,
    run_count: usize,
    corpus_bytes: usize,
    corpus_records: usize,
    find_entries: usize,
    sort_records: usize,
    grep_ms: Vec<f64>,
    grep_median_ms: f64,
    find_ms: Vec<f64>,
    find_median_ms: f64,
    redirected_cat_ms: Vec<f64>,
    redirected_cat_median_ms: f64,
    redirected_sort_ms: Vec<f64>,
    redirected_sort_median_ms: f64,
    grep_target_ms: f64,
    find_target_ms: f64,
    redirected_cat_target_ms: f64,
    redirected_sort_target_ms: f64,
}

#[derive(Serialize)]
struct UncachedTimingSampleV1 {
    cache_state: &'static str,
    operation: String,
    elapsed_ms: f64,
    corpus_bytes: usize,
    corpus_records: usize,
    find_entries: usize,
    sort_records: usize,
}

struct RunnerMeasurementV1 {
    elapsed: Duration,
    output: Output,
}

#[test]
#[ignore = "release performance baseline: creates a 100 MiB corpus and 20,000-entry tree"]
fn cached_runner_timing_baseline() {
    let sandbox = sandbox();
    fs::create_dir(&sandbox).expect("create performance sandbox");
    let text_corpus = sandbox.join("text-100mib.txt");
    let find_root = sandbox.join("find-20000");
    let sort_corpus = sandbox.join("sort-200000.txt");
    let cat_output = sandbox.join("cat-output.txt");
    let sort_output = sandbox.join("sort-output.txt");

    let result = std::panic::catch_unwind(|| {
        create_text_corpus(&text_corpus);
        create_find_tree(&find_root);
        create_sort_corpus(&sort_corpus);

        let grep_request = execute_request(
            vec![StagePlanV1::SearchText {
                pattern: "WINGMAN_PERF_MATCH".to_string(),
                paths: vec![path_spec(&text_corpus)],
                ignore_case: false,
                line_numbers: false,
                invert_match: false,
                fixed_strings: true,
                recursive: false,
            }],
            None,
        );
        let find_request = execute_request(
            vec![StagePlanV1::FindPaths {
                path: path_spec(&find_root),
                entry_type: None,
                name_pattern: None,
                ignore_case: false,
                min_depth: 1,
                max_depth: None,
            }],
            None,
        );
        let cat_request = execute_request(
            vec![StagePlanV1::ReadTextFiles {
                paths: vec![path_spec(&text_corpus)],
                number_lines: false,
            }],
            Some(&cat_output),
        );
        let sort_request = execute_request(
            vec![StagePlanV1::SortLines {
                path: Some(path_spec(&sort_corpus)),
                reverse: false,
                numeric: false,
                unique: false,
            }],
            Some(&sort_output),
        );

        validate_grep(run_runner(&sandbox, grep_request.clone()));
        validate_find(run_runner(&sandbox, find_request.clone()));
        validate_cat(run_runner(&sandbox, cat_request.clone()), &cat_output);
        validate_sort(run_runner(&sandbox, sort_request.clone()), &sort_output);

        let mut grep_ms = Vec::with_capacity(RUN_COUNT);
        let mut find_ms = Vec::with_capacity(RUN_COUNT);
        let mut cat_ms = Vec::with_capacity(RUN_COUNT);
        let mut sort_ms = Vec::with_capacity(RUN_COUNT);
        for _ in 0..RUN_COUNT {
            let grep = run_runner(&sandbox, grep_request.clone());
            grep_ms.push(duration_ms(grep.elapsed));
            validate_grep(grep);

            let find = run_runner(&sandbox, find_request.clone());
            find_ms.push(duration_ms(find.elapsed));
            validate_find(find);

            let cat = run_runner(&sandbox, cat_request.clone());
            cat_ms.push(duration_ms(cat.elapsed));
            validate_cat(cat, &cat_output);

            let sort = run_runner(&sandbox, sort_request.clone());
            sort_ms.push(duration_ms(sort.elapsed));
            validate_sort(sort, &sort_output);
        }

        let grep_median_ms = median(&grep_ms);
        let find_median_ms = median(&find_ms);
        let redirected_cat_median_ms = median(&cat_ms);
        let redirected_sort_median_ms = median(&sort_ms);
        assert_target("grep", grep_median_ms, GREP_CACHED_TARGET_MS, &grep_ms);
        assert_target("find", find_median_ms, FIND_CACHED_TARGET_MS, &find_ms);
        assert_target(
            "redirected cat",
            redirected_cat_median_ms,
            REDIRECTED_CAT_CACHED_TARGET_MS,
            &cat_ms,
        );
        assert_target(
            "redirected sort",
            redirected_sort_median_ms,
            REDIRECTED_SORT_CACHED_TARGET_MS,
            &sort_ms,
        );

        let report = CachedTimingReportV1 {
            cache_state: "warmed",
            run_count: RUN_COUNT,
            corpus_bytes: TEXT_CORPUS_BYTES,
            corpus_records: TEXT_RECORD_COUNT,
            find_entries: FIND_ENTRY_COUNT,
            sort_records: SORT_RECORD_COUNT,
            grep_median_ms,
            find_median_ms,
            redirected_cat_median_ms,
            redirected_sort_median_ms,
            grep_target_ms: GREP_CACHED_TARGET_MS,
            find_target_ms: FIND_CACHED_TARGET_MS,
            redirected_cat_target_ms: REDIRECTED_CAT_CACHED_TARGET_MS,
            redirected_sort_target_ms: REDIRECTED_SORT_CACHED_TARGET_MS,
            grep_ms,
            find_ms,
            redirected_cat_ms: cat_ms,
            redirected_sort_ms: sort_ms,
        };
        eprintln!(
            "WINGMAN_RUNNER_TIMING_V1={}",
            serde_json::to_string(&report).expect("serialize timing report")
        );
    });

    fs::remove_dir_all(&sandbox).expect("remove performance sandbox");
    if let Err(payload) = result {
        std::panic::resume_unwind(payload);
    }
}

#[test]
#[ignore = "release fixture preparation: persists a 100 MiB corpus and 20,000-entry tree"]
fn prepare_uncached_runner_fixture() {
    let root = required_fixture_root();
    if root.exists() {
        assert!(
            root.join(UNCACHED_FIXTURE_MARKER).is_file(),
            "refusing to reuse an unmarked fixture directory: {}",
            root.display()
        );
        validate_uncached_fixture(&root);
        eprintln!("WINGMAN_RUNNER_UNCACHED_FIXTURE_V1={}", root.display());
        return;
    }

    fs::create_dir(&root).expect("create uncached fixture root");
    let result = std::panic::catch_unwind(|| {
        create_text_corpus(&root.join("text-100mib.txt"));
        create_find_tree(&root.join("find-20000"));
        create_sort_corpus(&root.join("sort-200000.txt"));
        File::create(root.join(UNCACHED_FIXTURE_MARKER)).expect("create uncached fixture marker");
        validate_uncached_fixture(&root);
        eprintln!("WINGMAN_RUNNER_UNCACHED_FIXTURE_V1={}", root.display());
    });
    if let Err(payload) = result {
        let _ = fs::remove_dir_all(&root);
        std::panic::resume_unwind(payload);
    }
}

#[test]
#[ignore = "release uncached sample: run once per operation after a controlled Windows restart"]
fn uncached_runner_timing_sample() {
    let root = required_fixture_root();
    validate_uncached_fixture(&root);
    let operation = std::env::var(UNCACHED_OPERATION_ENV)
        .expect("WINGMAN_UNCACHED_OPERATION must name grep, find, cat, or sort");
    let text_corpus = root.join("text-100mib.txt");
    let find_root = root.join("find-20000");
    let sort_corpus = root.join("sort-200000.txt");
    let output = root.join(format!(
        ".wingman-uncached-output-{}-{}.txt",
        operation,
        Uuid::new_v4().as_simple()
    ));

    let measurement = match operation.as_str() {
        "grep" => {
            let measurement = run_runner(
                &root,
                execute_request(
                    vec![StagePlanV1::SearchText {
                        pattern: "WINGMAN_PERF_MATCH".to_string(),
                        paths: vec![path_spec(&text_corpus)],
                        ignore_case: false,
                        line_numbers: false,
                        invert_match: false,
                        fixed_strings: true,
                        recursive: false,
                    }],
                    None,
                ),
            );
            validate_grep_output(&measurement);
            measurement
        }
        "find" => {
            let measurement = run_runner(
                &root,
                execute_request(
                    vec![StagePlanV1::FindPaths {
                        path: path_spec(&find_root),
                        entry_type: None,
                        name_pattern: None,
                        ignore_case: false,
                        min_depth: 1,
                        max_depth: None,
                    }],
                    None,
                ),
            );
            validate_find_output(&measurement);
            measurement
        }
        "cat" => {
            let measurement = run_runner(
                &root,
                execute_request(
                    vec![StagePlanV1::ReadTextFiles {
                        paths: vec![path_spec(&text_corpus)],
                        number_lines: false,
                    }],
                    Some(&output),
                ),
            );
            validate_cat_output(&measurement, &output);
            measurement
        }
        "sort" => {
            let measurement = run_runner(
                &root,
                execute_request(
                    vec![StagePlanV1::SortLines {
                        path: Some(path_spec(&sort_corpus)),
                        reverse: false,
                        numeric: false,
                        unique: false,
                    }],
                    Some(&output),
                ),
            );
            validate_sort_output(&measurement, &output);
            measurement
        }
        _ => panic!("WINGMAN_UNCACHED_OPERATION must name grep, find, cat, or sort"),
    };
    let _ = fs::remove_file(&output);

    let report = UncachedTimingSampleV1 {
        cache_state: "fixture-first-read",
        operation,
        elapsed_ms: duration_ms(measurement.elapsed),
        corpus_bytes: TEXT_CORPUS_BYTES,
        corpus_records: TEXT_RECORD_COUNT,
        find_entries: FIND_ENTRY_COUNT,
        sort_records: SORT_RECORD_COUNT,
    };
    eprintln!(
        "WINGMAN_RUNNER_UNCACHED_V1={}",
        serde_json::to_string(&report).expect("serialize uncached timing sample")
    );
}

fn create_text_corpus(path: &Path) {
    let mut writer = BufWriter::new(File::create(path).expect("create 100 MiB text corpus"));
    let mut record = [b'x'; TEXT_RECORD_BYTES];
    record[TEXT_RECORD_BYTES - 1] = b'\n';
    for index in 0..TEXT_RECORD_COUNT {
        if index + 1 == TEXT_RECORD_COUNT {
            record[..18].copy_from_slice(b"WINGMAN_PERF_MATCH");
        }
        writer.write_all(&record).expect("write text corpus record");
    }
    writer.flush().expect("flush text corpus");
    assert_eq!(fs::metadata(path).unwrap().len(), TEXT_CORPUS_BYTES as u64);
}

fn create_find_tree(root: &Path) {
    fs::create_dir(root).expect("create find root");
    for directory_index in 0..FIND_DIRECTORY_COUNT {
        let directory = root.join(format!("d-{directory_index:03}"));
        fs::create_dir(&directory).expect("create find directory");
        for file_index in 0..FIND_FILES_PER_DIRECTORY {
            File::create(directory.join(format!("f-{file_index:03}.txt")))
                .expect("create find file");
        }
    }
    assert_eq!(
        FIND_DIRECTORY_COUNT + FIND_DIRECTORY_COUNT * FIND_FILES_PER_DIRECTORY,
        FIND_ENTRY_COUNT
    );
}

fn create_sort_corpus(path: &Path) {
    let mut writer = BufWriter::new(File::create(path).expect("create sort corpus"));
    for value in (0..SORT_RECORD_COUNT).rev() {
        writeln!(writer, "{value:06}:{value:024}").expect("write sort record");
    }
    writer.flush().expect("flush sort corpus");
    assert_eq!(
        fs::metadata(path).unwrap().len(),
        (SORT_RECORD_COUNT * SORT_RECORD_BYTES) as u64
    );
}

fn execute_request(stages: Vec<StagePlanV1>, output: Option<&Path>) -> PreparedRequestV1 {
    PreparedRequestV1 {
        protocol: "wingman.run".to_string(),
        version: 1,
        kind: PreparedRequestKindV1::Execute {
            plan: ExecutionPlanV1 {
                stages,
                redirect: output.map(|path| ValidatedRedirectPlanV1 {
                    mode: RedirectModeV1::Overwrite,
                    path: path_spec(path),
                }),
            },
        },
    }
}

fn run_runner(cwd: &Path, request: PreparedRequestV1) -> RunnerMeasurementV1 {
    let request_id = Uuid::new_v4().as_simple().to_string();
    let pipe_name = format!(
        r"\\.\pipe\wingman-runner-perf-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let broker = OneShotBrokerV1::bind(&pipe_name, request_id.clone(), request)
        .expect("bind performance broker");
    let server = thread::spawn(move || broker.serve());

    let started = Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_wingman-runner"))
        .arg(&request_id)
        .env("WINGMAN_BROKER_PIPE", &pipe_name)
        .current_dir(cwd)
        .output()
        .expect("start release runner");
    let elapsed = started.elapsed();
    server
        .join()
        .expect("performance broker thread")
        .expect("serve performance request");
    RunnerMeasurementV1 { elapsed, output }
}

fn validate_grep(measurement: RunnerMeasurementV1) {
    validate_grep_output(&measurement);
}

fn validate_grep_output(measurement: &RunnerMeasurementV1) {
    assert_success(&measurement.output, "grep");
    assert_eq!(
        measurement
            .output
            .stdout
            .windows(18)
            .filter(|window| *window == b"WINGMAN_PERF_MATCH")
            .count(),
        1
    );
    assert!(measurement.output.stdout.ends_with(b"\r\n"));
}

fn validate_find(measurement: RunnerMeasurementV1) {
    validate_find_output(&measurement);
}

fn validate_find_output(measurement: &RunnerMeasurementV1) {
    assert_success(&measurement.output, "find");
    assert_eq!(
        measurement
            .output
            .stdout
            .windows(2)
            .filter(|window| *window == b"\r\n")
            .count(),
        FIND_ENTRY_COUNT
    );
}

fn validate_cat(measurement: RunnerMeasurementV1, output: &Path) {
    validate_cat_output(&measurement, output);
}

fn validate_cat_output(measurement: &RunnerMeasurementV1, output: &Path) {
    assert_success(&measurement.output, "cat");
    assert!(measurement.output.stdout.is_empty());
    assert_eq!(
        fs::metadata(output).unwrap().len(),
        (TEXT_CORPUS_BYTES + TEXT_RECORD_COUNT) as u64
    );
}

fn validate_sort(measurement: RunnerMeasurementV1, output: &Path) {
    validate_sort_output(&measurement, output);
}

fn validate_sort_output(measurement: &RunnerMeasurementV1, output: &Path) {
    assert_success(&measurement.output, "sort");
    assert!(measurement.output.stdout.is_empty());
    let bytes = fs::read(output).expect("read sorted output");
    assert_eq!(bytes.len(), SORT_RECORD_COUNT * (SORT_RECORD_BYTES + 1));
    assert!(bytes.starts_with(b"000000:000000000000000000000000\r\n"));
    assert!(bytes.ends_with(b"199999:000000000000000000199999\r\n"));
}

fn assert_success(output: &Output, operation: &str) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{operation} runner failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{operation} emitted stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn median(samples: &[f64]) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[sorted.len() / 2]
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn assert_target(operation: &str, median_ms: f64, target_ms: f64, samples: &[f64]) {
    assert!(
        median_ms <= target_ms,
        "{operation} cached median {median_ms:.1} ms exceeded {target_ms:.1} ms; samples={samples:?}"
    );
}

fn path_spec(path: &Path) -> wingman_lib::windows_path::ValidatedPathSpecV1 {
    validate_path_value(&path.to_string_lossy()).expect("validate performance path")
}

fn sandbox() -> PathBuf {
    std::env::temp_dir().join(format!(
        "wingman-runner-performance-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    ))
}

fn required_fixture_root() -> PathBuf {
    let value = std::env::var_os(UNCACHED_FIXTURE_ENV)
        .expect("WINGMAN_UNCACHED_FIXTURE_ROOT must name an explicit fixture directory");
    let root = PathBuf::from(value);
    assert!(root.is_absolute(), "uncached fixture root must be absolute");
    root
}

fn validate_uncached_fixture(root: &Path) {
    assert!(
        root.join(UNCACHED_FIXTURE_MARKER).is_file(),
        "uncached fixture marker is missing"
    );
    assert_eq!(
        fs::metadata(root.join("text-100mib.txt"))
            .expect("stat uncached text corpus")
            .len(),
        TEXT_CORPUS_BYTES as u64
    );
    assert!(root.join("find-20000").is_dir());
    assert_eq!(
        fs::metadata(root.join("sort-200000.txt"))
            .expect("stat uncached sort corpus")
            .len(),
        (SORT_RECORD_COUNT * SORT_RECORD_BYTES) as u64
    );
}
