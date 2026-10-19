use std::collections::VecDeque;
use std::ops::Deref;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::HeldSend;

static QUEUED_SENDS: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn has_queued_sends() -> bool {
    QUEUED_SENDS.load(Ordering::SeqCst) != 0
}

fn subtract_queued_sends(count: usize) {
    let _ = QUEUED_SENDS.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |queued| {
        Some(queued.saturating_sub(count))
    });
}

struct RetainAccounting<'a> {
    queue: &'a mut VecDeque<HeldSend>,
    before: usize,
}

impl Drop for RetainAccounting<'_> {
    fn drop(&mut self) {
        subtract_queued_sends(self.before.saturating_sub(self.queue.len()));
    }
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
            subtract_queued_sends(1);
        }
        item
    }

    pub(super) fn remove(&mut self, index: usize) -> Option<HeldSend> {
        let item = self.0.remove(index);
        if item.is_some() {
            subtract_queued_sends(1);
        }
        item
    }

    pub(super) fn retain(&mut self, keep: impl FnMut(&HeldSend) -> bool) {
        let before = self.0.len();
        let accounting = RetainAccounting {
            queue: &mut self.0,
            before,
        };
        accounting.queue.retain(keep);
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
        subtract_queued_sends(self.0.len());
    }
}
