use crate::grep_pattern::GrepPatternV1;
use crate::interpreter::{ExecutionPlanV1, StagePlanV1};
use crate::runner_cancel::RunnerCancellationV1;
use crate::runner_readonly::{MAX_TAIL_BUFFER_BYTES, MAX_TAIL_BUFFER_RECORDS};
use crate::sort_support::{
    compare_exact_decimal, parse_exact_decimal, resource_limit_exceeded as sort_limit_exceeded,
};
use crate::text_stream::{RecordFrameV1, RecordStreamWriterV1, TextStreamWriteErrorV1};
use crate::windows_path::ValidatedPathSpecV1;
use std::cmp::Ordering;
use std::collections::{HashSet, VecDeque};
use std::io::{self, Write};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OrderedPipelineFaultV1 {
    Unsupported,
    TailResource,
    SortResource,
    InvalidNumeric,
    Overflow,
    Cancelled,
    Output { kind: io::ErrorKind },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OrderedFlowV1 {
    Continue,
    StopUpstream,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OrderedFinishCauseV1 {
    Complete,
    UpstreamStopped,
    SourceFailed,
}

#[derive(Clone, Debug)]
struct PipelineRecordV1 {
    frame: RecordFrameV1,
    input_index: usize,
}

enum StageActionV1 {
    Drop,
    Emit(PipelineRecordV1),
    EmitAndStop(PipelineRecordV1),
    Stop,
}

struct StageSlotV1 {
    stage: RuntimeStageV1,
    pending_unterminated: Option<PipelineRecordV1>,
}

enum RuntimeStageV1 {
    Search(SearchStateV1),
    Head { remaining: usize },
    Tail(TailStateV1),
    Count { terminated: u64 },
    Sort(SortStateV1),
    Unique(UniqueStateV1),
}

struct SearchStateV1 {
    pattern: GrepPatternV1,
    line_numbers: bool,
    invert_match: bool,
    direct: bool,
    multiple_paths: bool,
    line_number: u64,
    matched_any: bool,
}

struct TailStateV1 {
    limit: usize,
    records: VecDeque<PipelineRecordV1>,
    bytes: usize,
}

struct SortStateV1 {
    reverse: bool,
    numeric: bool,
    unique: bool,
    records: Vec<PipelineRecordV1>,
    bytes: usize,
}

struct UniqueStateV1 {
    count: bool,
    repeated_only: bool,
    unique_only: bool,
    pending: Option<(PipelineRecordV1, u64)>,
}

const SORT_RUN_RECORDS: usize = 2_048;
const CANCELLATION_CHECK_INTERVAL: usize = 1_024;

pub(crate) struct OrderedPipelineV1<'a, W: Write> {
    stages: Vec<StageSlotV1>,
    sink: &'a mut RecordStreamWriterV1<W>,
    cancellation: &'a RunnerCancellationV1,
    source_paths: &'a [&'a ValidatedPathSpecV1],
}

