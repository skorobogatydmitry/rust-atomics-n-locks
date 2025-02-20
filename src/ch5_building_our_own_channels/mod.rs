/*
 * Channels are used to send data between threads.
 * There are flavours:
 * - 1:1, 1:many, many:1, many;many
 * - blocking / non-blocking
 * - throughput / latency -optimized
 * ... there are more. Below are some of variations.
 */

use std::{
    collections::VecDeque,
    sync::{Condvar, Mutex},
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

pub fn run() {}
