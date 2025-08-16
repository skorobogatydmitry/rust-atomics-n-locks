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
//!
//! ## Summary
//! - the atomic-wait crate provides platform-independent futex-like functionality
//! - a simplest Mutex requires just 2 states, as the 4th chapter SpinLock
//! - a better Mutex is the one which tracks whether there's anybody to wake up
//! - spinning before going to sleep _can_ be beneficial
//! - a minimal Condvar needs just 1 counter to check before and after unlocking the underlying mutex
//! - a better condvar tracks # of waiters to avoid unnecessary wakes
//! - avoiding spurious wakeups on Condvar is tricky and needs more bookkeeping
//! - a simplest RWLock only needs an atomic counter to manage state
//! - +1 atomic and RWLock can wake writers separately
//! - an extra state semantic allows to prioritize writers, what's good in some cases

pub mod p1_mutex;
pub mod p2_condition_variable;
pub mod p3_read_write_lock;
