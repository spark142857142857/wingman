use crate::interpreter::{
    ActiveShell, FrontendDecisionV1, InterpreterSession, LineEvidence, PrepareSubmissionErrorV1,
    PrepareSubmissionV1, PreparedRequestV1,
};
use crate::transport::{
    parse_editor_readiness_frame, EditorAdapterCapabilityV1, EditorLocationKindV1,
    EditorReadinessFrameV1,
};
use uuid::Uuid;

const MARKER_PREFIX: &str = "\u{1b}]777;wingman-prompt;";
const MARKER_TERMINATOR: char = '\u{7}';
const MAX_MARKER_BYTES: usize = 512;
const ESCAPE: char = '\u{1b}';
const MAX_INPUT_ESCAPE_BYTES: usize = 64;
const MAX_MIRRORED_LINE_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalInputActionV1 {
    Forward {
        data: String,
    },
    Prepared {
        decision: FrontendDecisionV1,
        editor: EditorSnapshotV1,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EditorSnapshotV1 {
    pub character_count: usize,
    pub cursor: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalPrepareErrorV1 {
    PromptNotValidated,
    Interpreter(PrepareSubmissionErrorV1),
}

pub struct TerminalSessionV1 {
    session_id: u64,
    expected_shell: ActiveShell,
    integration_nonce: String,
    expected_sequence: u64,
    editing_reliable: bool,
    readiness_cycle_dirty: bool,
    accept_pty_readiness: bool,
    interpreter: Option<InterpreterSession>,
    pending_output: String,
    input_buffer: Vec<char>,
    input_buffer_bytes: usize,
    input_cursor: usize,
    input_reliable: bool,
    input_escape: String,
    ignore_next_line_feed: bool,
}

impl TerminalSessionV1 {
    pub fn new(session_id: u64, expected_shell: ActiveShell) -> Self {
        Self {
            session_id,
            expected_shell,
            integration_nonce: Uuid::new_v4().as_simple().to_string(),
            expected_sequence: 1,
            editing_reliable: false,
            readiness_cycle_dirty: false,
            accept_pty_readiness: true,
            interpreter: None,
            pending_output: String::new(),
            input_buffer: Vec::new(),
            input_buffer_bytes: 0,
            input_cursor: 0,
            input_reliable: false,
            input_escape: String::new(),
            ignore_next_line_feed: false,
        }
    }

    pub fn integration_nonce(&self) -> &str {
        &self.integration_nonce
    }

    pub fn disable_pty_readiness(&mut self) {
        self.accept_pty_readiness = false;
        self.pending_output.clear();
    }

    pub fn editor_ready(&self) -> bool {
        self.editing_reliable && self.input_reliable && !self.readiness_cycle_dirty
    }

    pub fn ingest_pty_output(&mut self, chunk: &str) -> String {
        if !self.accept_pty_readiness {
            return chunk.to_string();
        }
        self.pending_output.push_str(chunk);
        let mut visible = String::new();

        loop {
            let Some(marker_start) = self.pending_output.find(MARKER_PREFIX) else {
                let retained = partial_prefix_suffix_len(&self.pending_output);
                let visible_end = self.pending_output.len() - retained;
                visible.push_str(&self.pending_output[..visible_end]);
                self.pending_output.drain(..visible_end);
                break;
            };

            visible.push_str(&self.pending_output[..marker_start]);
            self.pending_output.drain(..marker_start);

            let Some(terminator_offset) = self.pending_output.find(MARKER_TERMINATOR) else {
                if self.pending_output.len() > MAX_MARKER_BYTES {
                    let first_character_bytes = self
                        .pending_output
                        .chars()
                        .next()
                        .map(char::len_utf8)
                        .unwrap_or_default();
                    visible.push_str(&self.pending_output[..first_character_bytes]);
                    self.pending_output.drain(..first_character_bytes);
                    continue;
                }
                break;
            };

            let frame_end = terminator_offset + MARKER_TERMINATOR.len_utf8();
            let frame = self.pending_output[..frame_end].to_string();
            self.pending_output.drain(..frame_end);
            if !self.apply_prompt_marker(&frame) {
                visible.push_str(&frame);
            }
        }

        visible
    }

    pub fn prepare_submission(
        &mut self,
        raw_line: &str,
        familiar_enabled: bool,
    ) -> Result<FrontendDecisionV1, TerminalPrepareErrorV1> {
        if !self.editing_reliable || !self.input_reliable {
            return Err(TerminalPrepareErrorV1::PromptNotValidated);
        }
        let interpreter = self
            .interpreter
            .as_mut()
            .ok_or(TerminalPrepareErrorV1::PromptNotValidated)?;
        let decision = interpreter
            .prepare_submission(PrepareSubmissionV1 {
                session_id: self.session_id,
                command_sequence: self.expected_sequence,
                shell: self.expected_shell,
                familiar_enabled,
                evidence: LineEvidence::Reliable,
                raw_line: raw_line.to_string(),
            })
            .map_err(TerminalPrepareErrorV1::Interpreter)?;
        self.editing_reliable = false;
        self.expected_sequence = self.expected_sequence.saturating_add(1);
        Ok(decision)
    }

    pub fn handle_terminal_input(
        &mut self,
        data: &str,
        familiar_enabled: bool,
    ) -> Vec<TerminalInputActionV1> {
        if !self.editing_reliable {
            self.record_unvalidated_input(data);
            return vec![TerminalInputActionV1::Forward {
                data: data.to_string(),
            }];
        }
        let submission_count = logical_submission_count(data);
        if submission_count > 1 || text_follows_first_line_boundary(data) {
            self.input_buffer.clear();
            self.input_buffer_bytes = 0;
            self.input_cursor = 0;
            self.input_reliable = false;
            self.input_escape.clear();
            self.editing_reliable = false;
            self.expected_sequence = self
                .expected_sequence
                .saturating_add(submission_count.max(1) as u64);
            return vec![TerminalInputActionV1::Forward {
                data: data.to_string(),
            }];
        }

        let mut actions = Vec::new();
        for character in data.chars() {
            if !self.input_escape.is_empty() {
                self.input_escape.push(character);
                if input_escape_complete(&self.input_escape)
                    || self.input_escape.len() > MAX_INPUT_ESCAPE_BYTES
                {
                    let sequence = std::mem::take(&mut self.input_escape);
                    if !self.apply_editing_sequence(&sequence) {
                        self.input_reliable = false;
                    }
                    push_forward(&mut actions, sequence);
                }
                continue;
            }

            if character == ESCAPE {
                self.input_escape.push(character);
                continue;
            }

            if character == '\n' && self.ignore_next_line_feed {
                self.ignore_next_line_feed = false;
                continue;
            }
            self.ignore_next_line_feed = false;

            if matches!(character, '\r' | '\n') {
                if self.input_reliable {
                    let raw_line: String = self.input_buffer.iter().collect();
                    let editor = EditorSnapshotV1 {
                        character_count: self.input_buffer.len(),
                        cursor: self.input_cursor,
                    };
                    match self.prepare_submission(&raw_line, familiar_enabled) {
                        Ok(decision) => {
                            actions.push(TerminalInputActionV1::Prepared { decision, editor })
                        }
                        Err(_) => push_forward(&mut actions, character.to_string()),
                    }
                } else {
                    push_forward(&mut actions, character.to_string());
                    self.editing_reliable = false;
                    self.expected_sequence = self.expected_sequence.saturating_add(1);
                }
                self.input_buffer.clear();
                self.input_buffer_bytes = 0;
                self.input_cursor = 0;
                self.input_reliable = false;
                self.ignore_next_line_feed = character == '\r';
                continue;
            }

            if character == '\u{3}' {
                self.input_buffer.clear();
                self.input_buffer_bytes = 0;
                self.input_cursor = 0;
                self.input_reliable = false;
                self.input_escape.clear();
                self.editing_reliable = false;
                self.expected_sequence = self.expected_sequence.saturating_add(1);
                push_forward(&mut actions, character.to_string());
                continue;
            }

            if matches!(character, '\u{7f}' | '\u{8}') {
                if self.input_reliable && self.input_cursor > 0 {
                    self.input_cursor -= 1;
                    let removed = self.input_buffer.remove(self.input_cursor);
                    self.input_buffer_bytes -= removed.len_utf8();
                }
                push_forward(&mut actions, character.to_string());
                continue;
            }

            if character == '\t' || character < ' ' {
                self.input_reliable = false;
            } else if self.input_reliable && character != '\u{7f}' {
                if self.input_buffer_bytes + character.len_utf8() > MAX_MIRRORED_LINE_BYTES {
                    self.input_reliable = false;
                    self.input_buffer.clear();
                    self.input_buffer_bytes = 0;
                    self.input_cursor = 0;
                } else {
                    self.input_buffer.insert(self.input_cursor, character);
                    self.input_buffer_bytes += character.len_utf8();
                    self.input_cursor += 1;
                }
            }
            push_forward(&mut actions, character.to_string());
        }
        actions
    }

    pub fn consume_prepared(&mut self, request_id: &str) -> Option<PreparedRequestV1> {
        self.interpreter
            .as_mut()
            .and_then(|interpreter| interpreter.consume_prepared(request_id))
    }

    pub fn suspend_for_native_paste(&mut self, data: &str) {
        self.input_buffer.clear();
        self.input_buffer_bytes = 0;
        self.input_cursor = 0;
        self.input_reliable = false;
        self.input_escape.clear();
        self.ignore_next_line_feed = false;
        self.editing_reliable = false;
        self.readiness_cycle_dirty = !matches!(data.chars().last(), Some('\r' | '\n'));
        self.expected_sequence = self
            .expected_sequence
            .saturating_add(logical_submission_count(data).max(1) as u64);
    }

    pub fn suspend_after_transport_failure(&mut self) {
        self.input_buffer.clear();
        self.input_buffer_bytes = 0;
        self.input_cursor = 0;
        self.input_reliable = false;
        self.input_escape.clear();
        self.ignore_next_line_feed = false;
        self.editing_reliable = false;
        self.readiness_cycle_dirty = true;
    }

    pub fn apply_editor_readiness(&mut self, frame: &EditorReadinessFrameV1) -> bool {
        if self.expected_shell == ActiveShell::Cmd
            || self.readiness_cycle_dirty
            || frame.nonce != self.integration_nonce
            || frame.sequence != self.expected_sequence
            || frame.shell != self.expected_shell
            || frame.shell_depth != 0
            || frame.location_kind != EditorLocationKindV1::FileSystem
            || frame.adapter_capability != EditorAdapterCapabilityV1::PsReadLineReplaceV1
        {
            return false;
        }

        if let Some(interpreter) = self.interpreter.as_mut() {
            if !interpreter.synchronize_prompt(self.expected_sequence, self.expected_shell) {
                return false;
            }
        } else {
            self.interpreter = Some(InterpreterSession::new(
                self.session_id,
                self.expected_sequence,
                self.expected_shell,
            ));
        }
        self.editing_reliable = true;
        self.readiness_cycle_dirty = false;
        self.input_buffer.clear();
        self.input_buffer_bytes = 0;
        self.input_cursor = 0;
        self.input_reliable = true;
        self.input_escape.clear();
        self.ignore_next_line_feed = false;
        true
    }

    fn record_unvalidated_input(&mut self, data: &str) {
        for character in data.chars() {
            if character == '\n' && self.ignore_next_line_feed {
                self.ignore_next_line_feed = false;
                continue;
            }
            self.ignore_next_line_feed = false;
            match character {
                '\r' => {
                    self.expected_sequence = self.expected_sequence.saturating_add(1);
                    self.readiness_cycle_dirty = false;
                    self.ignore_next_line_feed = true;
                }
                '\n' => {
                    self.expected_sequence = self.expected_sequence.saturating_add(1);
                    self.readiness_cycle_dirty = false;
                }
                _ => self.readiness_cycle_dirty = true,
            }
        }
    }

    fn apply_prompt_marker(&mut self, frame: &str) -> bool {
        let Some(body) = frame
            .strip_prefix(MARKER_PREFIX)
            .and_then(|value| value.strip_suffix(MARKER_TERMINATOR))
        else {
            return false;
        };
        parse_editor_readiness_frame(body)
            .ok()
            .is_some_and(|readiness| self.apply_editor_readiness(&readiness))
    }

    fn apply_editing_sequence(&mut self, sequence: &str) -> bool {
        if !self.input_reliable {
            return false;
        }
        match sequence {
            "\u{1b}[D" => self.input_cursor = self.input_cursor.saturating_sub(1),
            "\u{1b}[C" => self.input_cursor = (self.input_cursor + 1).min(self.input_buffer.len()),
            "\u{1b}[H" | "\u{1b}[1~" => self.input_cursor = 0,
            "\u{1b}[F" | "\u{1b}[4~" => self.input_cursor = self.input_buffer.len(),
            "\u{1b}[3~" => {
                if self.input_cursor < self.input_buffer.len() {
                    let removed = self.input_buffer.remove(self.input_cursor);
                    self.input_buffer_bytes -= removed.len_utf8();
                }
            }
            _ => return false,
        }
        true
    }
}

fn push_forward(actions: &mut Vec<TerminalInputActionV1>, data: String) {
    if let Some(TerminalInputActionV1::Forward { data: previous }) = actions.last_mut() {
        previous.push_str(&data);
    } else {
        actions.push(TerminalInputActionV1::Forward { data });
    }
}

fn input_escape_complete(sequence: &str) -> bool {
    if sequence.len() < 2 {
        return false;
    }
    let bytes = sequence.as_bytes();
    match bytes[1] {
        b'[' => {
            sequence.len() >= 3
                && bytes
                    .last()
                    .is_some_and(|byte| (0x40..=0x7e).contains(byte))
        }
        b'O' => sequence.len() >= 3,
        b']' => sequence.ends_with('\u{7}') || sequence.ends_with("\u{1b}\\"),
        _ => true,
    }
}

fn logical_submission_count(data: &str) -> usize {
    let mut count = 0;
    let mut previous_was_carriage_return = false;
    for character in data.chars() {
        match character {
            '\r' => {
                count += 1;
                previous_was_carriage_return = true;
            }
            '\n' if previous_was_carriage_return => previous_was_carriage_return = false,
            '\n' => count += 1,
            _ => previous_was_carriage_return = false,
        }
    }
    count
}

fn text_follows_first_line_boundary(data: &str) -> bool {
    data.find(['\r', '\n'])
        .is_some_and(|index| !data[index..].trim_matches(['\r', '\n']).is_empty())
}

fn partial_prefix_suffix_len(value: &str) -> usize {
    let maximum = value.len().min(MARKER_PREFIX.len().saturating_sub(1));
    (1..=maximum)
        .rev()
        .find(|length| value.ends_with(&MARKER_PREFIX[..*length]))
        .unwrap_or(0)
}
