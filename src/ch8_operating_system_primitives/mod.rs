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
//! ### Futex Operations
//!
//! There're more that WAIT and WAKE. The syscall takes:
//! 1. atomic to operate on
//! 2. operation code and 2 options: FUTEX_PRIVATE_FLAG, FUTEX_CLOCK_REALTIME
//! 3. operation-specific tail of parameters
//!
//! #### FUTEX_WAIT
//!
//! Tail: expected atomic's value and wait timeout.  
//! Logic: if atomic's value matches - wait until woken up or timeout.
//!
//! May spuriously wake.
//!
//! The value checking and blocking happens atomically for futex operations.
//!
//! FUTEX_CLOCK_REALTIME allows to use system clock instead of monotonic ([std::time::Instant] VS [std::time::SystemTime] in rust).
//!
//! Returns: value says whether variable's value matched and whether timeout was reached.
//!
//! _No way to tell is we were woken up spuriously?_
//!
//! #### FUTEX_WAKE
//!
//! Tail: # of threads to wake up (i32).  
//! Logic: wakes up as many (of fewer) blocked threads as specified.  
//! Returns: # of woken threads.
//!
//! #### FUTEX_WAIT_BITSET
//!
//! Tail: expected atomic's value, max time to wait, non-relevant pointer, u32 "bitset".  
//! Logic: similar to [FUTEX_WAIT](#futex_wait) but:
//! - if bitset has one or more 1-bits in common with [FUTEX_WAKE_BITSET](#futex_wake_bitset)'s, the thread gets woked up (FUTEX_WAKE can't be ignored)
//! - timestamp is always absolute, not duration
//!
//! The first feature allows to split futex waiters into groups. E.g. "readers" and "writers". Using 2 atomics could be more efficient though.
//!
//! #### FUTEX_WAKE_BITSET
//!
//! Tail: # of threads to wake up, two pointers to ignore, u32 "bitset".  
//! Logic: wakes up # of threads where bitset has one or more 1-bits in common with ours.
//!
//! With u32::MAX as bitset, it's identical to [FUTEX_WAKE](#futex_wake).
//!
//! #### FUTEX_REQUEUE
//!
//! Tail: # of threads to wake up, # of threads to re-queue, the address of a secondary atomic variable.  
//! Logic: wake up # of threads, re-queue # of threads among remaining waiters on the new atomic specified.  
//! Returns: # of woken threads.
//!
//! It's an elegant way to schedule multiple waiters: wake one, leave the rest on another barrier.
//!
//! #### FUTEX_CMP_REQUEUE
//!
//! Tail: # of threads to wake up, # of threads to re-queue, the address of a secondary atomic variable, the expected value of the current atomic.  
//! Logic: similar to [FUTEX_REQUEUE](#futex_requeue) but checks the original atomic's value. As usual, comparison and requeue happens atomically for other futex ops.  
//! Returns: the sum of the number of awoken and requeued threads.
//!
//! #### FUTEX_WAKE_OP
//!
//! Tail:
//!   - \# of threads to wake up on the primary atomic variable (i32)
//!   - \# of threads to potentially wake up on the 2nd atomic variable
//!   - the address of a secondary atomic variable
//!   - 32-bit value encoding an operation and a comparison to be made
//! Logic:
//!   - modify the secondary atomic variable with the operation
//!   - wake # of threads on the primary atomic
//!   - checks the secondary atomic matches the condition
//!   - if so => wake up # of threads on the secondary variable
//! Returns: total \# of awoken threads
//!
//! It's a heavily specialized operation. As usual, it happens atomically for other futex ops.  
//! Code:
//! ```
//! let old = atomic2.fetch_update(Relaxed, Relaxed, some_operation);
//! wake(atomic1, N);
//! if some_condition(old) {
//!   wake(atomic2, M);
//! }
//! ```
//!
//! Available operations:
//! - assignment
//! - addition
//! - binary `or`
//! - binary `and-not`
//! - binary `xor`
//! Argument takes 12 bits in plain or in a form of power of 2.
//!
//! Available comparisons: ==, !=, <, <=, >, >=. Argument has similar format.
//!
//! `linux-futex` crate may be of a great help in making this argument.
//!
//! It was made specifically for one use-case in glibc: to wake up 2 threads on 2 separated atomic variables.
//! Current implementation no longer uses [FUTEX_WAKE_OP](#futex_wake_op).
//!
//! NetBSD supports all these operations too.
//! OpenBSD supports only [FUTEX_WAKE](#futex_wake), [FUTEX_WAIT](#futex_wait) and [FUTEX_REQUEUE](#futex_requeue).
//! FreeBSD has no futex, but `_umtx_op`, which includes something close to [FUTEX_WAKE](#futex_wake) and [FUTEX_WAIT](#futex_wait).
//!
//! ### FUTEX_PRIVATE_FLAG
//!
//! `FUTEX_PRIVATE_FLAG` can be added to any of the operations to enable some optimization.
//! The optimization takes place if all operations with the same flag happen within the same process.
//! This allows to skip some expensive steps.
//!
//! ### New futex operations
//!
//! Linux 5.16 has `futex_waitv`, which waits on multiple variables and respective values at the same time.  
//! A wake for any variable wakes the thread. It also allows to specify variable size rather than 32-bit of the usual futex.
//!
//! ### Priority Inheritance Futex Operations
//!
//! Priority inversion happens when a high-priority thread waits on a lock held by a low-priority thread.  
//! This is solved by temporary pulling the low priority thread's priority. It's called priority inheritance.  
//! There are 6 more futex operations to implement _priority inheriting locks_.
//!
//! In order for kernel to detect locking it needs to add some semantic to the futex-es atomic variable:
//! - highest bit: whether there're waiting threads
//! - 2nd highest bit: set if locking thread terminated without unlocking (lock poisoning)
//! - lowest 30 bits: lock holding thread ID / 0 if none
//!
//! The six: `FUTEX_LOCK_PI`, `FUTEX_LOCK_PI2`, `FUTEX_UNLOCK_PI`, `FUTEX_TRYLOCK_PI`, `FUTEX_CMP_REQUEUE_PI` and `FUTEX_WAIT_REQUEUE_PI`.
//! See `man 2 futex` for details.
//!
//! ## macOS
//!
//! Interfaces for syncronization primitives lay in libraries for C (libc), C++ (libc++), Objective-C and Swift.
//!
//! libc has pthread due to POSIX compatibility. This pthreads is a foundation for locks in most languages,
//! but it's slower that on other OS-es. The reason is that macOS-es locks are **fail locks**,
//! what means that arrived threads are served in the order of arrival.  
//! This has an overhead on maintaining the order.
//!
//! macOS 10.12 introduced `os_unfair_lock` - a new lightweight platform-specific unfail mutex.
//! It's 32-bit, statically initialized to `OS_UNFAIR_LOCK_INIT` constant, doesn't require destruction.
//! It can be locked in a blocking and non-blocking way.
//! It doesn't have condvar and rw variant.
//!
//! ## Windows
//!
//! "Native API" isn't supposed to be used and is like syscalls.  
//! "Win32 API" is like the libc and contains everything. It's available through `windows` and `windows-sys` crates.
//!
//! ### Heavyweight Kernel Objects
//!
//! There're old-fasioned syncronization primitives on Windows implemented as objects. Objects are files-alike: have names, permissions, etc.
//! Some of them:
//! - Mutex (lock/unlock)
//! - Event (signal/wait for)
//! - WaitableTimer (auto-signal on timeout or interval)
//!
//! Creation of an object returns `HANDLE`, as for files. This handle could be easily passed around to e.g wait.
//!
//! ### Lighter-Weight Objects
//!
//! `critical section` is more lightweight. It denotes a section of code which can be only executed by one threads at a time.
//! Its functionality is very similar to what a mutex can do.
//!
//! `CRITICAL_SECTION` is a _recursive mutex_, which can be locked (entered) and unlocked (leaved).
//! As a recursive mutex, it protects the section from other threads and ignores re-entering by the same thread.
//! Hence, it can't be used to obtain `& mut`, because the same thread could obtain it twice from the OS, what's UB.
//!
//! ### Slim reader-writer locks
//!
//! Windows Vista and newer includes SRW locks (Slim reader-writer locks):
//! - `SRWLOCK` is one pointer in size
//! - can be statically initialized with SRWLOCK_INIT
//! - doesn't require destruction
//! - movable
//!
//! It supports exclusive (writer) / shared (reader) blocking / non-blocking locking / unlocking functions.
//! It's a ready-to-go mutex if you ignore the reader part.
//!
//! It's unfair (tries to maintain order but without guarantees), acquiring second shared lock may lead to deadlock.
//!
//! `CONDITION_VARIABLE` was introduced at the same time as `SRWLOCK`. `CONDITION_VARIABLE` is also:
//! - is one pointer in size
//! - can be statically initialized with CONDITION_VARIABLE_INIT
//! - doesn't require destruction
//! - movable
//!
//! It can be used with SRWLOCK or critical section or could be woken up directly.
//!
//! ### Address-Based Waiting
//!
//! Windows 8 introduced something extremely alike to Linux's `FUTEX_WAIT` and `FUTEX_WAKE`: `WaitOnAddress` and `WakeByAddress`.
//!
//! The only major difference is that it can operate on 8, 16, 32 and 64 -bit atomic variables.
//! It's also the building block for the rest of syncronization primitives of the "Win32 API".
//!
//!
//! # Takeaways
//! - syscall calls the kernel
//! - programs usually use libraires, not syscalls
//! - **libc** crate gives access to **libc**'s functions.
//! - libc of actual OS-es usually carries more than POSIX C requires / guarantees
//! - POSIX includes pthreads library with some syncronization primitives
//! - **pthread_** types are designed for C, what poses some problems on wrapping
//! - Linux has **FUTEX_** syscall family with waiting and waking on an AtomicU32.
//! - maacOS has an extra to pthread - `os_unfair_lock`, which is a bit faster on macOS
//! - Windows objects are heavy-weight primitives
//! - `SRWLOCK`` and `CONDITION_VARIABLE` are more lightweight and Rust-friendly
//! - Windows also has futex-like functionality these days
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
