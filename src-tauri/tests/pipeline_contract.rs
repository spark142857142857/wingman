use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use wingman_lib::pipeline::{
    resolve_pipeline_status, run_head_pipeline, run_head_pipeline_with_cancellation,
    CancellationTokenV1, PipelineFailureV1, PipelineRunV1, PipelineStatusV1, StageOutcomeV1,
};
use wingman_lib::text_stream::{RecordFrameV1, Utf8RecordReaderV1};

#[test]
fn early_head_stop_does_not_pull_or_validate_the_unread_suffix() {
    let pulls = Arc::new(AtomicUsize::new(0));
    let source_pulls = pulls.clone();
    let mut step = 0;
    let source = std::iter::from_fn(move || {
        source_pulls.fetch_add(1, Ordering::SeqCst);
        step += 1;
        match step {
            1 => Some(Ok(record("first", true))),
            2 => Some(Err(PipelineFailureV1::new("invalid unread suffix"))),
            _ => None,
        }
    });

    let run = run_head_pipeline(source, 1, 1).expect("run bounded head pipeline");

    assert_eq!(pulls.load(Ordering::SeqCst), 1);
    assert_eq!(
        run,
        PipelineRunV1 {
            records: vec![record("first", true)],
            source_outcome: StageOutcomeV1::StoppedNormally,
            final_outcome: StageOutcomeV1::Success { exit_code: 0 },
            exit_code: 0,
            diagnostic: None,
        }
    );
}

#[test]
fn head_zero_stops_before_the_source_produces_any_record() {
    let pulls = Arc::new(AtomicUsize::new(0));
    let source_pulls = pulls.clone();
    let source = std::iter::from_fn(move || {
        source_pulls.fetch_add(1, Ordering::SeqCst);
        Some(Ok(record("must not be read", true)))
    });

    let run = run_head_pipeline(source, 0, 1).expect("run head zero");

    assert_eq!(pulls.load(Ordering::SeqCst), 0);
    assert!(run.records.is_empty());
    assert_eq!(run.source_outcome, StageOutcomeV1::StoppedNormally);
    assert_eq!(run.exit_code, 0);
}

#[test]
fn operational_failure_observed_before_stop_dominates_final_success() {
    let source = vec![
        Ok(record("partial", true)),
        Err(PipelineFailureV1::new("decode failed at byte 7")),
    ]
    .into_iter();

    let run = run_head_pipeline(source, 5, 1).expect("run failing pipeline");

    assert_eq!(run.records, vec![record("partial", true)]);
    assert_eq!(
        run.source_outcome,
        StageOutcomeV1::OperationalFailure {
            diagnostic: "decode failed at byte 7".to_string(),
        }
    );
    assert_eq!(run.exit_code, 1);
    assert_eq!(run.diagnostic.as_deref(), Some("decode failed at byte 7"));
}

#[test]
fn source_completion_preserves_the_final_record_termination_state() {
    let source = vec![Ok(record("first", true)), Ok(record("last", false))].into_iter();

    let run = run_head_pipeline(source, 10, 1).expect("run complete pipeline");

    assert_eq!(
        run.records,
        vec![record("first", true), record("last", false)]
    );
    assert_eq!(run.source_outcome, StageOutcomeV1::Success { exit_code: 0 });
    assert_eq!(run.final_outcome, StageOutcomeV1::Success { exit_code: 0 });
    assert_eq!(run.exit_code, 0);
}

