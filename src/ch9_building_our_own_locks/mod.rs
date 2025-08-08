//! # Building our own locks
//!
//! The target: mutex, condvar, rwlock. The tools mentioned in chapter 8 will be used.
//! As they vary OS to OS, we'll be relying to the most common fitex's wait and wake.
//! The only troublemaker here is OSX, but libc++ could be used there.
//!
//! We're going to cheat a bit by using the [atomic-wait](https://crates.io/crates/atomic-wait) crate
//! so not to deal with all the OS-specific stuff. There'are just 3 functions:
//! - `wait(&AtomicU32, u32)` - waits until woken up, blocks only if the atomic variable has the specified value, may wake up spuriously
//! - `wake_one(&AtomicU32)` - wakes a single thread that waits on the same atomic variable
//! - `wake(&AtomicU32)` - wakes all the threads waiting on the atomic variable

use std::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{
        AtomicU32,
        Ordering::{Acquire, Release},
    },
};

use atomic_wait::{wait, wake_all, wake_one};

/// # Mutex
/// It's starting at [`SpinLock<Y>`](crate::ch4_building_our_own_spin_lock::SpinLock) from the chapter 4.
///
/// There's u32 instead of boolean so it works with the wait & wake.
///
/// Note that wait and wake don't take any part in memory consistency or correctness of the Mutex.
/// They just spare us from wasting processor cycles.
///
/// lock_api crate is a good framework to make mutexes for other platforms, etc to avoid boilerplate.
///
pub struct Mutex<Y> {
    /// 0 - unlocked
    /// 1 - locked
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
