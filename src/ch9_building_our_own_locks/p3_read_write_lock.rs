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
//! ## Avoiding busy-looping writers
//!
//! The [RWLock::write] has a flaw: it may spin a lot if there are many readers constantly do locks and unlocks.
//!
//! See [RWLock2::write] for one of the possible solutions.
//!
//! ## Avoiding writer starvation
//!
//! A typical problem for RWLock: there're many readers pooling data,
//! but the only writer can't take the lock to access the data,
//! as there're always read lock(s).
//!
//! One of the solution is not to read lock if there're waiting writers.
//!
//! See [RWLock3] for the implementation.
//!

use std::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{
        AtomicU32,
        Ordering::{Acquire, Relaxed, Release},
    },
    u32,
};

use atomic_wait::{wait, wake_all, wake_one};

/// Use 1 state to track everything with special values
pub struct RWLock<Y> {
    state: AtomicU32, // 0 - not locked, N - number of readers, U32::MAX - write lock
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
                // The _weak version is used, as the additional cost of a false-negative is low - just repeat the loop
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
        // The non-_weak version is used, as there's already quite a chance to repepat this loop:
        // we don't want any false-negatives
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

/// 2 states: for readers and writers
pub struct RWLock2<Y> {
    state: AtomicU32,               // N - number of readers, U32::MAX - write lock
    writer_wake_counter: AtomicU32, // a signal to wake up writers
    value: UnsafeCell<Y>,
}

/// as the RWLock may have multiple references to Y accessed from different threads,
/// Y has to be not only Send, but also Sync
unsafe impl<Y> Sync for RWLock2<Y> where Y: Send + Sync {}

/// Note the 2 separated methods and structs for different kinds of locks
impl<Y> RWLock2<Y> {
    pub const fn new(value: Y) -> Self {
        Self {
            state: AtomicU32::new(0),
            writer_wake_counter: AtomicU32::new(0),
            value: UnsafeCell::new(value),
        }
    }

    /// exactly the same as [RWLock::read]
    pub fn read(&self) -> ReadGuard2<Y> {
        let mut s = self.state.load(Relaxed);
        loop {
            if s < u32::MAX {
                assert!(s < u32::MAX - 2, "too many readers!");
                match self.state.compare_exchange_weak(s, s + 1, Acquire, Relaxed) {
                    Ok(_) => return ReadGuard2 { lock: self },
                    Err(e) => s = e,
                }
            }
            if s == u32::MAX {
                wait(&self.state, u32::MAX);
                s = self.state.load(Relaxed);
            }
        }
    }

    /// engage the new field to avoid spinning more than needed
    pub fn write(&self) -> WriteGuard2<Y> {
        while self
            .state
            .compare_exchange(0, u32::MAX, Acquire, Relaxed)
            .is_err()
        // just check for error - no need to see the value
        {
            let w = self.writer_wake_counter.load(Acquire);
            if self.state.load(Relaxed) != 0 {
                // if there are read locks, wait on the writer wake counter
                // but only if there have been no wake signals since we've loaded it
                wait(&self.writer_wake_counter, w);
            }
        }
        WriteGuard2 { lock: self }
    }
}

/// Duplicate all guards for [RWLock2] with a twist in [ReadGuard2::drop] and [WriteGuard2::drop]
struct ReadGuard2<'a, Y> {
    lock: &'a RWLock2<Y>,
}

impl<Y> Deref for ReadGuard2<'_, Y> {
    type Target = Y;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.value.get() }
    }
}

impl<Y> Drop for ReadGuard2<'_, Y> {
    fn drop(&mut self) {
        if self.lock.state.fetch_sub(1, Release) == 1 {
            // 1 - signal to the [RWLock2::write] that there's a read guard released
            // this release pairs with the load Acquire in the write method to compose a much needed happens-before
            self.lock.writer_wake_counter.fetch_add(1, Release);
            // 2 - wake an already waiting [RWLock2::write]
            wake_one(&self.lock.writer_wake_counter);
        }
    }
}

struct WriteGuard2<'a, Y> {
    lock: &'a RWLock2<Y>,
}

impl<Y> Deref for WriteGuard2<'_, Y> {
    type Target = Y;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.value.get() }
    }
}

impl<Y> DerefMut for WriteGuard2<'_, Y> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.value.get() }
    }
}

