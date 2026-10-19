use std::collections::VecDeque;
use std::ops::Deref;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::HeldSend;

static QUEUED_SENDS: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn has_queued_sends() -> bool {
    QUEUED_SENDS.load(Ordering::SeqCst) != 0
}

#[derive(Default)]
pub(super) struct PoliteQueue(VecDeque<HeldSend>);

impl PoliteQueue {
    pub(super) fn push_back(&mut self, item: HeldSend) {
        QUEUED_SENDS.fetch_add(1, Ordering::SeqCst);
        self.0.push_back(item);
    }

    pub(super) fn pop_front(&mut self) -> Option<HeldSend> {
        let item = self.0.pop_front();
        if item.is_some() {
            QUEUED_SENDS.fetch_sub(1, Ordering::SeqCst);
        }
        item
    }

    pub(super) fn remove(&mut self, index: usize) -> Option<HeldSend> {
        let item = self.0.remove(index);
        if item.is_some() {
            QUEUED_SENDS.fetch_sub(1, Ordering::SeqCst);
        }
        item
    }

    pub(super) fn retain(&mut self, mut keep: impl FnMut(&HeldSend) -> bool) {
        self.0.retain(|item| {
            let retained = keep(item);
            if !retained {
                QUEUED_SENDS.fetch_sub(1, Ordering::SeqCst);
            }
            retained
        });
    }
}

impl Deref for PoliteQueue {
    type Target = VecDeque<HeldSend>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for PoliteQueue {
    fn drop(&mut self) {
        QUEUED_SENDS.fetch_sub(self.0.len(), Ordering::SeqCst);
    }
}
