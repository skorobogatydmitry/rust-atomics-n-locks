/*
 * Channels are used to send data between threads.
 * There are flavours:
 * - 1:1, 1:many, many:1, many;many
 * - blocking / non-blocking
 * - throughput / latency -optimized
 * ... there are more. Below are some of variations.
 */

use std::{
    cell::UnsafeCell,
    collections::VecDeque,
    hint,
    mem::MaybeUninit,
    sync::{
        atomic::{
            AtomicBool, AtomicU8,
            Ordering::{Acquire, Relaxed, Release},
        },
        Condvar, Mutex,
    },
    thread::{self, scope},
};

/* A poor-man-s-channel of VecDeque + Condvar
 * But it's thread-safe, as its content is.
 * Flaws:
 * - no queue size limits
 * - VecDeque extension may take time
 * - receivers compete for the lock through they pull different items
 */
pub struct Channel<Y> {
    queue: Mutex<VecDeque<Y>>,
    item_ready: Condvar,
}

impl<Y> Channel<Y> {
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            item_ready: Condvar::new(),
        }
    }

    // push message to the end of the queue
    pub fn send(&self, message: Y) {
        self.queue.lock().unwrap().push_back(message);
        self.item_ready.notify_one();
    }

    // take one message from the queue
    pub fn receive(&self) -> Y {
        let mut b = self.queue.lock().unwrap();
        loop {
            if let Some(message) = b.pop_front() {
                return message;
            }
            b = self.item_ready.wait(b).unwrap()
        }
    }
}

/*
 * An usafe oneshot channel
 * Oneshot stands for "send one msg and die"
 */

pub struct OneshotChannel<Y> {
    message: UnsafeCell<MaybeUninit<Y>>, // a single cell with unsafe Optional to store data
    ready: AtomicBool,                   // a flag to tell if the message is received
}

unsafe impl<Y> Sync for OneshotChannel<Y> where Y: Send {}

impl<Y> OneshotChannel<Y> {
    pub const fn new() -> Self {
        Self {
            message: UnsafeCell::new(MaybeUninit::uninit()),
            ready: AtomicBool::new(false),
        }
    }

    /*
     * 3 methods of a minimal interface are below ...
     * in all its glory:
     * - calling send 2nd time could cause data race on receiver's side
     * - synchronizing send call from threads is on the end-user too
     * - calling receive multiple times causes several copies of a possibly un-Copy-able object to exist
     * - the channel doesn't drop its content => unreceived data is left hanging
     */

    // SAFETY: only call it once
    pub unsafe fn send(&self, message: Y) {
        (*self.message.get()).write(message);
        self.ready.store(true, Release);
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Acquire)
    }

    // SAFETY: only call it once and when .is_ready() == true
    pub unsafe fn receive(&self) -> Y {
        (*self.message.get()).assume_init_read()
    }
}

/*
 * Starting point is the OneshotChannel above and eliminating UB with runtime checks
 */
pub struct SafeInRuntimeChannel<Y> {
    message: UnsafeCell<MaybeUninit<Y>>,
    ready: AtomicBool,
    in_use: AtomicBool, // a new field to syncronize self.send
}

unsafe impl<Y> Sync for SafeInRuntimeChannel<Y> where Y: Send {}

impl<Y> SafeInRuntimeChannel<Y> {
    pub const fn new() -> Self {
        Self {
            message: UnsafeCell::new(MaybeUninit::uninit()),
            ready: AtomicBool::new(false),
            in_use: AtomicBool::new(false),
        }
    }

    /*
     * check for self.ready mitigates the UB on calling .receive with self.ready == false
     * calling it multiple times is handled by swapping self.ready with false
     * => once a message is received, the channel is reset
     */

    /// # Panics
    /// - if there's no message available yet => use is_ready to check before use
    /// - if the message was already consumed
    pub fn receive(&self) -> Y {
        if !self.ready.swap(false, Acquire) {
            panic!("no message available");
        }
        unsafe { (*self.message.get()).assume_init_read() } // now we're sure the method could be called safely
                                                            // ... but may panic
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Relaxed) // as .receive now Acquire, we could relax the order here
    }

    /// # Panics
    /// - there's another send in progress
    pub fn send(&self, message: Y) {
        if self.in_use.swap(true, Relaxed) {
            // a very similar to what's done to self.receive
            // Relaxed ordering is ok, as there's only 1 atomic involved
            // and it, as by Relaxed, has the T(otal)M(odification)O(rder)
            panic!("channel is in use");
        }
        unsafe { (*self.message.get()).write(message) }; // the above was the only issue with self.send
        self.ready.store(true, Release);
    }
}

