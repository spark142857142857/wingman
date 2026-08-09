use serde_json::json;
use wingman_lib::interpreter::{
    decode_prepared_request, ActiveShell, FrontendDecisionKindV1, InterpreterSession, LineEvidence,
    PrepareSubmissionV1, RunnerRequestDecodeErrorV1, RunnerRequestValidationErrorV1,
    MAX_PIPELINE_STAGES, MAX_PREPARED_REQUEST_BYTES,
};

#[test]
fn prepared_rejection_has_a_versioned_bounded_wire_shape() {
    let mut session = InterpreterSession::new(91, 1, ActiveShell::Cmd);
    let decision = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 91,
            command_sequence: 1,
            shell: ActiveShell::Cmd,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: "grep -z TODO app.log".to_string(),
        })
        .expect("current prompt");
    let request_id = match decision.decision {
        FrontendDecisionKindV1::InvokePrepared { request_id, .. } => request_id,
        other => panic!("expected prepared rejection, got {other:?}"),
    };
    let prepared = session
        .consume_prepared(&request_id)
        .expect("one-shot request");

    assert_eq!(
        serde_json::to_value(prepared).expect("serialize prepared request"),
        json!({
            "protocol": "wingman.run",
            "version": 1,
            "kind": {
                "type": "reject",
                "diagnostic": "wingman grep: unsupported option",
                "exit_code": 2
            }
        })
    );
}

#[test]
fn runner_rejects_an_unsupported_protocol_version_before_execution() {
    let wire = br#"{
        "protocol":"wingman.run",
        "version":2,
        "kind":{
            "type":"reject",
            "diagnostic":"must not execute",
            "exit_code":2
        }
    }"#;

    assert_eq!(
        decode_prepared_request(wire),
        Err(RunnerRequestDecodeErrorV1::UnsupportedVersion {
            expected: 1,
            received: 2,
        })
    );
}

#[test]
fn runner_rejects_an_oversized_request_before_json_parsing() {
    let wire = vec![b' '; MAX_PREPARED_REQUEST_BYTES + 1];

    assert_eq!(
        decode_prepared_request(&wire),
        Err(RunnerRequestDecodeErrorV1::TooLarge {
            limit: MAX_PREPARED_REQUEST_BYTES,
            received: MAX_PREPARED_REQUEST_BYTES + 1,
        })
    );
}

#[test]
fn runner_rejects_unknown_nested_execution_fields() {
    let wire = br#"{
        "protocol":"wingman.run",
        "version":1,
        "kind":{
            "type":"execute",
            "plan":{
                "stages":[{"PrintWorkingDirectory":{"unexpected":true}}],
                "redirect":null
            }
        }
    }"#;

    assert_eq!(
        decode_prepared_request(wire),
        Err(RunnerRequestDecodeErrorV1::Malformed)
    );
}

#[test]
fn runner_rejects_terminal_control_characters_and_wrong_prepared_statuses() {
    let rejection = br#"{
        "protocol":"wingman.run",
        "version":1,
        "kind":{"type":"reject","diagnostic":"safe\u001b[2Junsafe","exit_code":2}
    }"#;
    assert_eq!(
        decode_prepared_request(rejection),
        Err(RunnerRequestDecodeErrorV1::InvalidRequest(
            RunnerRequestValidationErrorV1::InvalidDiagnostic
        ))
    );

    let control = br#"{
        "protocol":"wingman.run",
        "version":1,
        "kind":{"type":"control","response":"Familiar: ON","exit_code":2}
    }"#;
    assert_eq!(
        decode_prepared_request(control),
        Err(RunnerRequestDecodeErrorV1::InvalidRequest(
            RunnerRequestValidationErrorV1::InvalidExitCode
        ))
    );
}

#[test]
fn runner_rejects_empty_and_overlong_execution_pipelines() {
    let empty = br#"{
        "protocol":"wingman.run",
        "version":1,
        "kind":{"type":"execute","plan":{"stages":[],"redirect":null}}
    }"#;
    assert_eq!(
        decode_prepared_request(empty),
        Err(RunnerRequestDecodeErrorV1::InvalidRequest(
            RunnerRequestValidationErrorV1::InvalidStageCount
        ))
    );

    let stages = std::iter::repeat_n(r#""PrintWorkingDirectory""#, MAX_PIPELINE_STAGES + 1)
        .collect::<Vec<_>>()
        .join(",");
    let wire = format!(
        r#"{{"protocol":"wingman.run","version":1,"kind":{{"type":"execute","plan":{{"stages":[{stages}],"redirect":null}}}}}}"#
    );
    assert_eq!(
        decode_prepared_request(wire.as_bytes()),
        Err(RunnerRequestDecodeErrorV1::InvalidRequest(
            RunnerRequestValidationErrorV1::InvalidStageCount
        ))
    );
}

#[test]
fn runner_revalidates_serialized_path_specs_and_pipeline_shapes() {
    let forged_path = br#"{
        "protocol":"wingman.run",
        "version":1,
        "kind":{"type":"execute","plan":{
            "stages":[{"ReadTextFiles":{"paths":[{
                "original":"safe.txt",
                "kind":"Relative",
                "components":["different.txt"]
            }],"number_lines":false}}],
            "redirect":null
        }}
    }"#;
    assert_eq!(
        decode_prepared_request(forged_path),
        Err(RunnerRequestDecodeErrorV1::InvalidRequest(
            RunnerRequestValidationErrorV1::InvalidPath
        ))
    );

    let invalid_shape = br#"{
        "protocol":"wingman.run",
        "version":1,
        "kind":{"type":"execute","plan":{
            "stages":[{"HeadLines":{"count":10,"path":null}}],
            "redirect":null
        }}
    }"#;
    assert_eq!(
        decode_prepared_request(invalid_shape),
        Err(RunnerRequestDecodeErrorV1::InvalidRequest(
            RunnerRequestValidationErrorV1::InvalidStageShape
        ))
    );

    let invalid_sort_source = br#"{
        "protocol":"wingman.run",
        "version":1,
        "kind":{"type":"execute","plan":{
            "stages":[{"SortLines":{
                "path":null,"reverse":false,"numeric":true,"unique":false
            }}],
            "redirect":null
        }}
    }"#;
    assert_eq!(
        decode_prepared_request(invalid_sort_source),
        Err(RunnerRequestDecodeErrorV1::InvalidRequest(
            RunnerRequestValidationErrorV1::InvalidStageShape
        ))
    );
}