#[test]
fn cancellation_and_stage_status_priority_follow_the_contract() {
    assert_eq!(
        resolve_pipeline_status(
            &StageOutcomeV1::OperationalFailure {
                diagnostic: "source failed".to_string(),
            },
            &StageOutcomeV1::Cancelled,
        ),
        PipelineStatusV1 {
            exit_code: 130,
            diagnostic: None,
        }
    );
    assert_eq!(
        resolve_pipeline_status(
            &StageOutcomeV1::Result { exit_code: 1 },
            &StageOutcomeV1::Success { exit_code: 0 },
        ),
        PipelineStatusV1 {
            exit_code: 0,
            diagnostic: None,
        }
    );
    assert_eq!(
        resolve_pipeline_status(
            &StageOutcomeV1::Success { exit_code: 0 },
            &StageOutcomeV1::Result { exit_code: 1 },
        ),
        PipelineStatusV1 {
            exit_code: 1,
            diagnostic: None,
        }
    );
    assert_eq!(
        resolve_pipeline_status(
            &StageOutcomeV1::OperationalFailure {
                diagnostic: "stage zero".to_string(),
            },
            &StageOutcomeV1::OperationalFailure {
                diagnostic: "stage one".to_string(),
            },
        ),
        PipelineStatusV1 {
            exit_code: 1,
            diagnostic: Some("stage zero".to_string()),
        }
    );
}

#[test]
fn decoded_file_source_and_head_share_the_same_early_stop_boundary() {
    let bytes = [b"good\n".as_slice(), &[0xff, b'\n']].concat();
    let source = Utf8RecordReaderV1::new(std::io::Cursor::new(bytes))
        .map(|record| record.map_err(|error| PipelineFailureV1::new(format!("{error:?}"))));

    let run = run_head_pipeline(source, 1, 1).expect("run decoded head pipeline");

    assert_eq!(run.records, vec![record("good", true)]);
    assert_eq!(run.source_outcome, StageOutcomeV1::StoppedNormally);
    assert_eq!(run.exit_code, 0);
}

#[test]
fn decoded_failure_before_head_completion_keeps_partial_records_and_exits_one() {
    let bytes = [b"good\n".as_slice(), &[0xff, b'\n']].concat();
    let source = Utf8RecordReaderV1::new(std::io::Cursor::new(bytes))
        .map(|record| record.map_err(|error| PipelineFailureV1::new(format!("{error:?}"))));

    let run = run_head_pipeline(source, 2, 1).expect("run decoded failing pipeline");

    assert_eq!(run.records, vec![record("good", true)]);
    assert_eq!(run.exit_code, 1);
    assert!(run
        .diagnostic
        .as_deref()
        .is_some_and(|diagnostic| diagnostic.contains("InvalidUtf8")));
}

#[test]
fn cancellation_before_start_pulls_no_source_data_and_exits_130() {
    let pulls = Arc::new(AtomicUsize::new(0));
    let source_pulls = pulls.clone();
    let source = std::iter::from_fn(move || {
        source_pulls.fetch_add(1, Ordering::SeqCst);
        Some(Ok(record("never", true)))
    });
    let cancellation = CancellationTokenV1::new();
    cancellation.cancel();

    let run = run_head_pipeline_with_cancellation(source, usize::MAX, 1, cancellation)
        .expect("cancel before start");

    assert_eq!(pulls.load(Ordering::SeqCst), 0);
    assert_eq!(run.source_outcome, StageOutcomeV1::Cancelled);
    assert_eq!(run.final_outcome, StageOutcomeV1::Cancelled);
    assert_eq!(run.exit_code, 130);
}

#[test]
fn cancellation_wakes_an_unbounded_pipeline_without_waiting_for_source_completion() {
    for _ in 0..25 {
        let source = std::iter::repeat_with(|| Ok(record("stream", true)));
        let cancellation = CancellationTokenV1::new();
        let canceller = cancellation.clone();
        let worker = std::thread::spawn(move || {
            run_head_pipeline_with_cancellation(source, usize::MAX, 1, cancellation)
        });
        std::thread::sleep(std::time::Duration::from_millis(2));
        canceller.cancel();

        let run = worker
            .join()
            .expect("pipeline worker")
            .expect("cancel unbounded pipeline");

        assert_eq!(run.exit_code, 130);
        assert!(matches!(run.source_outcome, StageOutcomeV1::Cancelled));
        assert!(matches!(run.final_outcome, StageOutcomeV1::Cancelled));
    }
}

fn record(text: &str, terminated: bool) -> RecordFrameV1 {
    RecordFrameV1 {
        text: text.to_string(),
        terminated,
    }
}