impl<'a, W: Write> OrderedPipelineV1<'a, W> {
    pub(crate) fn new(
        plan: &ExecutionPlanV1,
        sink: &'a mut RecordStreamWriterV1<W>,
        cancellation: &'a RunnerCancellationV1,
        source_paths: &'a [&'a ValidatedPathSpecV1],
    ) -> Result<Self, OrderedPipelineFaultV1> {
        let direct_search = matches!(
            plan.stages.first(),
            Some(StagePlanV1::SearchText { paths, .. }) if !paths.is_empty()
        );
        let mut stages = Vec::new();
        for (index, stage) in plan.stages.iter().enumerate() {
            if matches!(
                stage,
                StagePlanV1::ReadTextFiles { .. }
                    | StagePlanV1::ListEntries { .. }
                    | StagePlanV1::FindPaths { .. }
            ) {
                if index != 0 {
                    return Err(OrderedPipelineFaultV1::Unsupported);
                }
                continue;
            }
            stages.push(StageSlotV1 {
                stage: RuntimeStageV1::from_plan(
                    stage,
                    direct_search && index == 0,
                    source_paths.len() > 1,
                )?,
                pending_unterminated: None,
            });
        }
        Ok(Self {
            stages,
            sink,
            cancellation,
            source_paths,
        })
    }

    pub(crate) fn starts_stopped(&self) -> bool {
        self.stages.iter().any(|slot| {
            matches!(slot.stage, RuntimeStageV1::Head { remaining: 0 })
                || matches!(
                    slot.stage,
                    RuntimeStageV1::Tail(TailStateV1 { limit: 0, .. })
                )
        })
    }

    pub(crate) fn start_input(&mut self) {
        for slot in &mut self.stages {
            if let RuntimeStageV1::Search(search) = &mut slot.stage {
                if search.direct {
                    search.line_number = 1;
                }
            }
        }
    }

    pub(crate) fn push(
        &mut self,
        frame: RecordFrameV1,
        input_index: usize,
    ) -> Result<OrderedFlowV1, OrderedPipelineFaultV1> {
        self.route_from(0, PipelineRecordV1 { frame, input_index })
    }

    pub(crate) fn finish(
        &mut self,
        cause: OrderedFinishCauseV1,
    ) -> Result<(), OrderedPipelineFaultV1> {
        for index in 0..self.stages.len() {
            if self.cancellation.is_cancelled() {
                self.abort();
                return Err(OrderedPipelineFaultV1::Cancelled);
            }
            let output = self.stages[index].stage.finish(cause, self.cancellation)?;
            for record in output {
                if self.emit_from_slot(index, record)? == OrderedFlowV1::StopUpstream {
                    break;
                }
            }
            if let Some(record) = self.stages[index].pending_unterminated.take() {
                let _ = self.route_from(index + 1, record)?;
            }
        }
        Ok(())
    }

    pub(crate) fn final_search_matched(&self) -> Option<bool> {
        match self.stages.last().map(|slot| &slot.stage) {
            Some(RuntimeStageV1::Search(search)) => Some(search.matched_any),
            _ => None,
        }
    }

    fn route_from(
        &mut self,
        start_index: usize,
        record: PipelineRecordV1,
    ) -> Result<OrderedFlowV1, OrderedPipelineFaultV1> {
        let mut queue = VecDeque::from([(start_index, record)]);
        let mut stop = false;
        while let Some((index, record)) = queue.pop_front() {
            if self.cancellation.is_cancelled() {
                self.abort();
                return Err(OrderedPipelineFaultV1::Cancelled);
            }
            if index == self.stages.len() {
                self.sink.push(record.frame).map_err(map_sink_fault)?;
                continue;
            }
            let action = self.stages[index].stage.push(record, self.source_paths)?;
            match action {
                StageActionV1::Drop => {}
                StageActionV1::Emit(output) => {
                    self.enqueue_output(index, output, &mut queue);
                }
                StageActionV1::EmitAndStop(output) => {
                    self.enqueue_output(index, output, &mut queue);
                    stop = true;
                }
                StageActionV1::Stop => stop = true,
            }
        }
        Ok(if stop {
            OrderedFlowV1::StopUpstream
        } else {
            OrderedFlowV1::Continue
        })
    }

    fn emit_from_slot(
        &mut self,
        index: usize,
        record: PipelineRecordV1,
    ) -> Result<OrderedFlowV1, OrderedPipelineFaultV1> {
        let mut queue = VecDeque::new();
        self.enqueue_output(index, record, &mut queue);
        let mut stop = false;
        while let Some((next, record)) = queue.pop_front() {
            stop |= self.route_from(next, record)? == OrderedFlowV1::StopUpstream;
        }
        Ok(if stop {
            OrderedFlowV1::StopUpstream
        } else {
            OrderedFlowV1::Continue
        })
    }

    fn enqueue_output(
        &mut self,
        index: usize,
        record: PipelineRecordV1,
        queue: &mut VecDeque<(usize, PipelineRecordV1)>,
    ) {
        if let Some(mut pending) = self.stages[index].pending_unterminated.take() {
            pending.frame.terminated = true;
            queue.push_back((index + 1, pending));
        }
        if record.frame.terminated {
            queue.push_back((index + 1, record));
        } else {
            self.stages[index].pending_unterminated = Some(record);
        }
    }

    fn abort(&mut self) {
        for slot in &mut self.stages {
            slot.pending_unterminated = None;
            slot.stage.abort();
        }
    }
}

