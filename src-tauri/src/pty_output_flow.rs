use parking_lot::{Condvar, Mutex};

#[derive(Default)]
struct OutputFlowStateV1 {
    closed: bool,
    next_sequence: u64,
    awaiting: Option<u64>,
}

pub struct PtyOutputFlowV1 {
    state: Mutex<OutputFlowStateV1>,
    changed: Condvar,
}

impl PtyOutputFlowV1 {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(OutputFlowStateV1 {
                next_sequence: 1,
                ..OutputFlowStateV1::default()
            }),
            changed: Condvar::new(),
        }
    }

    pub fn deliver(&self, sequence: u64, publish: impl FnOnce() -> bool) -> bool {
        {
            let mut state = self.state.lock();
            if state.closed || state.awaiting.is_some() || sequence != state.next_sequence {
                return false;
            }
            state.awaiting = Some(sequence);
        }

        if !publish() {
            self.close();
            return false;
        }

        let mut state = self.state.lock();
        while !state.closed && state.awaiting == Some(sequence) {
            self.changed.wait(&mut state);
        }
        !state.closed
    }

    pub fn acknowledge(&self, sequence: u64) -> bool {
        let mut state = self.state.lock();
        if state.closed || state.awaiting != Some(sequence) {
            return false;
        }
        state.awaiting = None;
        state.next_sequence = state.next_sequence.saturating_add(1);
        self.changed.notify_all();
        true
    }

    pub fn close(&self) {
        let mut state = self.state.lock();
        if state.closed {
            return;
        }
        state.closed = true;
        state.awaiting = None;
        self.changed.notify_all();
    }
}
