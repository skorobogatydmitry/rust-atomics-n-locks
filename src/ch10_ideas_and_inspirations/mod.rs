//! # Ideas and Inspirations
//! It's not a typical chapter => there're my notes on the topics
//!
//! # RCU
//! RWLock is good for cases when multiple threads read data frequently and changes are rare.
//! If data is simple, just an atomic may suffice.
//!
//! Though there're no complex structs in std to work with them atomically.
//!
//! The Read, Copy, Update (RCU) pattern is used in this case.
//! An atomic holds pointer to the data. In case it needs to be changes in any way, you:
//! - read the content
//! - copy it to a local variable
//! - change it as needed
//! - replace the pointer atomically with e.g. `compare_and_exchange`
//! - the old data should be de-allocated
//!
//! The other threads may still work with the pointer, so the last step is actually tricky. Options in hand:
//! - Arc
//! - leak
//! - GC (nah?)
//! - hazard pointers: pointer with a way to tell if iit's is in use
//! - quiescent state tracking: make sure all threads arrived at the point where the pointer isn't in use

/// ## Semaphore
///
/// It's a primitive with 2 operations:
/// - signal / up / V - increment the counter to a certain maximum
/// - wait / down / P - decrements the counter, if the counter is 0 - block and wait for a singal operation to be able to proceed
///
/// See the module for my attempt implementing it.
///
/// From the book:
/// - semaphore could be a combilation of Mutex<u32> + Condvar
/// - Mutex can be implemented with Semaphore
/// - don't ~~cros the streams~~ implement 'em in matryoshka style
pub mod semaphore;

#[cfg(test)]
pub mod rcu;
