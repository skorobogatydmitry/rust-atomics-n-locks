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
    marker::PhantomData,
    mem::MaybeUninit,
    sync::{
        atomic::{
            AtomicBool, AtomicU8,
            Ordering::{Acquire, Relaxed, Release},
        },
        Arc, Condvar, Mutex,
    },
    thread::{self, scope, Thread},
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

/*
 * Safety by types: consuming objects on sending and receiving assures us there are no 2nd calls
 * ... but it forces to have 2 objects - Sender and Receiver
 *
 * it's cool that there's no need for `in_use' as .send can't be called twice. Sender is an oneshot value
 * it's unfortunate that we need to have Arc, which has its penalty
 */

// an internal helper to store the message and the flag, non-public
struct TypeSafeChannel<Y> {
    message: UnsafeCell<MaybeUninit<Y>>,
    ready: AtomicBool,
    // there's no need for in_use, as Sender::send guarantees that there'are no 2 places to call it
}

unsafe impl<Y> Sync for TypeSafeChannel<Y> where Y: Send {}

pub struct Sender<Y> {
    channel: Arc<TypeSafeChannel<Y>>, // just store channel - one for the pair
}
pub struct Receiver<Y> {
    channel: Arc<TypeSafeChannel<Y>>,
}

pub fn typesafe_channel<Y>() -> (Sender<Y>, Receiver<Y>) {
    let channel = Arc::new(TypeSafeChannel {
        message: UnsafeCell::new(MaybeUninit::uninit()),
        ready: AtomicBool::new(false),
    });
    (
        Sender {
            channel: channel.clone(),
        },
        Receiver { channel },
    )
}

/* impls are very similar, but:
 * - methods are split between the Sender and the Receiver
 * - `send' and `receive' consume self so can't be called multiple times
 * - `send' can't panic, as it can be used just once
 */

impl<Y> Sender<Y> {
    // note that it consumes self
    pub fn send(self, message: Y) {
        unsafe { (*self.channel.message.get()).write(message) };
        self.channel.ready.store(true, Release);
    }
}

impl<Y> Receiver<Y> {
    pub fn is_ready(&self) -> bool {
        self.channel.ready.load(Relaxed)
    }

    pub fn receive(self) -> Y {
        // load false to the flag so Drop can leverage it
        if !self.channel.ready.swap(false, Acquire) {
            panic!("no message available");
        }
        unsafe { (*self.channel.message.get()).assume_init_read() }
    }
}

impl<Y> Drop for TypeSafeChannel<Y> {
    fn drop(&mut self) {
        if *self.ready.get_mut() {
            unsafe { self.message.get_mut().assume_init_drop() }
        }
    }
}

/*
 * Borrowing to avoid allocation
 * The same TypeSafeChannel can be borrowed manually by Sender / Receiver to avoid having Arc and
 * - sacrifice usability
 * - improve perf
 */

struct BorrowingSender<'a, Y> {
    channel: &'a TypeSafeChannel<Y>,
}

struct BorrowingReceiver<'a, Y> {
    channel: &'a TypeSafeChannel<Y>,
}

impl<Y> TypeSafeChannel<Y> {
    pub const fn new() -> Self {
        Self {
            message: UnsafeCell::new(MaybeUninit::uninit()),
            ready: AtomicBool::new(false),
        }
    }

    /*
     * 1. it borrows self through an exclusive reference
     * 2. the content behind mut ref isn't used
     * 3. the method re-inits the structure
     * 4. so channel could be called multiple times
     * 5. the new value re-borrowed 2 times - one for Sender and one for Receiver
     * 6. these borrows have the same lifetime as the original link
     * 7. self can't be re-borrowed untill Sender and Receiver Drop
     *
     * explicit lifetime spec isn't needed here
     */
    pub fn split<'a>(&'a mut self) -> (BorrowingSender<'a, Y>, BorrowingReceiver<'a, Y>) {
        *self = Self::new(); // + invoke Drop for the old channel
        (
            BorrowingSender { channel: self },
            BorrowingReceiver { channel: self },
        )
    }
}

// the rest is pretty much the same...

impl<Y> BorrowingSender<'_, Y> {
    pub fn send(self, message: Y) {
        unsafe { (*self.channel.message.get()).write(message) };
        self.channel.ready.store(true, Release);
    }
}