impl RuntimeStageV1 {
    fn from_plan(
        stage: &StagePlanV1,
        direct_search: bool,
        multiple_paths: bool,
    ) -> Result<Self, OrderedPipelineFaultV1> {
        match stage {
            StagePlanV1::SearchText {
                pattern,
                ignore_case,
                line_numbers,
                invert_match,
                fixed_strings,
                recursive,
                ..
            } if !*recursive => Ok(Self::Search(SearchStateV1 {
                pattern: GrepPatternV1::compile(pattern, *fixed_strings, *ignore_case)
                    .map_err(|_| OrderedPipelineFaultV1::Unsupported)?,
                line_numbers: *line_numbers,
                invert_match: *invert_match,
                direct: direct_search,
                multiple_paths,
                line_number: 1,
                matched_any: false,
            })),
            StagePlanV1::HeadLines { count, .. } => Ok(Self::Head { remaining: *count }),
            StagePlanV1::TailLines { count, .. } => Ok(Self::Tail(TailStateV1 {
                limit: *count,
                records: VecDeque::new(),
                bytes: 0,
            })),
            StagePlanV1::CountLines { .. } => Ok(Self::Count { terminated: 0 }),
            StagePlanV1::SortLines {
                reverse,
                numeric,
                unique,
                ..
            } => Ok(Self::Sort(SortStateV1 {
                reverse: *reverse,
                numeric: *numeric,
                unique: *unique,
                records: Vec::new(),
                bytes: 0,
            })),
            StagePlanV1::UniqueLines {
                count,
                repeated_only,
                unique_only,
                ..
            } => Ok(Self::Unique(UniqueStateV1 {
                count: *count,
                repeated_only: *repeated_only,
                unique_only: *unique_only,
                pending: None,
            })),
            _ => Err(OrderedPipelineFaultV1::Unsupported),
        }
    }

    fn push(
        &mut self,
        mut record: PipelineRecordV1,
        source_paths: &[&ValidatedPathSpecV1],
    ) -> Result<StageActionV1, OrderedPipelineFaultV1> {
        match self {
            Self::Search(search) => {
                let current_line = search.line_number;
                search.line_number = search
                    .line_number
                    .checked_add(1)
                    .ok_or(OrderedPipelineFaultV1::Overflow)?;
                let selected = search.pattern.is_match(&record.frame.text) != search.invert_match;
                if !selected {
                    return Ok(StageActionV1::Drop);
                }
                search.matched_any = true;
                if search.direct && search.multiple_paths {
                    let path = source_paths
                        .get(record.input_index)
                        .ok_or(OrderedPipelineFaultV1::Unsupported)?
                        .original
                        .replace('/', "\\");
                    record.frame.text = if search.line_numbers {
                        format!("{path}:{current_line}:{}", record.frame.text)
                    } else {
                        format!("{path}:{}", record.frame.text)
                    };
                } else if search.line_numbers {
                    record.frame.text = format!("{current_line}:{}", record.frame.text);
                }
                Ok(StageActionV1::Emit(record))
            }
            Self::Head { remaining } => {
                if *remaining == 0 {
                    return Ok(StageActionV1::Stop);
                }
                *remaining -= 1;
                Ok(if *remaining == 0 {
                    StageActionV1::EmitAndStop(record)
                } else {
                    StageActionV1::Emit(record)
                })
            }
            Self::Tail(tail) => {
                if tail.limit == 0 {
                    return Ok(StageActionV1::Stop);
                }
                while tail.records.len() >= tail.limit {
                    if let Some(discarded) = tail.records.pop_front() {
                        tail.bytes = tail.bytes.saturating_sub(discarded.frame.text.len());
                    }
                }
                let next_bytes = tail.bytes.saturating_add(record.frame.text.len());
                if tail.records.len() >= MAX_TAIL_BUFFER_RECORDS
                    || next_bytes > MAX_TAIL_BUFFER_BYTES
                {
                    tail.records.clear();
                    tail.bytes = 0;
                    return Err(OrderedPipelineFaultV1::TailResource);
                }
                tail.records.push_back(record);
                tail.bytes = next_bytes;
                Ok(StageActionV1::Drop)
            }
            Self::Count { terminated } => {
                if record.frame.terminated {
                    *terminated = terminated
                        .checked_add(1)
                        .ok_or(OrderedPipelineFaultV1::Overflow)?;
                }
                Ok(StageActionV1::Drop)
            }
            Self::Sort(sort) => {
                if sort_limit_exceeded(sort.records.len(), sort.bytes, record.frame.text.len()) {
                    sort.records.clear();
                    sort.bytes = 0;
                    return Err(OrderedPipelineFaultV1::SortResource);
                }
                let next_bytes = sort.bytes.saturating_add(record.frame.text.len());
                sort.records.push(record);
                sort.bytes = next_bytes;
                Ok(StageActionV1::Drop)
            }
            Self::Unique(unique) => {
                let Some((mut pending, mut occurrences)) = unique.pending.take() else {
                    unique.pending = Some((record, 1));
                    return Ok(StageActionV1::Drop);
                };
                if pending.frame.text == record.frame.text {
                    occurrences = occurrences
                        .checked_add(1)
                        .ok_or(OrderedPipelineFaultV1::Overflow)?;
                    pending.frame.terminated = record.frame.terminated;
                    unique.pending = Some((pending, occurrences));
                    return Ok(StageActionV1::Drop);
                }
                pending.frame.terminated = true;
                let output = unique.prepare_group(pending, occurrences);
                unique.pending = Some((record, 1));
                Ok(match output {
                    Some(output) => StageActionV1::Emit(output),
                    None => StageActionV1::Drop,
                })
            }
        }
    }

