//! # Ideas and Inspirations
//! It's not a typical chapter => there're my notes on the topics
//!
//! ## Lock-Free Linked List
//! Using RCU on individual elements of a linked list allows to independently work with the list from multiple threads:
//! add, remove elements without locking the list or waiting on the either side.
//!
//! This requires the list to be based on AtomicPtr's. To add an element:
//! - make the new head
//! - set the first element of the list as `next` through AtomicPtr
//! - set the element as the first through AtomicPtr
//!
//! If there are multiple writers adding and removing elements, they should sync their operations for neighbours.
//! Otherwise, 2 threads can remove and add Nth element in a way that makes a dangling chain of the two:
//! - thread 1 reads `current.next`
//! - thread 2 removes `current.next` from the list by setting `current.next` to `current.next.next`
//! - thread 1 uses `current.next` as `new.next`
//!
//! This case can be solved using Mutex, which adds some overhead to writes, but keeps reads independent and cheap.
//!
//! Detaching an element poses the same problem with deallocation as the vanilla RCU. Solutions are the same.
//!
//! ## Queue-Based Locks
//!
//! Usually, kernel decides who to wake on a Mutex, as futex is used under the hood.
//!
//! But it's sometimes suitable to have a queue under your control for the matter.
//!
//! It could be done with a linked list built on a single AtomicPrt pointing to a list of waiting threads.
//! Each element should hold the needful to wake the corresponding thread.
//!
//! The queue could be:
//! - protected by its own lock bit or could be partially lock-free (RCU?)
//! - allocated on stacks of the waiting threads: they won't be disposed
//! - doubly-linked to ease navigation
//! - with a pointer to the last element to speed up `push_back`
//!
//! This pattern allows to implement locking primitives with thread parking (without `wake_all` capability).
//!
//! ## Parking Lot-Based Locks
//!
//! The idea is to have a global HashMap, which maps memory addresses to queues of threads waiting on an address.
//! > This HashMap is usually called a parking lot.
//!
//! Mutex requires 2 bits to track state: `is_locked` and `has_queue`. They can be squeezed into the pointer.
//! It makes Mutex's memory cost very low.
//!
//! This approach allows to provided a futex-like functionality on platforms where there's no futex provided by OS.
//!
//! There's a Rust crate called [parking_lot](https://crates.io/crates/parking_lot).

/// ## Semaphore
///
/// It's a primitive with 2 operations:
/// - signal / up / V - increment the counter to a certain maximum
/// - wait / down / P - decrements the counter, if the counter is 0 - block and wait for a singal operation to be able to proceed
///
/// See the module for my attempt implementing it.
///
/// From the book:
/// - semaphore could be a combilation of `Mutex<u32>` + Condvar
/// - Mutex can be implemented with Semaphore
/// - don't ~~cros the streams~~ implement 'em in matryoshka style
pub mod semaphore;

/// # RCU
/// RWLock is good for cases when multiple threads read data frequently and changes are rare.
/// If data is simple, just an atomic may suffice.
///
/// Though there're no complex structs in std to work with them atomically.
///
/// The Read, Copy, Update (RCU) pattern is used in this case.
/// An atomic holds pointer to the data. In case it needs to be changes in any way, you:
/// - read the content
/// - copy it to a local variable
/// - change it as needed
/// - replace the pointer atomically with e.g. `compare_and_exchange`
/// - the old data should be de-allocated
///
/// The other threads may still work with the pointer, so the last step is actually tricky. Options in hand:
/// - Arc
/// - leak
/// - GC (nah?)
/// - hazard pointers: pointer with a way to tell if iit's is in use
/// - quiescent state tracking: make sure all threads arrived at the point where the pointer isn't in use
#[cfg(test)]
pub mod rcu;

/// ## Sequence lock
///
/// It's an approach, alternative to RCU, that allows to update big data structures without traditional locks.
///
/// There's one counter, which is:
/// - even if the data is available for reading
/// - even if the data is being updated
///
/// The writer should increment the counter once to "lock" the data and increment it once more after modifications.
///
/// The reader can read data at any point and check if it's consistent by comparing the counter before and after the read operation.
///
/// This pattern is heavily used in embedded systems, Linux, with shared memory, in multi-process environments.
#[allow(unused)]
pub mod seqlock;