// and the final part - Dropping unreceived messages
impl<Y> Drop for SafeInRuntimeChannel<Y> {
    fn drop(&mut self) {
        if *self.ready.get_mut() {
            // acquire the lock by getting the exclusive ref to it and deref it
            unsafe {
                self.message.get_mut().assume_init_drop();
            }
        }
    }
}

// it's possible to compress 2 booleans into one u8
const EMPTY: u8 = 0;
const WRITING: u8 = 1;
const READY: u8 = 2;
const READING: u8 = 3;

pub struct SafeInRuntimeChannelV2<Y> {
    message: UnsafeCell<MaybeUninit<Y>>,
    state: AtomicU8,
}

impl<Y> SafeInRuntimeChannelV2<Y> {
    fn new() -> Self {
        Self {
            message: UnsafeCell::new(MaybeUninit::uninit()),
            state: AtomicU8::new(EMPTY),
        }
    }

    pub fn send(&self, message: Y) {
        if self
            .state
            .compare_exchange(EMPTY, WRITING, Relaxed, Relaxed)
            .is_err()
        {
            panic!("channel is not empty");
        }
        unsafe { (*self.message.get()).write(message) };
        self.state.store(READY, Release); // happens-before is required to make sure message is written
    }

    pub fn is_ready(&self) -> bool {
        self.state.load(Relaxed) == READY
    }

    pub fn receive(&self) -> Y {
        if self
            .state
            .compare_exchange(READY, READING, Acquire, Relaxed) // only successful swap requires Acquire so the data is written
            .is_err()
        {
            panic!("channel is not ready");
        }
        unsafe { (*self.message.get()).assume_init_read() }
    }
}

impl<Y> Drop for SafeInRuntimeChannelV2<Y> {
    fn drop(&mut self) {
        if *self.state.get_mut() == READY {
            unsafe { self.message.get_mut().assume_init_drop() };
        }
    }
}

pub fn run() {
    let ch: Channel<&str> = Channel::new();
    scope(|s| {
        let receiver = s.spawn(|| {
            println!("waiting for a message");
            println!("received `{}'", ch.receive());
        });
        let sender = s.spawn(|| {
            println!("sending a message");
            ch.send("hey!");
            println!("sent a message");
        });
        receiver.join().unwrap();
        sender.join().unwrap();
    });

    let uch = OneshotChannel::new();
    scope(|s| {
        let receiver = s.spawn(|| {
            println!("waiting for a message");
            while !uch.is_ready() {
                hint::spin_loop();
            }
            // SAFETY: we call it once
            let msg = unsafe { uch.receive() };
            println!("received `{msg}'",);
        });
        let sender = s.spawn(|| {
            println!("sending a message");
            // SAFETY: we send it once
            unsafe { uch.send("a less safe hey!") };
            println!("sent a message");
        });
        receiver.join().unwrap();
        sender.join().unwrap();
    });

    let rsch = SafeInRuntimeChannel::new();
    let t = thread::current();
    thread::scope(|s| {
        s.spawn(|| {
            rsch.send("hello, it's safe!");
            // rsch.send("hello, it's safe!"); // leads to panic
            t.unpark(); // unpark the main thread to receive the message
        });
        while !rsch.is_ready() {
            thread::park(); // unpark-park makes happens-before between the threads => is_ready will be set
        }
        // rsch.receive(); // leads to panic
        assert_eq!("hello, it's safe!", rsch.receive());
        // rsch.receive(); // leads to panic
    });

    let rsch2 = SafeInRuntimeChannel::new();
    let t = thread::current();
    thread::scope(|s| {
        s.spawn(|| {
            rsch2.send("hello, it's safe!");
            // rsch2.send("hello, it's safe!"); // leads to panic
            t.unpark(); // unpark the main thread to receive the message
        });
        while !rsch2.is_ready() {
            thread::park(); // unpark-park makes happens-before between the threads => is_ready will be set
        }
        // rsch2.receive(); // leads to panic
        assert_eq!("hello, it's safe!", rsch2.receive());
        // rsch2.receive(); // leads to panic
    });
}
