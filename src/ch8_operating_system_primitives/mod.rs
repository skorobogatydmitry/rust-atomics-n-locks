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
//!
//! ## Linux
//! All pthread primitives are made using `futex` syscall. It operates on a 32-bit atomic integer.  
//! Two main operations:
//! - FUTEX_WAIT - puts current thread to sleep
//! - FUTEX_WAKE - wakes the thread which sleeps on the atomic variable
//!
//! The two don't store anything in the integer.  
//! FUTEX_WAIT requires the expected value the variable should have, doesn't wait otherwise.
//! FUTEX_WAIT is atomic from the WAKE's perspective.  
//! The two features of WAIT allows to make sure a WAKE isn't lost by changing for value before WAKE.
//! So the changed value dismiss any WAIT calls which are late-comers.
//!
//! Implementation example: [futex_wait], [futex_wake_one].
//!
//! Like with parking and condition variables, futex operations can spuriously wake up.
//!

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

#[cfg(target_os = "linux")]
use std::sync::atomic::AtomicU32;

/// That's a minimal example of linux futex-es: start waiting
///
/// # An example of futex usage:
/// - park the main thread with FUTEX_WAIT with 0
/// - have it loop-ed to protect from spontaneous wakes
/// - in a side-thread:
///     - flip value to 1
///     - wake the main
/// It's important that the FUTEX_WAIT is in the loop that checks for atomic to be flipped.  
/// If we just wait on 0, the main thread can be waked spontaneously.
/// It's important that FUTEX_WAIT checks that the atomic variable is still 0 before going to sleep.
/// Otherwise, the WAKE can be lost between entering the loop and the WAIT call.
///
/// Futex allows to avoid any unneeded syscalls by checking the atomic.  
/// E.g., the main thread doesn't enter the loop if the variable is flipped to 1.
/// Also, the side-thread can skip the WAKE if the main thread is waiting.
/// Hence, futex allows to skip all unnecessary syscalls implementing parking.
///
/// I enhanced the example with 2 compare-n-exchange to skip syscalls where possible. Hope it works as expected.
///
/// My note on memory ordering: [std::sys::sync::thread_parking::futex::Parker](https://doc.rust-lang.org/nightly/src/std/sys/sync/thread_parking/futex.rs.html#20)
/// only sets memory ordering as an interface for the API consumers, but the code below works ok with `Relaxed`.
///
/// ```
/// use std::{
///     sync::atomic::{AtomicU32, Ordering::*},
///     thread,
///     time::Duration,
/// };
/// use atomics_n_locks::ch8_operating_system_primitives::*;
/// let a = AtomicU32::new(0);
/// thread::scope(|s| {
///     s.spawn(|| {
///         thread::sleep(Duration::from_secs(1)); // let the main thread to block
///         match a.compare_exchange_weak(0, 1, Relaxed, Relaxed) {
///             Ok(_) => println!("The main thread isn't waiting yet"),
///             Err(new) => {
///                 if new == 2 {
///                     println!("The main thread's already waiting, need to WAKE");
///                     a.store(1, Relaxed);
///                     futex_wake_one(&a);
///                 } else {
///                     panic!("Unknown value: {new}");
///                 }
///             }
///         }
///     });
///     // Uncommenting this line lets the side-thread to bypass syscall and set the atomic to 1
///     // ... so we bypass the syscall too
///     // thread::sleep(Duration::from_secs(3));
///     // Check that the side-thread isn't unlocked us yet
///     thread::park();
///     match a.compare_exchange_weak(0, 2, Relaxed, Relaxed) {
///         Err(new) => println!("Free to go: {new}"),
///         Ok(_) => {
///             println!("Waiting ...");
///             // Wait for the side-thread to flip the variable
///             while a.load(Relaxed) != 1 {
///                 futex_wait(&a, 2); // wait only on 0, exit on 1 right away
///             }
///         }
///     }
///     println!("Done");
/// });
/// ```
#[cfg(target_os = "linux")]
pub fn futex_wait(a: &AtomicU32, expected: u32) {
    // Refer to the futex (2) man page for the syscall signature
    unsafe {
        libc::syscall(
            libc::SYS_futex,                    // syscall "name"
            a as *const AtomicU32,              // atomic to operate on
            libc::FUTEX_WAIT,                   // operation
            expected,                           // expected value
            std::ptr::null::<libc::timespec>(), // no timeout
        );
    }
}

/// That's a minimal example of linux futex-es: waking
/// See [futex_wait] for an explanation.
pub fn futex_wake_one(a: &AtomicU32) {
    // Refer to the futex (2) man page for the syscall signature
    unsafe {
        libc::syscall(
            libc::SYS_futex,       // syscall "name"
            a as *const AtomicU32, // atomic to operate on
            libc::FUTEX_WAKE,      // operation
            1,                     // number of threads
        );
    }
}
