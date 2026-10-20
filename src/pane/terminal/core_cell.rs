use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LockResult, Mutex, MutexGuard, PoisonError, TryLockError, TryLockResult};

pub(crate) struct CoreCell<T> {
    inner: Mutex<T>,
    generation: AtomicU64,
}

pub(crate) struct CoreGuard<'a, T> {
    guard: MutexGuard<'a, T>,
    generation: &'a AtomicU64,
}

impl<T> CoreCell<T> {
    pub(crate) fn new(value: T) -> Self {
        Self {
            inner: Mutex::new(value),
            generation: AtomicU64::new(1),
        }
    }

    pub(crate) fn lock(&self) -> LockResult<CoreGuard<'_, T>> {
        match self.inner.lock() {
            Ok(guard) => Ok(self.wrap(guard)),
            Err(poisoned) => Err(PoisonError::new(self.wrap(poisoned.into_inner()))),
        }
    }

    pub(crate) fn try_lock(&self) -> TryLockResult<CoreGuard<'_, T>> {
        match self.inner.try_lock() {
            Ok(guard) => Ok(self.wrap(guard)),
            Err(TryLockError::Poisoned(poisoned)) => Err(TryLockError::Poisoned(
                PoisonError::new(self.wrap(poisoned.into_inner())),
            )),
            Err(TryLockError::WouldBlock) => Err(TryLockError::WouldBlock),
        }
    }

    pub(crate) fn lock_quiet(&self) -> LockResult<MutexGuard<'_, T>> {
        self.inner.lock()
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    fn wrap<'a>(&'a self, guard: MutexGuard<'a, T>) -> CoreGuard<'a, T> {
        CoreGuard {
            guard,
            generation: &self.generation,
        }
    }
}

impl<T> Deref for CoreGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<T> DerefMut for CoreGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.generation.fetch_add(1, Ordering::Release);
        &mut self.guard
    }
}