    fn finish(
        &mut self,
        cause: OrderedFinishCauseV1,
        cancellation: &RunnerCancellationV1,
    ) -> Result<VecDeque<PipelineRecordV1>, OrderedPipelineFaultV1> {
        match self {
            Self::Search(_) | Self::Head { .. } => Ok(VecDeque::new()),
            Self::Tail(tail) => Ok(std::mem::take(&mut tail.records)),
            Self::Count { terminated } => Ok(VecDeque::from([PipelineRecordV1 {
                frame: RecordFrameV1 {
                    text: terminated.to_string(),
                    terminated: true,
                },
                input_index: 0,
            }])),
            Self::Sort(sort) => {
                if cause == OrderedFinishCauseV1::SourceFailed {
                    sort.records.clear();
                    return Ok(VecDeque::new());
                }
                let records = sort.finish(cancellation)?;
                Ok(records.into())
            }
            Self::Unique(unique) => {
                let output = unique
                    .pending
                    .take()
                    .and_then(|(record, occurrences)| unique.prepare_group(record, occurrences));
                Ok(output.into_iter().collect())
            }
        }
    }

    fn abort(&mut self) {
        match self {
            Self::Tail(tail) => {
                tail.records.clear();
                tail.bytes = 0;
            }
            Self::Sort(sort) => {
                sort.records.clear();
                sort.bytes = 0;
            }
            Self::Unique(unique) => unique.pending = None,
            Self::Search(_) | Self::Head { .. } | Self::Count { .. } => {}
        }
    }
}

impl SortStateV1 {
    fn finish(
        &mut self,
        cancellation: &RunnerCancellationV1,
    ) -> Result<Vec<PipelineRecordV1>, OrderedPipelineFaultV1> {
        let mut records = std::mem::take(&mut self.records);
        self.bytes = 0;
        if records.is_empty() {
            return Ok(records);
        }
        let final_terminated = records.last().is_some_and(|record| record.frame.terminated);
        if self.numeric {
            let mut keyed = Vec::with_capacity(records.len());
            for record in records {
                if cancellation.is_cancelled() {
                    return Err(OrderedPipelineFaultV1::Cancelled);
                }
                let key = parse_exact_decimal(&record.frame.text)
                    .ok_or(OrderedPipelineFaultV1::InvalidNumeric)?;
                keyed.push((record, key));
            }
            if self.unique {
                keyed = retain_first_identical_cancellable(
                    keyed,
                    |entry| entry.0.frame.text.as_str(),
                    || cancellation.is_cancelled(),
                )?;
            }
            let reverse = self.reverse;
            keyed = stable_sort_cancellable(
                keyed,
                |left, right| {
                    let ordering = compare_exact_decimal(&left.1, &right.1);
                    if reverse {
                        ordering.reverse()
                    } else {
                        ordering
                    }
                },
                || cancellation.is_cancelled(),
            )?;
            records = keyed.into_iter().map(|(record, _)| record).collect();
        } else {
            if self.unique {
                records = retain_first_identical_cancellable(
                    records,
                    |record| record.frame.text.as_str(),
                    || cancellation.is_cancelled(),
                )?;
            }
            let reverse = self.reverse;
            records = stable_sort_cancellable(
                records,
                |left, right| {
                    let ordering = left.frame.text.cmp(&right.frame.text);
                    if reverse {
                        ordering.reverse()
                    } else {
                        ordering
                    }
                },
                || cancellation.is_cancelled(),
            )?;
        }
        let last = records.len().saturating_sub(1);
        for (index, record) in records.iter_mut().enumerate() {
            record.frame.terminated = index != last || final_terminated;
        }
        Ok(records)
    }
}