/// we still don't know whom to wake up => wake up both - readers and writers
impl<Y> Drop for WriteGuard2<'_, Y> {
    fn drop(&mut self) {
        self.lock.state.store(0, Release);
        self.lock.writer_wake_counter.fetch_add(1, Release);
        wake_one(&self.lock.writer_wake_counter);
        // a possible optimization: in case the above function returns # of woken up threads, we could skip the below one
        // but that's true not for all and every system
        wake_all(&self.lock.state);
    }
}

/// change state's semantic
pub struct RWLock3<Y> {
    state: AtomicU32, // N / 2 - number of readers, n % 2 - whether a writer is waiting
    writer_wake_counter: AtomicU32, // a signal to wake up writers
    value: UnsafeCell<Y>,
}

unsafe impl<Y> Sync for RWLock3<Y> where Y: Send + Sync {}

impl<Y> RWLock3<Y> {
    pub const fn new(value: Y) -> Self {
        Self {
            state: AtomicU32::new(0),
            writer_wake_counter: AtomicU32::new(0),
            value: UnsafeCell::new(value),
        }
    }

    /// check for odd / even self.state instead of just increments and U32::MAX
    pub fn read(&self) -> ReadGuard3<Y> {
        let mut s = self.state.load(Relaxed);
        loop {
            // was: `if s < u32::MAX {`
            // no writer locks
            if s % 2 == 0 {
                assert!(s < u32::MAX - 2, "too many readers!"); // still useful
                match self.state.compare_exchange_weak(s, s + 2, Acquire, Relaxed) {
                    Ok(_) => return ReadGuard3 { lock: self },
                    Err(e) => s = e,
                }
            }
            if s % 2 == 1 {
                // there's a write waiting to lock
                wait(&self.state, s);
                s = self.state.load(Relaxed);
            }
        }
    }

    /// singificantly changed
    pub fn write(&self) -> WriteGuard3<Y> {
        let mut s = self.state.load(Relaxed);
        loop {
            // try to lock if unlocked
            if s <= 1 {
                // 0 || 1 => unlocked
                // try to lock with U32::MAX (which is odd)
                match self.state.compare_exchange(s, u32::MAX, Acquire, Relaxed) {
                    Ok(_) => return WriteGuard3 { lock: self },
                    Err(e) => {
                        // got unlucky - repeat
                        s = e;
                        continue;
                    }
                }
            }
            // there're locks => block readers by making s % 2 == 1
            if s % 2 == 0 {
                match self.state.compare_exchange(s, s + 1, Acquire, Relaxed) {
                    Ok(_) => {}
                    Err(e) => {
                        // no luck stopping readers => repeat
                        s = e;
                        continue;
                    }
                }
            }
            // wait if it's still locked
            let w = self.writer_wake_counter.load(Acquire);
            s = self.state.load(Relaxed); // refresh, some time passed
            if s >= 2 {
                // there're read locks
                wait(&self.writer_wake_counter, w);
                s = self.state.load(Relaxed); // refresh, some time passed
            }
        }
    }
}

/// Duplicate all guards for [RWLock3] with a twist in [ReadGuard3::drop] and [WriteGuard3::drop]
struct ReadGuard3<'a, Y> {
    lock: &'a RWLock3<Y>,
}

impl<Y> Deref for ReadGuard3<'_, Y> {
    type Target = Y;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.value.get() }
    }
}

// a clearer
impl<Y> Drop for ReadGuard3<'_, Y> {
    fn drop(&mut self) {
        // decrement by 2 to remove 1 read lock
        if self.lock.state.fetch_sub(2, Release) == 3 {
            // If we decremented from 3 to 1, that means
            // the RwLock is now unlocked _and_ there is
            // a waiting writer, which we wake up.
            self.lock.writer_wake_counter.fetch_add(1, Release);
            wake_one(&self.lock.writer_wake_counter);
        }
    }
}

struct WriteGuard3<'a, Y> {
    lock: &'a RWLock3<Y>,
}

impl<Y> Deref for WriteGuard3<'_, Y> {
    type Target = Y;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.value.get() }
    }
}

impl<Y> DerefMut for WriteGuard3<'_, Y> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.value.get() }
    }
}

/// we still don't know whom to wake up => wake up both - readers and writers
impl<Y> Drop for WriteGuard3<'_, Y> {
    fn drop(&mut self) {
        self.lock.state.store(0, Release);
        self.lock.writer_wake_counter.fetch_add(1, Release);
        wake_one(&self.lock.writer_wake_counter);
        wake_all(&self.lock.state);
    }
}
