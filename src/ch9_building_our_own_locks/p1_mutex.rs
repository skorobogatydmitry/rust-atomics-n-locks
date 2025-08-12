//! # Mutex
//! It's starting at [`SpinLock<Y>`](crate::ch4_building_our_own_spin_lock::SpinLock) from the chapter 4.
//!
//! There's u32 instead of boolean so it works with the wait & wake.
//!
//! Note that wait and wake don't take any part in memory consistency or correctness of the Mutex.
//! They just spare us from wasting processor cycles.
//!
//! lock_api crate is a good framework to make mutexes for other platforms, etc to avoid boilerplate.
//!
//! ## Avoiding syscalls
//! The wait and wake syscalls are slow (as all syscalls) => implementation should avoid them if possible.
//!
//! Initial implementation doesn't call `wait` unless needed, but it does unconditionally calls `wake_one`.
//! The `wake_one` can be skipped if we know there are no other threads waiting. The new part is value `2` of the state and
//! semantic around it.
//!
//! See the [Mutex::lock2] and [MutexGuard::drop2] function for further changes.
//!
//! The important part is that if there're no races for the lock, both syscalls aren't called.
//!
//! ## Optimizing Further
//!
//! The spinlock version can be more efficient comparing to syscalls if the time in the waiting loop is short.
//! It's actually a common use-case for Mutex-es.
//!
//! Let's combine a short-time spinlock with waiting. See [Mutex::lock3].

use std::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{
        AtomicU32,
        Ordering::{Acquire, Relaxed, Release},
    },
};

use atomic_wait::{wait, wake_one};

pub struct Mutex<Y> {
    /// 0 - unlocked
    /// 1 - locked, no other threads
    /// 2 - locked, other threads are waiting
    state: AtomicU32,
    value: UnsafeCell<Y>,
}

/// promise to the compiler that it's safe to share if the underlying value is safe to send
unsafe impl<Y> Sync for Mutex<Y> where Y: Send {}

impl<Y> Mutex<Y> {
    /// Just the usual boring part
    pub fn new(value: Y) -> Self {
        Self {
            state: AtomicU32::new(0), // it's unocked from the start
            value: UnsafeCell::new(value),
        }
    }

    /// It's an almost identical to the ch4's implementation
    /// but 1 extra wait to save some cycles
    pub fn lock(&self) -> MutexGuard<'_, Y> {
        // while locked
        while self.state.swap(1, Acquire) == 1 {
            // wait on the state == 1
            wait(&self.state, 1);
        }
        MutexGuard { mutex: self }
    }

    // it's an advanced version with 0,1,2 values for the state.
    pub fn lock2(&self) -> MutexGuard<'_, Y> {
        // try to lock the mutex
        if self.state.compare_exchange(0, 1, Acquire, Relaxed).is_err() {
            // if the locking failed, the mutex is in 1 or 2 state =>
            // spin until we see an unlocked mutex and store 2 in its state.
            // Note that this lock leaves the state as 2 to not lose other potential waiters.
            while self.state.swap(2, Acquire) != 0 {
                wait(&self.state, 2);
            }
        }
        MutexGuard { mutex: self }
    }

    pub fn lock3(&self) -> MutexGuard<'_, Y> {
        if self.state.compare_exchange(0, 1, Acquire, Relaxed).is_err() {
            // The lock was already locked T_T
            // => do the logic to wait
            Self::lock_contented(&self.state);
        }
        MutexGuard { mutex: self }
    }

    /// The function containes optimized logic for waiting for the Mutex to unlock:
    /// - spinlock for some cycles
    /// - engage wait syscall if still locked
    #[cold] // means that the function is a fallback of the algo
    fn lock_contented(state: &AtomicU32) {
        let mut spin_count = 0;

        // spinlock for a hundred cycles
        // use load here, as compare_and_exchange has impact on cache perf
        // only check for 1, as 2 means that the other thread already gave up here
        while state.load(Relaxed) == 1 && spin_count < 100 {
            spin_count += 1;
            std::hint::spin_loop();
        }

        // try to lock the Mutex
        if state.compare_exchange(0, 1, Acquire, Relaxed).is_ok() {
            // we've successfully acquired the lock
            return;
        }

        // the last resort - wait syscall
        while state.swap(2, Acquire) != 0 {
            wait(state, 2);
        }
    }
}

/// This drop unlocks the mutex. it's the only way to do that.  
/// There're no guarantees that the lock will be obtained by the thread we wake up.
/// Any other thread may be faster.
impl<Y> Drop for MutexGuard<'_, Y> {
    fn drop(&mut self) {
        // change the state first
        self.mutex.state.store(0, Release);
        // wake up exactly 1 waiting thread, what's just enough to proceed
        wake_one(&self.mutex.state);
    }
}

/// It should be impl Drop, but there could only be 1
impl<Y> MutexGuard<'_, Y> {
    /// It's an advanced drop implementation that skips unnecessary wakes
    #[allow(unused)]
    fn drop2(&mut self) {
        // wake the thread only if someone switched the state to 2
        if self.mutex.state.swap(0, Release) == 2 {
            wake_one(&self.mutex.state);
        }
    }
}

/// Guard structure to ease ownership and usage
pub struct MutexGuard<'a, Y> {
    mutex: &'a Mutex<Y>,
}

/// The guard itself is thread-safe to be fancy, IIUC.  
/// But this means that the underlying data should be Send / Sync
unsafe impl<Y> Send for MutexGuard<'_, Y> where Y: Send {}
unsafe impl<Y> Sync for MutexGuard<'_, Y> where Y: Sync {}

impl<Y> Deref for MutexGuard<'_, Y> {
    type Target = Y;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.value.get() }
    }
}

impl<Y> DerefMut for MutexGuard<'_, Y> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mutex.value.get() }
    }
}
