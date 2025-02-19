/*
 * It's not practical to send thread to sleep on lock if
 * mutex is locked for brief moments of time.
 * It's better to spinlock on the mutex and leave the thread awake in this case.
 *
 * std::sync::Mutex spin-locks for some time too as an attempt to combine the best of 2 worlds
 */

use std::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{
        AtomicBool,
        Ordering::{Acquire, Release},
    },
    thread,
};

// our spinlock implementation
pub struct SpinLock<Y> {
    locked: AtomicBool,
    value: UnsafeCell<Y>,
}
// UnsafeCell makes the above definition !Sync => we can't share it between threads
// We promice to the compiler that our type is Sync as long as Y is Sync
unsafe impl<Y> Sync for SpinLock<Y> where Y: Send {}

impl<Y> SpinLock<Y> {
    pub const fn new(value: Y) -> Self {
        Self {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    // it returned a reerence to the underlying data
    // pub fn lock<'a>(&'a self) -> &'a mut Y {
    // 'a just an illustration, lifetime elision works too
    pub fn lock<'a>(&'a self) -> Guard<'a, Y> {
        while self.locked.swap(true, Acquire) {
            // Acquire makes sure the previous lock owner has happens-before with the current one
            std::hint::spin_loop(); // give a hint to CPU that this loop is waiting
        }
        // SAFETY: the value is protected by the atomic boolean
        // unsafe { &mut *self.value.get() }
        Guard { lock: self } // it can't be made by any other means
    }

    // SAFETY: The &mut Y from lock() must be disposed along with all its tails
    //         before calling me to life
    // ... it would be nice to do so automagically, innit ? see to the guard
    pub unsafe fn unlock(&self) {
        self.locked.store(false, Release);
    }
}

// a guard to Deref like &mut and Drop like .unlock()
pub struct Guard<'a, Y> {
    lock: &'a SpinLock<Y>,
}

impl<Y> Deref for Guard<'_, Y> {
    type Target = Y;
    fn deref(&self) -> &Self::Target {
        // SAFETY: existence of the guard is caused by locking its lock
        unsafe { &*self.lock.value.get() }
    }
}

impl<Y> DerefMut for Guard<'_, Y> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: existence of the guard is caused by locking its lock
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<Y> Drop for Guard<'_, Y> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Release);
    }
}

pub fn run() {
    let x = SpinLock::new(Vec::new());
    thread::scope(|s| {
        s.spawn(|| x.lock().push(1));
        s.spawn(|| {
            let mut vec_guard = x.lock();
            vec_guard.push(2);
            vec_guard.push(2);
        });
    });
    let vec_guard = x.lock();
    assert!(vec_guard.as_slice() == [1, 2, 2] || vec_guard.as_slice() == [2, 2, 1]);
    println!("x is {:?}", *vec_guard);
}