fn retain_first_identical_cancellable<T>(
    records: Vec<T>,
    text: impl Fn(&T) -> &str,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Vec<T>, OrderedPipelineFaultV1> {
    let mut seen = HashSet::with_capacity(records.len());
    let mut retained = Vec::with_capacity(records.len());
    for (index, record) in records.iter().enumerate() {
        if index % CANCELLATION_CHECK_INTERVAL == 0 && cancelled() {
            return Err(OrderedPipelineFaultV1::Cancelled);
        }
        retained.push(seen.insert(text(record)));
    }
    drop(seen);
    Ok(records
        .into_iter()
        .zip(retained)
        .filter_map(|(record, keep)| keep.then_some(record))
        .collect())
}

fn stable_sort_cancellable<T>(
    records: Vec<T>,
    mut compare: impl FnMut(&T, &T) -> Ordering,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Vec<T>, OrderedPipelineFaultV1> {
    if cancelled() {
        return Err(OrderedPipelineFaultV1::Cancelled);
    }
    let mut input = records.into_iter();
    let mut runs = VecDeque::new();
    loop {
        let mut run = Vec::with_capacity(SORT_RUN_RECORDS);
        for _ in 0..SORT_RUN_RECORDS {
            let Some(record) = input.next() else {
                break;
            };
            run.push(record);
        }
        if run.is_empty() {
            break;
        }
        run.sort_by(|left, right| compare(left, right));
        if cancelled() {
            return Err(OrderedPipelineFaultV1::Cancelled);
        }
        runs.push_back(run);
    }

    while runs.len() > 1 {
        let mut merged_runs = VecDeque::with_capacity(runs.len().div_ceil(2));
        while let Some(left) = runs.pop_front() {
            let Some(right) = runs.pop_front() else {
                merged_runs.push_back(left);
                break;
            };
            let mut left = left.into_iter().peekable();
            let mut right = right.into_iter().peekable();
            let mut merged = Vec::with_capacity(left.len().saturating_add(right.len()));
            while left.peek().is_some() || right.peek().is_some() {
                let take_left = match (left.peek(), right.peek()) {
                    (Some(left), Some(right)) => compare(left, right) != Ordering::Greater,
                    (Some(_), None) => true,
                    (None, Some(_)) => false,
                    (None, None) => break,
                };
                merged.push(if take_left {
                    left.next().expect("peeked left sort record")
                } else {
                    right.next().expect("peeked right sort record")
                });
                if merged.len() % CANCELLATION_CHECK_INTERVAL == 0 && cancelled() {
                    return Err(OrderedPipelineFaultV1::Cancelled);
                }
            }
            merged_runs.push_back(merged);
        }
        if cancelled() {
            return Err(OrderedPipelineFaultV1::Cancelled);
        }
        runs = merged_runs;
    }

    Ok(runs.pop_front().unwrap_or_default())
}

impl UniqueStateV1 {
    fn prepare_group(
        &self,
        mut record: PipelineRecordV1,
        occurrences: u64,
    ) -> Option<PipelineRecordV1> {
        if (self.repeated_only && occurrences < 2) || (self.unique_only && occurrences != 1) {
            return None;
        }
        if self.count {
            record.frame.text = format!("{occurrences} {}", record.frame.text);
        }
        Some(record)
    }
}

fn map_sink_fault(error: TextStreamWriteErrorV1) -> OrderedPipelineFaultV1 {
    match error {
        TextStreamWriteErrorV1::Encode(_) => OrderedPipelineFaultV1::Output {
            kind: io::ErrorKind::InvalidData,
        },
        TextStreamWriteErrorV1::Io { kind } => OrderedPipelineFaultV1::Output { kind },
    }
}

#[cfg(test)]
mod tests {
    use super::{retain_first_identical_cancellable, stable_sort_cancellable};
    use crate::ordered_pipeline::OrderedPipelineFaultV1;
    use std::cell::Cell;

    #[test]
    fn stable_sort_observes_cancellation_between_bounded_runs() {
        let checks = Cell::new(0usize);
        let result = stable_sort_cancellable(
            (0..5_000).rev().collect::<Vec<_>>(),
            |left, right| left.cmp(right),
            || {
                let next = checks.get() + 1;
                checks.set(next);
                next >= 3
            },
        );

        assert_eq!(result, Err(OrderedPipelineFaultV1::Cancelled));
        assert!(checks.get() >= 3);
    }

    #[test]
    fn identical_retention_observes_cancellation_during_the_scan() {
        let checks = Cell::new(0usize);
        let result = retain_first_identical_cancellable(
            (0..5_000).map(|value| value.to_string()).collect(),
            |value| value.as_str(),
            || {
                let next = checks.get() + 1;
                checks.set(next);
                next >= 3
            },
        );

        assert_eq!(result, Err(OrderedPipelineFaultV1::Cancelled));
        assert!(checks.get() >= 3);
    }
}