impl<Y> BorrowingReceiver<'_, Y> {
    pub fn is_ready(&self) -> bool {
        self.channel.ready.load(Relaxed)
    }
    pub fn receive(self) -> Y {
        if !self.channel.ready.swap(false, Acquire) {
            panic!("no message available");
        }
        unsafe { (*self.channel.message.get()).assume_init_read() }
    }
}

// it already exists above
// impl<Y> Drop for TypeSafeChannel<Y> {
//     fn drop(&mut self) {
//         if *self.ready.get_mut() {
//             unsafe { self.message.get_mut().assume_init_drop() };
//         }
//     }
// }

/*
 * Blocking - lack of blocking interface to wait for message
 */

struct BlockingSender<'a, Y> {
    channel: &'a TypeSafeChannel2<Y>,
    receiving_thread: Thread, // <-- new
}

struct BlockingReceiver<'a, Y> {
    channel: &'a TypeSafeChannel2<Y>,
    _no_send: PhantomData<*const ()>, // it's not an ownership marker but an anchor to dismiss Send from the type
                                      // otherwise, receiver can be moved, what invalidates `receiving_thread' variable
}

// a copy of the above to have 2nd impl
struct TypeSafeChannel2<Y> {
    message: UnsafeCell<MaybeUninit<Y>>,
    ready: AtomicBool,
}

unsafe impl<Y> Sync for TypeSafeChannel2<Y> where Y: Send {}

impl<Y> TypeSafeChannel2<Y> {
    fn new() -> Self {
        Self {
            message: UnsafeCell::new(MaybeUninit::uninit()),
            ready: AtomicBool::new(false),
        }
    }

    fn split<'a>(&'a mut self) -> (BlockingSender<'a, Y>, BlockingReceiver<'a, Y>) {
        *self = Self::new();
        (
            BlockingSender {
                channel: self,
                receiving_thread: thread::current(), // only current thread can receive
            },
            BlockingReceiver {
                channel: self,
                _no_send: PhantomData,
            },
        )
    }
}

impl<Y> BlockingSender<'_, Y> {
    fn send(self, message: Y) {
        unsafe { (*self.channel.message.get()).write(message) };
        self.channel.ready.store(true, Release);
        self.receiving_thread.unpark(); // let the receiving thread know there's data
    }
}

// Receiver now waits for the message forever,
// but doesn't panic if there's no message
impl<Y> BlockingReceiver<'_, Y> {
    fn receive(self) -> Y {
        while !self.channel.ready.swap(false, Acquire) {
            thread::park();
        }
        unsafe { (*self.channel.message.get()).assume_init_read() }
    }
}

// ... there are many more options

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

    thread::scope(|s| {
        let (sender, receiver) = typesafe_channel();
        let t = thread::current();
        s.spawn(move || {
            sender.send("a typesafe hello");
            t.unpark();
            // sender.send("this message cannot be sent, as the sender is already consumed");
        });
        while !receiver.is_ready() {
            thread::park(); // a little bit inconvenient to wait manually
        }
        assert_eq!("a typesafe hello", receiver.receive());
    });

    let mut tsch = TypeSafeChannel::new();
    thread::scope(|s| {
        let (sender, receiver) = tsch.split();
        let t = thread::current();
        s.spawn(move || {
            sender.send("typesafe hello # 1");
            t.unpark();
        });

        while !receiver.is_ready() {
            thread::park();
        }
        assert_eq!("typesafe hello # 1", receiver.receive());
    });
    // ... and again
    thread::scope(|s| {
        let (sender, receiver) = tsch.split();
        let t = thread::current();
        s.spawn(move || {
            sender.send("typesafe hello # 2");
            t.unpark();
        });

        while !receiver.is_ready() {
            thread::park();
        }
        assert_eq!("typesafe hello # 2", receiver.receive());
    });

    // blocking
    let mut bch = TypeSafeChannel2::new();
    thread::scope(|s| {
        let (sender, receiver) = bch.split();
        s.spawn(move || sender.send("a typesafe hello to the blocked thread"));
        // YAY! no need for the loop anymore
        assert_eq!("a typesafe hello to the blocked thread", receiver.receive());
    });
}
