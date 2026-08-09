use wingman_lib::interpreter::{
    ActiveShell, ExecutionPlanV1, FrontendDecisionKindV1, InterpreterSession, LineEvidence,
    PrepareSubmissionV1, PreparedRequestKindV1, PreparedRequestV1, RunnerRequestValidationErrorV1,
};
use wingman_lib::runner::{
    dispatch_prepared, execute_prepared, RunnerDispatchErrorV1, RunnerOutcomeV1,
};

#[test]
fn opaque_rejection_id_produces_one_shot_runner_output_and_status() {
    let mut session = InterpreterSession::new(121, 1, ActiveShell::Cmd);
    let decision = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 121,
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

    assert_eq!(
        dispatch_prepared(&mut session, &request_id),
        Ok(RunnerOutcomeV1 {
            stdout: Vec::new(),
            stderr: b"wingman grep: unsupported option\r\n".to_vec(),
            exit_code: 2,
        })
    );
    assert_eq!(
        dispatch_prepared(&mut session, &request_id),
        Err(RunnerDispatchErrorV1::UnknownOrConsumedRequest)
    );
}

#[test]
fn prepared_pwd_uses_the_runner_process_working_directory() {
    let mut session = InterpreterSession::new(121, 2, ActiveShell::WindowsPowerShell);
    let decision = session
        .prepare_submission(PrepareSubmissionV1 {
            session_id: 121,
            command_sequence: 2,
            shell: ActiveShell::WindowsPowerShell,
            familiar_enabled: true,
            evidence: LineEvidence::Reliable,
            raw_line: "pwd".to_string(),
        })
        .expect("current prompt");
    let request_id = match decision.decision {
        FrontendDecisionKindV1::InvokePrepared { request_id, .. } => request_id,
        other => panic!("expected prepared execution, got {other:?}"),
    };
    let cwd = std::env::current_dir().expect("test working directory");

    assert_eq!(
        dispatch_prepared(&mut session, &request_id),
        Ok(RunnerOutcomeV1 {
            stdout: format!("{}\r\n", cwd.display()).into_bytes(),
            stderr: Vec::new(),
            exit_code: 0,
        })
    );
}

#[test]
fn prepared_familiar_control_prints_only_its_validated_response() {
    assert_eq!(
        execute_prepared(PreparedRequestV1 {
            protocol: "wingman.run".to_string(),
            version: 1,
            kind: PreparedRequestKindV1::Control {
                response: "Familiar: OFF".to_string(),
                exit_code: 0,
            },
        }),
        Ok(RunnerOutcomeV1 {
            stdout: b"Familiar: OFF\r\n".to_vec(),
            stderr: Vec::new(),
            exit_code: 0,
        })
    );
}

#[test]
fn direct_execution_cannot_bypass_runner_request_validation() {
    assert_eq!(
        execute_prepared(PreparedRequestV1 {
            protocol: "wingman.run".to_string(),
            version: 1,
            kind: PreparedRequestKindV1::Execute {
                plan: ExecutionPlanV1 {
                    stages: Vec::new(),
                    redirect: None,
                },
            },
        }),
        Err(RunnerDispatchErrorV1::InvalidPreparedRequest(
            RunnerRequestValidationErrorV1::InvalidStageCount
        ))
    );
}
