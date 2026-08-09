use crate::text_stream::RecordFrameV1;
use crossbeam_channel::{bounded, Receiver, Sender, TryRecvError};
use parking_lot::Mutex;
use std::sync::Arc;
use std::thread;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PipelineFailureV1 {
    pub diagnostic: String,
}

impl PipelineFailureV1 {
    pub fn new(diagnostic: impl Into<String>) -> Self {
        Self {
            diagnostic: diagnostic.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StageOutcomeV1 {
    Success { exit_code: u8 },
    Result { exit_code: u8 },
    StoppedNormally,
    OperationalFailure { diagnostic: String },
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PipelineStatusV1 {
    pub exit_code: u8,
    pub diagnostic: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PipelineRunV1 {
    pub records: Vec<RecordFrameV1>,
    pub source_outcome: StageOutcomeV1,
    pub final_outcome: StageOutcomeV1,
    pub exit_code: u8,
    pub diagnostic: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipelineRunErrorV1 {
    InvalidChannelCapacity,
    WorkerPanicked,
    ProtocolFailure,
}

enum UpstreamDirective {
    Continue,
    Stop,
}

enum SourceMessage {
    Record {
        frame: RecordFrameV1,
        acknowledgement: Sender<UpstreamDirective>,
    },
    Failure(PipelineFailureV1),
}

struct CancellationInner {
    sender: Mutex<Option<Sender<()>>>,
    receiver: Receiver<()>,
}

#[derive(Clone)]
pub struct CancellationTokenV1 {
    inner: Arc<CancellationInner>,
}

impl Default for CancellationTokenV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl CancellationTokenV1 {
    pub fn new() -> Self {
        let (sender, receiver) = bounded(0);
        Self {
            inner: Arc::new(CancellationInner {
                sender: Mutex::new(Some(sender)),
                receiver,
            }),
        }
    }

    pub fn cancel(&self) {
        self.inner.sender.lock().take();
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(
            self.inner.receiver.try_recv(),
            Err(TryRecvError::Disconnected)
        )
    }

    fn receiver(&self) -> &Receiver<()> {
        &self.inner.receiver
    }
}

pub fn run_head_pipeline<I>(
    source: I,
    limit: usize,
    channel_capacity: usize,
) -> Result<PipelineRunV1, PipelineRunErrorV1>
where
    I: Iterator<Item = Result<RecordFrameV1, PipelineFailureV1>> + Send + 'static,
{
    run_head_pipeline_with_cancellation(source, limit, channel_capacity, CancellationTokenV1::new())
}

pub fn run_head_pipeline_with_cancellation<I>(
    source: I,
    limit: usize,
    channel_capacity: usize,
    cancellation: CancellationTokenV1,
) -> Result<PipelineRunV1, PipelineRunErrorV1>
where
    I: Iterator<Item = Result<RecordFrameV1, PipelineFailureV1>> + Send + 'static,
{
    if channel_capacity == 0 {
        return Err(PipelineRunErrorV1::InvalidChannelCapacity);
    }
    if cancellation.is_cancelled() {
        return Ok(cancelled_run());
    }
    if limit == 0 {
        return Ok(PipelineRunV1 {
            records: Vec::new(),
            source_outcome: StageOutcomeV1::StoppedNormally,
            final_outcome: StageOutcomeV1::Success { exit_code: 0 },
            exit_code: 0,
            diagnostic: None,
        });
    }

    let (sender, receiver) = bounded(channel_capacity);
    let source_cancellation = cancellation.clone();
    let source_worker = thread::spawn(move || run_source(source, sender, source_cancellation));
    let mut records = Vec::new();
    let mut final_outcome = StageOutcomeV1::Success { exit_code: 0 };
    let mut diagnostic = None;

    while records.len() < limit {
        let message = crossbeam_channel::select! {
            recv(cancellation.receiver()) -> _ => {
                final_outcome = StageOutcomeV1::Cancelled;
                break;
            }
            recv(receiver) -> message => message,
        };
        match message {
            Ok(SourceMessage::Record {
                frame,
                acknowledgement,
            }) => {
                records.push(frame);
                let directive = if records.len() == limit {
                    UpstreamDirective::Stop
                } else {
                    UpstreamDirective::Continue
                };
                let acknowledgement_result = crossbeam_channel::select! {
                    recv(cancellation.receiver()) -> _ => None,
                    send(acknowledgement, directive) -> result => Some(result),
                };
                match acknowledgement_result {
                    None => {
                        final_outcome = StageOutcomeV1::Cancelled;
                        break;
                    }
                    Some(Ok(())) => {}
                    Some(Err(_)) if cancellation.is_cancelled() => {
                        final_outcome = StageOutcomeV1::Cancelled;
                        break;
                    }
                    Some(Err(_)) => return Err(PipelineRunErrorV1::ProtocolFailure),
                }
                if records.len() == limit {
                    break;
                }
            }
            Ok(SourceMessage::Failure(failure)) => {
                diagnostic = Some(failure.diagnostic.clone());
                final_outcome = StageOutcomeV1::OperationalFailure {
                    diagnostic: failure.diagnostic,
                };
                break;
            }
            Err(_) if cancellation.is_cancelled() => {
                final_outcome = StageOutcomeV1::Cancelled;
                break;
            }
            Err(_) => break,
        }
    }

    let source_outcome = source_worker
        .join()
        .map_err(|_| PipelineRunErrorV1::WorkerPanicked)?;
    let mut status = resolve_pipeline_status(&source_outcome, &final_outcome);
    if status.diagnostic.is_none() {
        status.diagnostic = diagnostic;
    }

    Ok(PipelineRunV1 {
        records,
        source_outcome,
        final_outcome,
        exit_code: status.exit_code,
        diagnostic: status.diagnostic,
    })
}

pub fn resolve_pipeline_status(
    source_outcome: &StageOutcomeV1,
    final_outcome: &StageOutcomeV1,
) -> PipelineStatusV1 {
    if matches!(source_outcome, StageOutcomeV1::Cancelled)
        || matches!(final_outcome, StageOutcomeV1::Cancelled)
    {
        return PipelineStatusV1 {
            exit_code: 130,
            diagnostic: None,
        };
    }
    if let StageOutcomeV1::OperationalFailure { diagnostic } = source_outcome {
        return PipelineStatusV1 {
            exit_code: 1,
            diagnostic: Some(diagnostic.clone()),
        };
    }
    if let StageOutcomeV1::OperationalFailure { diagnostic } = final_outcome {
        return PipelineStatusV1 {
            exit_code: 1,
            diagnostic: Some(diagnostic.clone()),
        };
    }
    let exit_code = match final_outcome {
        StageOutcomeV1::Success { exit_code } | StageOutcomeV1::Result { exit_code } => *exit_code,
        StageOutcomeV1::StoppedNormally => 0,
        StageOutcomeV1::OperationalFailure { .. } | StageOutcomeV1::Cancelled => unreachable!(),
    };
    PipelineStatusV1 {
        exit_code,
        diagnostic: None,
    }
}

fn run_source<I>(
    mut source: I,
    sender: Sender<SourceMessage>,
    cancellation: CancellationTokenV1,
) -> StageOutcomeV1
where
    I: Iterator<Item = Result<RecordFrameV1, PipelineFailureV1>>,
{
    loop {
        if cancellation.is_cancelled() {
            return StageOutcomeV1::Cancelled;
        }
        match source.next() {
            None => return StageOutcomeV1::Success { exit_code: 0 },
            Some(Err(failure)) => {
                let outcome = StageOutcomeV1::OperationalFailure {
                    diagnostic: failure.diagnostic.clone(),
                };
                return crossbeam_channel::select! {
                    recv(cancellation.receiver()) -> _ => StageOutcomeV1::Cancelled,
                    send(sender, SourceMessage::Failure(failure)) -> result => {
                        if result.is_ok() {
                            outcome
                        } else if cancellation.is_cancelled() {
                            StageOutcomeV1::Cancelled
                        } else {
                            StageOutcomeV1::OperationalFailure {
                                diagnostic: "pipeline receiver closed unexpectedly".to_string(),
                            }
                        }
                    }
                };
            }
            Some(Ok(frame)) => {
                let (acknowledgement, response) = bounded(0);
                let send_result = crossbeam_channel::select! {
                    recv(cancellation.receiver()) -> _ => return StageOutcomeV1::Cancelled,
                    send(sender, SourceMessage::Record {
                        frame,
                        acknowledgement,
                    }) -> result => result,
                };
                if send_result.is_err() && cancellation.is_cancelled() {
                    return StageOutcomeV1::Cancelled;
                }
                if send_result.is_err() {
                    return StageOutcomeV1::OperationalFailure {
                        diagnostic: "pipeline receiver closed unexpectedly".to_string(),
                    };
                }
                let directive = crossbeam_channel::select! {
                    recv(cancellation.receiver()) -> _ => return StageOutcomeV1::Cancelled,
                    recv(response) -> directive => directive,
                };
                match directive {
                    Ok(UpstreamDirective::Continue) => {}
                    Ok(UpstreamDirective::Stop) => return StageOutcomeV1::StoppedNormally,
                    Err(_) if cancellation.is_cancelled() => {
                        return StageOutcomeV1::Cancelled;
                    }
                    Err(_) => {
                        return StageOutcomeV1::OperationalFailure {
                            diagnostic: "pipeline acknowledgement channel closed".to_string(),
                        };
                    }
                }
            }
        }
    }
}

fn cancelled_run() -> PipelineRunV1 {
    PipelineRunV1 {
        records: Vec::new(),
        source_outcome: StageOutcomeV1::Cancelled,
        final_outcome: StageOutcomeV1::Cancelled,
        exit_code: 130,
        diagnostic: None,
    }
}
