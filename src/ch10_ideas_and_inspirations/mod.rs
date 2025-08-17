//! # Ideas and Inspirations
//! It's not a typical chapter => there're my notes on the topics

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
