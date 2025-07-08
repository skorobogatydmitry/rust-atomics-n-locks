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
