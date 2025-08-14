//! # Condition variable
//!
//! The condvar's interface consists of the wait method that unlocks mutex, waits for a signal then locks the mutex.  
//! There're usually 2 notification modes:
//! - "notify one" / signal
//! - "notify all" / broadcast
//!
//! As the futex, condvar can also wake spuriously and it re-locks the mutex in this case too.
//!
//! The condvar is very similar to the futex-es interface, but it doesn't miss signals on its own when the futex relies on its atomic.
//! So let's use the futex's atomic as a counter to track notifications!
//!
//! ## Memory ordering
//!
//! _Speaking from the [Condvar::wait]'s perspective._
//!
//! First, we don't worry about notifcations that arrived before we unlocked the mutex, as
//! its sender can't do anything with the mutex's data yet (it's locked).
//!
//! But that's what can happen:
//! - we unlock the mutex
//! - another thread locks it, changes value then notifies us (and possibly locks the mutex back)
//!
//! In this case, as there's a happens-before between our unlock and another thread's lock,
//! our load will see the counter's value before the notification from the other thread.
//!
//! On the other hand, nothing guarantees what value will see the wait, 2 cases:
//! - it sees the new value and doesn't lock
//! - it sees the old value and locks, but it's guaranteed to receive the notification in this case as by the [Condvar::notify_one] code.
//!
//! ## Counter overflow
//!
//! The [Condvar::wait] can go to sleep unintentionally due to the counter overflow if it misses exactly 2^32 notifications.
//!
//! There's no good way around it, so we consider the possibility for the abovementioned to happen negligible.
//!
//! See [test::test_condvar].
//!
//! ## Avoiding syscalls
//! There's no need in trying to avoid the wait call, as waiting is the expected thing to happen for Condvars.
//!
//! What make sense to avoid is wake calls if there's nobody to wake up.
//!
//! See [Condvar2] for the new field to track # of waiters.
//!
//! [Condvar2::wait] increments and decrements the counter to let the wakers know it's "waiting".
//!
//! There's a risk for memory ordering, as we added one more variable into the picture, so
//! the wakers may see 0 waiters, but the waiter is there.
//!
//! This can happen:
//! - before the num_waiters.fetch_add - doesn't pose any risk, as mutex still holds the ordering
//! - after the num_waiters.fetch_sub - doesn't pose any risk, as it only can happen as a result of a spurious wakeup
//!
//! ## Avoiding spurious wake-ups
//!
//! The problem: spurious wake-ups compete for mutex's lock, as it tries to lock the mutex _potentially_ in parallel with other threads.
//!
//! The chance of the problem isn't low, as our notify_one can notify the 2nd already-waiting thread in case it re-locked the mutex in-between of the original wait.
//!
//! There's a straight-forward way to solve the issue - another counter to track notifications. Although it poses the problem of priorities:
//! some threads may miss notifications one-by-one beaten by luckier threads.
//!
//! So this optimization is in our growing "in depends" bucket.
//!
//! GNU libc uses a complex 2-leveled priority queue.

use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::{AtomicU32, AtomicUsize};

use atomic_wait::{wait, wake_all, wake_one};

use crate::ch9_building_our_own_locks::p1_mutex::MutexGuard2;

pub struct Condvar {
    counter: AtomicU32,
}

impl Condvar {
    pub fn new() -> Self {
        Self {
            counter: AtomicU32::new(0), // start from the empty counter
        }
    }

    /// the simplest approach - increase the counter then wake the thread(s) to let them see the change
    pub fn notify_one(&self) {
        self.counter.fetch_add(1, Relaxed);
        wake_one(&self.counter);
    }

    /// the simplest approach - increase the counter then wake the thread(s) to let them see the change
    pub fn notify_all(&self) {
        self.counter.fetch_add(1, Relaxed);
        wake_all(&self.counter);
    }

    /// - takes and returns a Guard as a proof that the mutex is locked
    pub fn wait<'a, Y>(&self, guard: MutexGuard2<'a, Y>) -> MutexGuard2<'a, Y> {
        let original_counter_value = self.counter.load(Relaxed);

        // take and unlock the mutex before starting to wait
        let mutex = guard.mutex;
        drop(guard);

        // wait on the counter if it isn't changed yet only
        wait(&self.counter, original_counter_value);

        mutex.lock()
    }
}

#[cfg(test)]
mod test {
    use std::{
        thread::{scope, sleep},
        time::Duration,
    };

    use crate::ch9_building_our_own_locks::p1_mutex::Mutex2;

    use super::*;

    #[test]
    fn test_condvar() {
        let mutex = Mutex2::new(0);
        let condvar = Condvar::new();

        let mut wakeups = 0;
        scope(|s| {
            s.spawn(|| {
                // simulate some work here to let the other thread to lock
                sleep(Duration::from_secs(1));
                // modify the mutex
                *mutex.lock() = 123;
                // let the waiter know the value is ready
                condvar.notify_one();
            });

            let mut g = mutex.lock();
            while *g < 100 {
                g = condvar.wait(g);
                wakeups += 1;
            }

            // check that we've waited for the value to arrive
            assert_eq!(*g, 123);
        });

        // check that the waiting actually happened (and possible some spurious wakeups)
        assert!(wakeups < 10);
    }
}

pub struct Condvar2 {
    counter: AtomicU32,
    num_waiters: AtomicUsize, // no worries about overflows, as it can count all the memory, hence, all the threads
}

impl Condvar2 {
    pub fn new() -> Self {
        Self {
            counter: AtomicU32::new(0),
            num_waiters: AtomicUsize::new(0),
        }
    }

    pub fn notify_one(&self) {
        // only notify if there're waiters
        if self.num_waiters.load(Relaxed) > 0 {
            self.counter.fetch_add(1, Relaxed);
            wake_one(&self.counter);
        }
    }

    pub fn notify_all(&self) {
        // only notify if there're waiters
        if self.num_waiters.load(Relaxed) > 0 {
            self.counter.fetch_add(1, Relaxed);
            wake_all(&self.counter);
        }
    }

    pub fn wait<'a, Y>(&self, guard: MutexGuard2<'a, Y>) -> MutexGuard2<'a, Y> {
        // let the wakers know we're about to start waiting
        self.num_waiters.fetch_add(1, Relaxed);

        let original_counter_value = self.counter.load(Relaxed);
        let mutex = guard.mutex;
        drop(guard);
        wait(&self.counter, original_counter_value);

        // let the wakers know we'ren't waiting anymore
        self.num_waiters.fetch_sub(1, Relaxed);

        mutex.lock()
    }
}
