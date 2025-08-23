//! A simple RCU example

use std::{
    fmt::Display,
    sync::atomic::{AtomicPtr, Ordering::*},
    thread::{scope, sleep},
    time::Duration,
};

/// data to be held, let's say it's complex
#[derive(Clone)]
pub struct Smart {
    value: u32,
}

impl Smart {
    fn new_ptr(value: u32) -> *mut Self {
        Box::into_raw(Box::new(Self { value }))
    }
}

impl Display for Smart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

#[test]
fn test_simple_rcu() {
    let ptr = AtomicPtr::new(Smart::new_ptr(10));

    scope(|s| {
        // 1st thread - access the data right away and after a small delay
        s.spawn(|| {
            let original_ptr = ptr.load(Relaxed);
            unsafe {
                println!("data before modifications: {}", *original_ptr);
            }
            sleep(Duration::from_millis(100));
            unsafe {
                let ptr = ptr.load(Relaxed);
                println!("data after modifications: {}", *ptr);
            }
            unsafe {
                println!("original data is still available: {}", *original_ptr);
            }
        });

        // 2nd thread - replace data with new one, leaking the old data
        s.spawn(|| {
            let original_ptr = ptr.load(Relaxed);
            // clone the data over to another pointer
            let new_ptr = Box::into_raw(Box::new(unsafe { (*original_ptr).clone() }));
            // some sophisticated manipulations
            unsafe {
                (*new_ptr).value += 1;
            }
            ptr.compare_exchange(original_ptr, new_ptr, Relaxed, Relaxed)
                .unwrap();
        });
    });
}
