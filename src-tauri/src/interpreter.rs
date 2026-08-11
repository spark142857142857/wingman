use crate::catalog::{build_execution_plan, CatalogErrorV1};
use crate::grep_pattern::GrepPatternV1;
use crate::lexer::{lex_p0_line, LexErrorV1};
use crate::parser::{parse_p0_tokens, ParseErrorV1};
use crate::windows_path::{validate_executable_name, validate_path_value, ValidatedPathSpecV1};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

pub const MAX_PREPARED_REQUEST_BYTES: usize = 64 * 1024;
pub const MAX_PIPELINE_STAGES: usize = 16;
pub const MAX_PATH_OPERANDS: usize = 128;
pub const MAX_PREPARED_DIAGNOSTIC_BYTES: usize = 4 * 1024;
pub const MAX_CONTROL_RESPONSE_BYTES: usize = 256;
pub const MAX_HEAD_LINE_COUNT: usize = u32::MAX as usize;
pub const MAX_GREP_PATTERN_BYTES: usize = 16 * 1024;
pub const MAX_FIND_DEPTH_VALUE: usize = u32::MAX as usize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActiveShell {
    Cmd,
    WindowsPowerShell,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LineEvidence {
    Reliable,
    Uncertain,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrepareSubmissionV1 {
    pub session_id: u64,
    pub command_sequence: u64,
    pub shell: ActiveShell,
    pub familiar_enabled: bool,
    pub evidence: LineEvidence,
    pub raw_line: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendDecisionV1 {
    pub session_id: u64,
    pub command_sequence: u64,
    pub decision: FrontendDecisionKindV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontendDecisionKindV1 {
    PassThrough {
        raw_line: String,
    },
    InvokePrepared {
        request_id: String,
        display_line: String,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedRequestV1 {
    pub protocol: String,
    pub version: u16,
    pub kind: PreparedRequestKindV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPlanV1 {
    pub stages: Vec<StagePlanV1>,
    pub redirect: Option<ValidatedRedirectPlanV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub enum StagePlanV1 {
    PrintWorkingDirectory,
    ClearTerminal,
    FindExecutable {
        name: String,
    },
    CreateDirectories {
        paths: Vec<ValidatedPathSpecV1>,
        parents: bool,
    },
    TouchFiles {
        paths: Vec<ValidatedPathSpecV1>,
    },
    CopyPath {
        source: ValidatedPathSpecV1,
        destination: ValidatedPathSpecV1,
        recursive: bool,
        existing_destination: ExistingDestinationPolicyV1,
    },
    MovePath {
        source: ValidatedPathSpecV1,
        destination: ValidatedPathSpecV1,
        existing_destination: ExistingDestinationPolicyV1,
    },
    ListEntries {
        path: Option<ValidatedPathSpecV1>,
        include_hidden: bool,
        long: bool,
        human_readable: bool,
    },
    FindPaths {
        path: ValidatedPathSpecV1,
        entry_type: Option<FindEntryTypeV1>,
        name_pattern: Option<String>,
        ignore_case: bool,
        min_depth: usize,
        max_depth: Option<usize>,
    },
    ReadTextFiles {
        paths: Vec<ValidatedPathSpecV1>,
        number_lines: bool,
    },
    HeadLines {
        count: usize,
        path: Option<ValidatedPathSpecV1>,
    },
    TailLines {
        count: usize,
        path: Option<ValidatedPathSpecV1>,
    },
    FollowFile {
        count: usize,
        path: ValidatedPathSpecV1,
    },
    SearchText {
        pattern: String,
        paths: Vec<ValidatedPathSpecV1>,
        ignore_case: bool,
        line_numbers: bool,
        invert_match: bool,
        fixed_strings: bool,
        recursive: bool,
    },
    SortLines {
        path: Option<ValidatedPathSpecV1>,
        reverse: bool,
        numeric: bool,
        unique: bool,
    },
    UniqueLines {
        path: Option<ValidatedPathSpecV1>,
        count: bool,
        repeated_only: bool,
        unique_only: bool,
    },
    CountLines {
        path: Option<ValidatedPathSpecV1>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum FindEntryTypeV1 {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ExistingDestinationPolicyV1 {
    Replace,
    Force,
    NoClobber,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatedRedirectPlanV1 {
    pub mode: RedirectModeV1,
    pub path: ValidatedPathSpecV1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RedirectModeV1 {
    Overwrite,
    Append,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PreparedRequestKindV1 {
    Reject { diagnostic: String, exit_code: u8 },
    Execute { plan: ExecutionPlanV1 },
    Control { response: String, exit_code: u8 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FamiliarControlEffectV1 {
    Set(bool),
    Status,
}

impl FamiliarControlEffectV1 {
    pub fn enabled(self) -> Option<bool> {
        match self {
            Self::Set(enabled) => Some(enabled),
            Self::Status => None,
        }
    }
}

pub fn parse_familiar_control(raw_line: &str) -> Option<FamiliarControlEffectV1> {
    let mut words = raw_line.split_ascii_whitespace();
    let command = words.next()?;
    if !["familiar", "fam", "compat"]
        .iter()
        .any(|candidate| command.eq_ignore_ascii_case(candidate))
    {
        return None;
    }
    let action = words.next()?;
    if words.next().is_some() {
        return None;
    }
    if action.eq_ignore_ascii_case("on") {
        Some(FamiliarControlEffectV1::Set(true))
    } else if action.eq_ignore_ascii_case("off") {
        Some(FamiliarControlEffectV1::Set(false))
    } else if action.eq_ignore_ascii_case("status") {
        Some(FamiliarControlEffectV1::Status)
    } else {
        None
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunnerRequestDecodeErrorV1 {
    TooLarge { limit: usize, received: usize },
    Malformed,
    UnsupportedProtocol { received: String },
    UnsupportedVersion { expected: u16, received: u16 },
    InvalidRequest(RunnerRequestValidationErrorV1),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunnerRequestValidationErrorV1 {
    InvalidProtocol,
    InvalidVersion,
    InvalidDiagnostic,
    InvalidControlResponse,
    InvalidExitCode,
    InvalidStageCount,
    InvalidStageShape,
    InvalidPathCount,
    InvalidPath,
    InvalidRange,
}

pub fn decode_prepared_request(
    wire: &[u8],
) -> Result<PreparedRequestV1, RunnerRequestDecodeErrorV1> {
    if wire.len() > MAX_PREPARED_REQUEST_BYTES {
        return Err(RunnerRequestDecodeErrorV1::TooLarge {
            limit: MAX_PREPARED_REQUEST_BYTES,
            received: wire.len(),
        });
    }
    let request: PreparedRequestV1 =
        serde_json::from_slice(wire).map_err(|_| RunnerRequestDecodeErrorV1::Malformed)?;
    if request.protocol != "wingman.run" {
        return Err(RunnerRequestDecodeErrorV1::UnsupportedProtocol {
            received: request.protocol,
        });
    }
    if request.version != 1 {
        return Err(RunnerRequestDecodeErrorV1::UnsupportedVersion {
            expected: 1,
            received: request.version,
        });
    }
    validate_prepared_request(&request).map_err(RunnerRequestDecodeErrorV1::InvalidRequest)?;
    Ok(request)
}

pub fn validate_prepared_request(
    request: &PreparedRequestV1,
) -> Result<(), RunnerRequestValidationErrorV1> {
    if request.protocol != "wingman.run" {
        return Err(RunnerRequestValidationErrorV1::InvalidProtocol);
    }
    if request.version != 1 {
        return Err(RunnerRequestValidationErrorV1::InvalidVersion);
    }

    match &request.kind {
        PreparedRequestKindV1::Reject {
            diagnostic,
            exit_code,
        } => {
            if !is_bounded_terminal_text(diagnostic, MAX_PREPARED_DIAGNOSTIC_BYTES) {
                return Err(RunnerRequestValidationErrorV1::InvalidDiagnostic);
            }
            if *exit_code != 2 {
                return Err(RunnerRequestValidationErrorV1::InvalidExitCode);
            }
            Ok(())
        }
        PreparedRequestKindV1::Control {
            response,
            exit_code,
        } => {
            if !is_bounded_terminal_text(response, MAX_CONTROL_RESPONSE_BYTES) {
                return Err(RunnerRequestValidationErrorV1::InvalidControlResponse);
            }
            if *exit_code != 0 {
                return Err(RunnerRequestValidationErrorV1::InvalidExitCode);
            }
            Ok(())
        }
        PreparedRequestKindV1::Execute { plan } => validate_execution_plan(plan),
    }
}

pub fn validate_execution_plan(
    plan: &ExecutionPlanV1,
) -> Result<(), RunnerRequestValidationErrorV1> {
    if plan.stages.is_empty() || plan.stages.len() > MAX_PIPELINE_STAGES {
        return Err(RunnerRequestValidationErrorV1::InvalidStageCount);
    }

    let mut path_count = 0usize;
    if let Some(redirect) = &plan.redirect {
        validate_serialized_path(&redirect.path)?;
        path_count = 1;
    }

    if plan.stages.as_slice() == [StagePlanV1::PrintWorkingDirectory] {
        return if plan.redirect.is_none() {
            Ok(())
        } else {
            Err(RunnerRequestValidationErrorV1::InvalidStageShape)
        };
    }

    if plan.stages.as_slice() == [StagePlanV1::ClearTerminal] {
        return if plan.redirect.is_none() {
            Ok(())
        } else {
            Err(RunnerRequestValidationErrorV1::InvalidStageShape)
        };
    }

    if let [StagePlanV1::FindExecutable { name }] = plan.stages.as_slice() {
        return if plan.redirect.is_none()
            && validate_executable_name(name).ok().as_deref() == Some(name)
        {
            Ok(())
        } else {
            Err(RunnerRequestValidationErrorV1::InvalidStageShape)
        };
    }

    if let [StagePlanV1::CreateDirectories { paths, .. }] = plan.stages.as_slice() {
        if plan.redirect.is_some() || paths.is_empty() {
            return Err(RunnerRequestValidationErrorV1::InvalidStageShape);
        }
        if paths.len() > MAX_PATH_OPERANDS {
            return Err(RunnerRequestValidationErrorV1::InvalidPathCount);
        }
        for path in paths {
            validate_serialized_path(path)?;
        }
        return Ok(());
    }

    if let [StagePlanV1::TouchFiles { paths }] = plan.stages.as_slice() {
        if plan.redirect.is_some() || paths.is_empty() {
            return Err(RunnerRequestValidationErrorV1::InvalidStageShape);
        }
        if paths.len() > MAX_PATH_OPERANDS {
            return Err(RunnerRequestValidationErrorV1::InvalidPathCount);
        }
        for path in paths {
            validate_serialized_path(path)?;
        }
        return Ok(());
    }

    if let [StagePlanV1::CopyPath {
        source,
        destination,
        ..
    }] = plan.stages.as_slice()
    {
        if plan.redirect.is_some() {
            return Err(RunnerRequestValidationErrorV1::InvalidStageShape);
        }
        validate_serialized_path(source)?;
        validate_serialized_path(destination)?;
        return Ok(());
    }

    if let [StagePlanV1::MovePath {
        source,
        destination,
        ..
    }] = plan.stages.as_slice()
    {
        if plan.redirect.is_some() {
            return Err(RunnerRequestValidationErrorV1::InvalidStageShape);
        }
        validate_serialized_path(source)?;
        validate_serialized_path(destination)?;
        return Ok(());
    }

    let mut saw_recursive_search = false;
    for (index, stage) in plan.stages.iter().enumerate() {
        match stage {
            StagePlanV1::PrintWorkingDirectory
            | StagePlanV1::ClearTerminal
            | StagePlanV1::FindExecutable { .. }
            | StagePlanV1::CreateDirectories { .. }
            | StagePlanV1::TouchFiles { .. }
            | StagePlanV1::CopyPath { .. }
            | StagePlanV1::MovePath { .. } => {
                return Err(RunnerRequestValidationErrorV1::InvalidStageShape);
            }
            StagePlanV1::ListEntries {
                path,
                long,
                human_readable,
                ..
            } => {
                if index != 0 || (*human_readable && !*long) {
                    return Err(RunnerRequestValidationErrorV1::InvalidStageShape);
                }
                if let Some(path) = path {
                    path_count = path_count
                        .checked_add(1)
                        .ok_or(RunnerRequestValidationErrorV1::InvalidPathCount)?;
                    if path_count > MAX_PATH_OPERANDS {
                        return Err(RunnerRequestValidationErrorV1::InvalidPathCount);
                    }
                    validate_serialized_path(path)?;
                }
            }
            StagePlanV1::FindPaths {
                path,
                name_pattern,
                ignore_case,
                min_depth,
                max_depth,
                ..
            } => {
                if index != 0
                    || *min_depth > MAX_FIND_DEPTH_VALUE
                    || max_depth.is_some_and(|depth| depth > MAX_FIND_DEPTH_VALUE)
                    || (name_pattern.is_none() && *ignore_case)
                    || name_pattern.as_ref().is_some_and(|pattern| {
                        crate::find_pattern::FindPatternV1::compile(pattern, *ignore_case).is_err()
                    })
                {
                    return Err(RunnerRequestValidationErrorV1::InvalidStageShape);
                }
                path_count = path_count
                    .checked_add(1)
                    .ok_or(RunnerRequestValidationErrorV1::InvalidPathCount)?;
                if path_count > MAX_PATH_OPERANDS {
                    return Err(RunnerRequestValidationErrorV1::InvalidPathCount);
                }
                validate_serialized_path(path)?;
            }
            StagePlanV1::ReadTextFiles { paths, .. } => {
                if index != 0 || paths.is_empty() {
                    return Err(RunnerRequestValidationErrorV1::InvalidStageShape);
                }
                path_count = path_count
                    .checked_add(paths.len())
                    .ok_or(RunnerRequestValidationErrorV1::InvalidPathCount)?;
                if path_count > MAX_PATH_OPERANDS {
                    return Err(RunnerRequestValidationErrorV1::InvalidPathCount);
                }
                for path in paths {
                    validate_serialized_path(path)?;
                }
            }
            StagePlanV1::HeadLines { count, path } => {
                if *count > MAX_HEAD_LINE_COUNT {
                    return Err(RunnerRequestValidationErrorV1::InvalidRange);
                }
                match (index, path) {
                    (0, Some(path)) => {
                        path_count = path_count
                            .checked_add(1)
                            .ok_or(RunnerRequestValidationErrorV1::InvalidPathCount)?;
                        if path_count > MAX_PATH_OPERANDS {
                            return Err(RunnerRequestValidationErrorV1::InvalidPathCount);
                        }
                        validate_serialized_path(path)?;
                    }
                    (0, None) | (_, Some(_)) => {
                        return Err(RunnerRequestValidationErrorV1::InvalidStageShape);
                    }
                    (_, None) => {}
                }
            }
            StagePlanV1::TailLines { count, path } => {
                if *count > MAX_HEAD_LINE_COUNT {
                    return Err(RunnerRequestValidationErrorV1::InvalidRange);
                }
                match (index, path) {
                    (0, Some(path)) => {
                        path_count = path_count
                            .checked_add(1)
                            .ok_or(RunnerRequestValidationErrorV1::InvalidPathCount)?;
                        if path_count > MAX_PATH_OPERANDS {
                            return Err(RunnerRequestValidationErrorV1::InvalidPathCount);
                        }
                        validate_serialized_path(path)?;
                    }
                    (0, None) | (_, Some(_)) => {
                        return Err(RunnerRequestValidationErrorV1::InvalidStageShape);
                    }
                    (_, None) => {}
                }
            }
            StagePlanV1::FollowFile { count, path } => {
                if index != 0 || *count > MAX_HEAD_LINE_COUNT {
                    return Err(RunnerRequestValidationErrorV1::InvalidStageShape);
                }
                path_count = path_count
                    .checked_add(1)
                    .ok_or(RunnerRequestValidationErrorV1::InvalidPathCount)?;
                if path_count > MAX_PATH_OPERANDS {
                    return Err(RunnerRequestValidationErrorV1::InvalidPathCount);
                }
                validate_serialized_path(path)?;
            }
            StagePlanV1::SearchText {
                pattern,
                paths,
                ignore_case,
                fixed_strings,
                recursive,
                ..
            } => {
                if (*recursive && (index != 0 || saw_recursive_search))
                    || pattern.len() > MAX_GREP_PATTERN_BYTES
                    || pattern.contains(['\0', '\r', '\n'])
                    || GrepPatternV1::compile(pattern, *fixed_strings, *ignore_case).is_err()
                {
                    return Err(RunnerRequestValidationErrorV1::InvalidStageShape);
                }
                saw_recursive_search |= *recursive;
                match index {
                    0 if !paths.is_empty() => {
                        path_count = path_count
                            .checked_add(paths.len())
                            .ok_or(RunnerRequestValidationErrorV1::InvalidPathCount)?;
                        if path_count > MAX_PATH_OPERANDS {
                            return Err(RunnerRequestValidationErrorV1::InvalidPathCount);
                        }
                        for path in paths {
                            validate_serialized_path(path)?;
                        }
                    }
                    0 => return Err(RunnerRequestValidationErrorV1::InvalidStageShape),
                    _ if paths.is_empty() && !recursive => {}
                    _ => return Err(RunnerRequestValidationErrorV1::InvalidStageShape),
                }
            }
            StagePlanV1::SortLines { path, .. } => match (index, path) {
                (0, Some(path)) => {
                    path_count = path_count
                        .checked_add(1)
                        .ok_or(RunnerRequestValidationErrorV1::InvalidPathCount)?;
                    if path_count > MAX_PATH_OPERANDS {
                        return Err(RunnerRequestValidationErrorV1::InvalidPathCount);
                    }
                    validate_serialized_path(path)?;
                }
                (0, None) | (_, Some(_)) => {
                    return Err(RunnerRequestValidationErrorV1::InvalidStageShape);
                }
                (_, None) => {}
            },
            StagePlanV1::UniqueLines {
                path,
                repeated_only,
                unique_only,
                ..
            } => {
                if *repeated_only && *unique_only {
                    return Err(RunnerRequestValidationErrorV1::InvalidStageShape);
                }
                match (index, path) {
                    (0, Some(path)) => {
                        path_count = path_count
                            .checked_add(1)
                            .ok_or(RunnerRequestValidationErrorV1::InvalidPathCount)?;
                        if path_count > MAX_PATH_OPERANDS {
                            return Err(RunnerRequestValidationErrorV1::InvalidPathCount);
                        }
                        validate_serialized_path(path)?;
                    }
                    (0, None) | (_, Some(_)) => {
                        return Err(RunnerRequestValidationErrorV1::InvalidStageShape);
                    }
                    (_, None) => {}
                }
            }
            StagePlanV1::CountLines { path } => {
                if index + 1 != plan.stages.len() {
                    return Err(RunnerRequestValidationErrorV1::InvalidStageShape);
                }
                match (index, path) {
                    (0, Some(path)) => {
                        path_count = path_count
                            .checked_add(1)
                            .ok_or(RunnerRequestValidationErrorV1::InvalidPathCount)?;
                        if path_count > MAX_PATH_OPERANDS {
                            return Err(RunnerRequestValidationErrorV1::InvalidPathCount);
                        }
                        validate_serialized_path(path)?;
                    }
                    (0, None) | (_, Some(_)) => {
                        return Err(RunnerRequestValidationErrorV1::InvalidStageShape);
                    }
                    (_, None) => {}
                }
            }
        }
    }
    Ok(())
}

fn validate_serialized_path(
    path: &ValidatedPathSpecV1,
) -> Result<(), RunnerRequestValidationErrorV1> {
    if validate_path_value(&path.original).ok().as_ref() == Some(path) {
        Ok(())
    } else {
        Err(RunnerRequestValidationErrorV1::InvalidPath)
    }
}

fn is_bounded_terminal_text(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty() && value.len() <= maximum_bytes && !value.chars().any(char::is_control)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrepareSubmissionErrorV1 {
    SessionMismatch {
        expected: u64,
        received: u64,
    },
    CommandSequenceMismatch {
        expected: u64,
        received: u64,
    },
    ShellMismatch {
        expected: ActiveShell,
        received: ActiveShell,
    },
    AlreadyPrepared {
        command_sequence: u64,
    },
}

pub struct InterpreterSession {
    session_id: u64,
    command_sequence: u64,
    shell: ActiveShell,
    submission_prepared: bool,
    prepared: HashMap<String, PreparedRequestV1>,
}

impl InterpreterSession {
    pub fn new(session_id: u64, command_sequence: u64, shell: ActiveShell) -> Self {
        Self {
            session_id,
            command_sequence,
            shell,
            submission_prepared: false,
            prepared: HashMap::new(),
        }
    }

    pub fn prepare_submission(
        &mut self,
        request: PrepareSubmissionV1,
    ) -> Result<FrontendDecisionV1, PrepareSubmissionErrorV1> {
        if request.session_id != self.session_id {
            return Err(PrepareSubmissionErrorV1::SessionMismatch {
                expected: self.session_id,
                received: request.session_id,
            });
        }
        if request.command_sequence != self.command_sequence {
            return Err(PrepareSubmissionErrorV1::CommandSequenceMismatch {
                expected: self.command_sequence,
                received: request.command_sequence,
            });
        }
        if request.shell != self.shell {
            return Err(PrepareSubmissionErrorV1::ShellMismatch {
                expected: self.shell,
                received: request.shell,
            });
        }
        if self.submission_prepared {
            return Err(PrepareSubmissionErrorV1::AlreadyPrepared {
                command_sequence: self.command_sequence,
            });
        }

        if request.evidence == LineEvidence::Reliable {
            if let Some(effect) = parse_familiar_control(&request.raw_line) {
                let enabled = effect.enabled().unwrap_or(request.familiar_enabled);
                return Ok(self.prepare_request(
                    request.command_sequence,
                    request.raw_line,
                    PreparedRequestKindV1::Control {
                        response: format!("Familiar: {}", if enabled { "ON" } else { "OFF" }),
                        exit_code: 0,
                    },
                ));
            }
        }

        if request.familiar_enabled
            && request.evidence == LineEvidence::Reliable
            && request.raw_line.trim().eq_ignore_ascii_case("pwd")
        {
            return Ok(self.prepare_request(
                request.command_sequence,
                request.raw_line,
                PreparedRequestKindV1::Execute {
                    plan: ExecutionPlanV1 {
                        stages: vec![StagePlanV1::PrintWorkingDirectory],
                        redirect: None,
                    },
                },
            ));
        }

        if request.familiar_enabled && request.evidence == LineEvidence::Reliable {
            if let Some(kind) = classify_p0_submission(&request.raw_line) {
                return Ok(self.prepare_request(request.command_sequence, request.raw_line, kind));
            }
        }

        self.submission_prepared = true;
        Ok(FrontendDecisionV1 {
            session_id: self.session_id,
            command_sequence: request.command_sequence,
            decision: FrontendDecisionKindV1::PassThrough {
                raw_line: request.raw_line,
            },
        })
    }

    pub fn consume_prepared(&mut self, request_id: &str) -> Option<PreparedRequestV1> {
        self.prepared.remove(request_id)
    }

    pub fn synchronize_prompt(&mut self, command_sequence: u64, shell: ActiveShell) -> bool {
        if command_sequence <= self.command_sequence {
            return false;
        }

        self.command_sequence = command_sequence;
        self.shell = shell;
        self.submission_prepared = false;
        self.prepared.clear();
        true
    }

    fn prepare_request(
        &mut self,
        command_sequence: u64,
        display_line: String,
        kind: PreparedRequestKindV1,
    ) -> FrontendDecisionV1 {
        let request_id = Uuid::new_v4().as_simple().to_string();
        self.prepared.insert(
            request_id.clone(),
            PreparedRequestV1 {
                protocol: "wingman.run".to_string(),
                version: 1,
                kind,
            },
        );
        self.submission_prepared = true;

        FrontendDecisionV1 {
            session_id: self.session_id,
            command_sequence,
            decision: FrontendDecisionKindV1::InvokePrepared {
                request_id,
                display_line,
            },
        }
    }
}

fn classify_p0_submission(raw_line: &str) -> Option<PreparedRequestKindV1> {
    let command_name = claimed_p0_command(raw_line)?;
    let plan = lex_p0_line(raw_line)
        .map_err(|error| p0_lex_diagnostic(command_name, error))
        .and_then(|tokens| {
            parse_p0_tokens(&tokens).map_err(|error| p0_parse_diagnostic(command_name, error))
        })
        .and_then(|parsed| {
            build_execution_plan(&parsed)
                .map_err(|error| p0_catalog_diagnostic(command_name, error))
        });

    Some(match plan {
        Ok(plan) => PreparedRequestKindV1::Execute { plan },
        Err(diagnostic) => PreparedRequestKindV1::Reject {
            diagnostic,
            exit_code: 2,
        },
    })
}

fn claimed_p0_command(raw_line: &str) -> Option<&'static str> {
    let line = raw_line.trim_start_matches([' ', '\t']);
    let end = line
        .char_indices()
        .find_map(|(index, character)| {
            (character.is_ascii_whitespace() || matches!(character, '|' | '>' | '<' | '&' | ';'))
                .then_some(index)
        })
        .unwrap_or(line.len());
    let candidate = &line[..end];
    if candidate.eq_ignore_ascii_case("cat") {
        Some("cat")
    } else if candidate.eq_ignore_ascii_case("clear") {
        Some("clear")
    } else if candidate.eq_ignore_ascii_case("which") {
        Some("which")
    } else if candidate.eq_ignore_ascii_case("ls") {
        Some("ls")
    } else if candidate.eq_ignore_ascii_case("ll") {
        Some("ll")
    } else if candidate.eq_ignore_ascii_case("find") {
        Some("find")
    } else if candidate.eq_ignore_ascii_case("head") {
        Some("head")
    } else if candidate.eq_ignore_ascii_case("wc") {
        Some("wc")
    } else if candidate.eq_ignore_ascii_case("tail") {
        Some("tail")
    } else if candidate.eq_ignore_ascii_case("grep") {
        Some("grep")
    } else if candidate.eq_ignore_ascii_case("uniq") {
        Some("uniq")
    } else if candidate.eq_ignore_ascii_case("sort") {
        Some("sort")
    } else if candidate.eq_ignore_ascii_case("mkdir") {
        Some("mkdir")
    } else if candidate.eq_ignore_ascii_case("touch") {
        Some("touch")
    } else if candidate.eq_ignore_ascii_case("cp") {
        Some("cp")
    } else if candidate.eq_ignore_ascii_case("mv") {
        Some("mv")
    } else {
        None
    }
}

fn p0_lex_diagnostic(command_name: &str, error: LexErrorV1) -> String {
    let message = match error {
        LexErrorV1::UnclosedSingleQuote | LexErrorV1::UnclosedDoubleQuote => "unclosed quote",
        LexErrorV1::UnsupportedOperator => "unsupported shell operator",
        LexErrorV1::UnsupportedStreamRedirection => "unsupported stream redirection",
        LexErrorV1::ControlCharacter => "unsupported control character",
    };
    format!("wingman {command_name}: {message}")
}

fn p0_parse_diagnostic(command_name: &str, error: ParseErrorV1) -> String {
    let message = match error {
        ParseErrorV1::EmptyPipelineStage => "empty pipeline stage",
        ParseErrorV1::MissingRedirectTarget => "missing redirection target",
        ParseErrorV1::MultipleRedirects => "multiple redirections are unsupported",
        ParseErrorV1::RedirectNotFinal => "redirection must be final",
    };
    format!("wingman {command_name}: {message}")
}

fn p0_catalog_diagnostic(command_name: &str, error: CatalogErrorV1) -> String {
    let message = match error {
        CatalogErrorV1::UnsupportedCommand => "pipeline contains an unsupported command",
        CatalogErrorV1::UnsupportedOption => "unsupported option",
        CatalogErrorV1::MissingOperand if command_name == "mkdir" => "missing directory operand",
        CatalogErrorV1::MissingOperand => "missing file operand",
        CatalogErrorV1::InvalidCount => "invalid line count",
        CatalogErrorV1::InvalidSourceShape => "invalid pipeline source shape",
        CatalogErrorV1::InvalidPattern => "invalid pattern",
        CatalogErrorV1::InvalidName => "invalid executable name",
        CatalogErrorV1::ResourceLimit => "request exceeds a resource limit",
        CatalogErrorV1::Path(_) => "unsupported path",
    };
    format!("wingman {command_name}: {message}")
}
