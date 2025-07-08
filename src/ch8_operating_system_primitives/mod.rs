//! # Operating System Primitives
//! Blocking operations require an efficient way to suspend current thread.  We've used spinning, but it's bad.
//! OS kernel's scheduling subsystem can put thread to sleep. Rust has ways to inform kernel that current thread is waiting.
//!
//! ## Interfaces
//! Interface implementations are OS-specific, but somewhat unified between OS-es.
//! Rust program -> Rust's StL -> OS lib (e.g. glibc) -> OS syscall within kernel.
//! A. `libc` crate could interfere the above path.
//! B. syscalls could be called manually through `interrupts`.
//!   
//! The above way is the idiomatic one, though you can call the OS lib directly.
//!
//! Linux: syscalls are stable and reliable  
//! MacOS: syscalls aren't stable => a lot better to use libraries  
//! Windows: no POSIX (a small lie), no libc, but has separated set of libraries e.g. kernel32.dll => no syscalls fun either
//!
//! Mutex-es could be syscalls, but it's slow. Usually syscalls implement something bigger like `sleep` or `wake`.
//!
//! ## POSIX
//! It has `POSIX threads` and the respective interface to manage ones. Including data types and functions for concurrency.
//! - `pthread_create`
//! - `pthread_join`
//! - `pthread_mutex_t`
//!   > It has methods to create and destroy a mutex. The creation takes configuratrion.  
//!   > Mutex-es behaviour on double-locking within a single thread could be configured: UB, error, deadlock, re-lock.  
//!   > There're functions to lock, try to lock, lock for a specified timeframe and unlock a mutex.
//! - `pthread_rwlock_r`
//!   > Also has init and destroy functions.  
//!   > Has a lot less configuration options. E.g. it always deadlock on double-locking for write and a recursive read lock always succeeds.  
//!   > So there's no way to prioritize writers in wrappers for this lock.  
//!   > Locking interface is identical but there're 2 functions for locking: r and w.
//! - `pthread_cond_t`
//!   > Also has init and destroy functions and just a few options.  
//!   > Time-limited functions may use monotonic or real-time clock.  
//!   > There are 2 counterparts: `pthread_cond_timedwait` to wait and `pthread_cond_signal` to wake one (`_broadcast` to wake all threads).
//!
//! There're more: barrier, spin lock, one-time initialization.
//!
//! ## Wrapping in Rust
//! Rust has e.g. borrowing rules, so we can't just wrap C types as-is.
//! See [Mutex].

use std::cell::UnsafeCell;

/// Interior mutability pattern helps to set rules for the C type => wrap to [UnsafeCell].  
/// Rust moves objects a lot, but C objects frequently rely on its constant memory address.
/// This address can be stored somewhere => moving isn't okay. 2 approaches:
/// - make interface unsafe and let the user deal with it
/// - hide ownership behind a wrapper
///
/// [Box] is a good wrapper for the purpose, as it can be moved, but its content stays in the same place.  
///
/// Downsides:
/// - extra heap allocation for each Mutex
/// - `Mutex::new` cannot be `const` => no static Mutex-es
/// - the underlying mutex recursive locking is UB => has to have an `unsafe fn lock`
/// - a mutex guard can be leaked (leak is safe) => there's an ever-locked mutex noone can drop
///     > but pthread says destroying a locked mutex is UB  
///     > lock-unlock-(drop-or-leak) solves the problem, but quite a logic to have in `Drop`
/// Same is applicable to all of the sync primitives from pthread.
pub struct Mutex {
    _m: Box<UnsafeCell<libc::pthread_mutex_t>>,
}
