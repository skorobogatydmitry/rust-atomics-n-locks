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
            AtomicBool,
            Ordering::{Acquire, Release},
        },
        Condvar, Mutex,
    },
    thread::scope,
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
     * - calling receive multiple times causes several copies of a possible un-Copy-able object to exist
     * - the channel doesn't drop its content => unreceived data is to be left hanging
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

    let ch = OneshotChannel::new();
    scope(|s| {
        let receiver = s.spawn(|| {
            println!("waiting for a message");
            while !ch.is_ready() {
                hint::spin_loop();
            }
            // SAFETY: we call it once
            let msg = unsafe { ch.receive() };
            println!("received `{msg}'",);
        });
        let sender = s.spawn(|| {
            println!("sending a message");
            // SAFETY: we send it once
            unsafe { ch.send("a less safe hey!") };
            println!("sent a message");
        });
        receiver.join().unwrap();
        sender.join().unwrap();
    });
}
