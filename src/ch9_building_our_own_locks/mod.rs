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

pub mod p1_mutex;
