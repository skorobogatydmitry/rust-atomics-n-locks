//! # Read-write lock
//! It works similarly to the [super::p1_mutex::Mutex2], but allows 2 types of flocking
//! - exclusive / write - completely idential to the Mutex
//! - shared / read - there can be multiple, but they doesn't allow modifications
//!
//! Plus, there could be only one type of locks at a given moment of time.
//!
//! Internally, in addition to the Mutex-es, the RWLock has to track how many readers are there to control locking.
//!
//! All the memory ordering below is for the external users: Release on unlocking and Acquire on locking.
//!

use std::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{
        AtomicU32,
        Ordering::{Acquire, Relaxed, Release},
    },
};

use atomic_wait::{wait, wake_all, wake_one};
use libc::statfs;

/// Use 1 state to track everything with special values
pub struct RWLock<Y> {
    state: AtomicU32, // 0 - not locked, N - number of reqders, U32::MAX - write lock
    value: UnsafeCell<Y>,
}

/// as the RWLock may have multiple references to Y accessed from different threads,
/// Y has to be not only Send, but also Sync
unsafe impl<Y> Sync for RWLock<Y> where Y: Send + Sync {}

/// Note the 2 separated methods and structs for different kinds of locks
impl<Y> RWLock<Y> {
    pub const fn new(value: Y) -> Self {
        Self {
            state: AtomicU32::new(0),
            value: UnsafeCell::new(value),
        }
    }

    /// lock rwlock for reads, probably not for the 1st time
    pub fn read(&self) -> ReadGuard<Y> {
        let mut s = self.state.load(Relaxed);
        loop {
            // there's no write lock
            if s < u32::MAX {
                // be safe - don't allow MAX-1 read locks to become 1 write lock
                assert!(s < u32::MAX - 2, "too many readers!");
                // TODO: why weak?
                match self.state.compare_exchange_weak(s, s + 1, Acquire, Relaxed) {
                    Ok(_) => return ReadGuard { lock: self }, // nothing changed since we've read it last time
                    Err(e) => s = e, // somebody locked the mutex since we've read it - try again
                }
            }
            if s == u32::MAX {
                // there's an exclusive lock - wait for it to disappear
                wait(&self.state, u32::MAX);
                s = self.state.load(Relaxed); // get the new value - it should definitely change to 0 or some higher number
            }
        }
    }

    /// lock the rwlock for writes
    pub fn write(&self) -> WriteGuard<Y> {
        // TODO: why not weak?
        while let Err(s) = self.state.compare_exchange(0, u32::MAX, Acquire, Relaxed) {
            wait(&self.state, s); // hang on the s to unlock / disappear
        }
        WriteGuard { lock: self }
    }
}

struct ReadGuard<'a, Y> {
    lock: &'a RWLock<Y>,
}

/// Read guard's main purpose - provide &Y
impl<Y> Deref for ReadGuard<'_, Y> {
    type Target = Y;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.value.get() }
    }
}

/// unlocking the read guard is simple - it needs to just decrement the value by 1
/// and, if it reaches 0 - notify one (any) of the writers
impl<Y> Drop for ReadGuard<'_, Y> {
    fn drop(&mut self) {
        if self.lock.state.fetch_sub(1, Release) == 1 {
            wake_one(&self.lock.state);
        }
    }
}

struct WriteGuard<'a, Y> {
    lock: &'a RWLock<Y>,
}

/// Write guard also allows to just read the data
impl<Y> Deref for WriteGuard<'_, Y> {
    type Target = Y;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.value.get() }
    }
}

/// write guard's main purpose - get &mut Y
impl<Y> DerefMut for WriteGuard<'_, Y> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.value.get() }
    }
}

/// unlocking the write guard - we can't tell who and how many are waiting for the lock, so tell 'em all
impl<Y> Drop for WriteGuard<'_, Y> {
    fn drop(&mut self) {
        self.lock.state.store(0, Release);
        wake_all(&self.lock.state);
    }
}
