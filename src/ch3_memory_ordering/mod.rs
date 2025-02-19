use std::sync::atomic::Ordering::{Acquire, Relaxed, Release, SeqCst};

// compiler and processor skims your program a lot
// => not all operations are wrote sequentially are executed sequentially

use std::sync::atomic::{fence, AtomicBool, AtomicI32, AtomicPtr, AtomicU64};
use std::time::Duration;
use std::{array, thread};

// an example function where compiler could do ...
#[allow(unused)]
fn f(x: &mut i32, y: &mut i32) {
    *x += 1;
    *y += 1;
    *x += 1;
}
// ... this:
#[allow(unused)]
fn f2(x: &mut i32, y: &mut i32) {
    *x += 2;
    *y += 1;
}
// important part is that the program's logic stays the same

// this optimization doesn't take threads into account
// => atomics is a sign that improper optimisation is possible
// => we always give some guidance for operations on atomics

/* Ordering is the hint. There are only 3 options:
 * - relaxed (Relaxed)
 * - release & acquire (Release, Acquire, AcqRel)
 * - sequentially consistent (SeqCst)
 */

/* Memory model is a set of abstract rules devs rely on and compiler writers follow.
 * E.g. borrowing rules is a part of it.
 * Memory modes establishes "happens-before" order relationship between operations.
 * All the cases unpinned by the model are allowed to drift in order.
 * A baseline: all in a single thread happens sequentially.
 * Threads only have order guarantees when:
 * - spawn / join
 * - mutex lock / unlock
 * - atomic ops with non-related order
 */

// Relaxed is the most basic & performant ordering
static X: AtomicI32 = AtomicI32::new(0);
static Y: AtomicI32 = AtomicI32::new(0);

#[allow(unused)]
fn a() {
    X.store(10, Relaxed); // happens before the next line
    Y.store(20, Relaxed);
}
// === there're no other guarantees between the functions ===
#[allow(unused)]
fn b() {
    let y = Y.load(Relaxed); // happens before the next line
    let x = X.load(Relaxed);
    println!("{x} {y}");
}

/*
 * functions call outcomes may be:
 * 0 0 => b loaded all before a stored the values
 * 10 20 => a stored all the values before b loaded 'em
 * 10 0 => Y is loaded and X is stored, then X is loaded (10) and Y is stored - still intuitive
 * 0 20 - is (SIC!) also possible, as there are NO guarantees between the threads
 * ... so thread with a(); may fully execute the fn
 * but this doesn't mean that the other thread with b(); sees the result
 *
 * spawn() creates a happens-before between what preceeds the call and the new thread
 */
pub fn run() {
    spawn_and_join();
    relaxed_yet_guaranteed();
    out_of_thin_air();
    release_and_acquire();
    sequentially_consistent_order();
    atomic_fences();
}
fn spawn_and_join() {
    X.store(1, Relaxed);
    let t = thread::spawn(ff); // store(1) happened => the thread can't see 0
    X.store(2, Relaxed);
    t.join().unwrap();
    X.store(3, Relaxed); // thread is joined => can't see 3
}

fn ff() {
    let x = X.load(Relaxed);
    assert!(x == 1 || x == 2);
}

/*
 * # 1 Relaxed ordering is a gatekeeper of the simple guarantee:
 * all operations on individual atomics happen in some order and don't interfere
 * (!) the change order of a given atomic is the same for all threads
 */
fn relaxed_yet_guaranteed() {
    // only this thread does modifications => the order is 0 5 15
    let t1 = thread::spawn(|| {
        X.fetch_add(2, Relaxed);
        X.fetch_add(10, Relaxed);
    });
    let t2 = thread::spawn(|| {
        let a = X.load(Relaxed);
        let b = X.load(Relaxed);
        let c = X.load(Relaxed);
        let d = X.load(Relaxed);
        println!("{a} {b} {c} {d}");
    });
    t2.join().unwrap();
    t1.join().unwrap();

    X.store(0, Relaxed);
    // but if we split atomic's modifications between threads
    let t1 = thread::spawn(|| {
        X.fetch_add(10, Relaxed);
    });
    let t2 = thread::spawn(|| {
        X.fetch_add(5, Relaxed);
    });
    let t3 = thread::spawn(|| {
        let a = X.load(Relaxed);
        let b = X.load(Relaxed);
        let c = X.load(Relaxed);
        let d = X.load(Relaxed);
        println!("{a} {b} {c} {d}");
    });
    t3.join().unwrap();
    t2.join().unwrap();
    t1.join().unwrap();
    // the result can be technically 0 -> 5 -> 15 and 0 -> 10 -> 15
}

// out-of-thin-air values the relaxed ordering could cause
fn out_of_thin_air() {
    // just resetting the workhorses
    X.store(0, Relaxed);
    Y.store(0, Relaxed);

    let a = thread::spawn(|| {
        let x = X.load(Relaxed);
        Y.store(x, Relaxed);
    });

    let b = thread::spawn(|| {
        let y = Y.load(Relaxed);
        X.store(y, Relaxed);
    });

    // there's a circular dep between a and b
    a.join().unwrap();
    b.join().unwrap();
    // (!) assertions below might fail due to memory model we have so far
    // our model doesn't deal with circular deps
    // => any value for x and y works for the model's reasoning
    assert_eq!(X.load(Relaxed), 0);
    assert_eq!(Y.load(Relaxed), 0);
}

/* # 2 Release and acquire ordering
 * It allows to form a happens-before between threads: Release for store, Acquire for load ops.
 * Mixed ops like compare-and-exchange could use a mix called AcqRel or Release / Acquire individually.
 */

// the example sends some data from a side-thread to the main one
static DATA: AtomicU64 = AtomicU64::new(0);
static READY: AtomicBool = AtomicBool::new(false);

static mut UNSAFE_DATA: u64 = 0;

static mut STRING_DATA: String = String::new();
static LOCKED: AtomicBool = AtomicBool::new(false);

fn release_and_acquire() {
    thread::spawn(|| {
        thread::sleep(Duration::from_millis(100));
        DATA.store(123, Relaxed);
        READY.store(true, Release); // everything before this .store() ...
    });
    // Acquire doesn't block execution, but it tells the .load side to provide memory ordering
    while !READY.load(Acquire) {
        // is visible after this .load()
        thread::sleep(Duration::from_millis(10));
        println!("waiting ...");
    }
    println!("DATA is {}", DATA.load(Relaxed));

    /* the DATA doesn't have to be an atomic due to our syntetic example
     * but it's not easy to leverage
     */
    READY.store(false, Relaxed);
    thread::spawn(|| {
        // SAFETY: nothing else is accessing UNSAFE_DATA, as READY == false
        unsafe { UNSAFE_DATA = 123 };
        READY.store(true, Release); // here we use a separated bool to sync threads
    });

    while !READY.load(Acquire) {
        thread::sleep(Duration::from_millis(100));
        println!("... waiting for unsafe");
    }
    // SAFETY: UNSAFE_DATA is synced by the READY
    unsafe { println!("UNSAFE_DATA is {UNSAFE_DATA}") };

    /*
     * The release-acquire leads to questions about multiple releasers and acquirers.
     * Remember that the Release / Acquire is only about memory ordering =>
     * any Acquire could see any Release, but, once seen
     * all what preceeded the Release is guaranteed to happen-before.
     * Though this doesn't work for the whole chain => pg. 60
     */

    /*
     * Mutex-es is the most common use-case
     * as you want payload behind mutex to be somewhat transactional
     */
    thread::scope(|s| {
        for _i in 0..100 {
            s.spawn(mutex_locked_data_manipulation);
        }
    });
    // SAFETY: all threads mutating the string are joined by the scope
    unsafe { println!("STRING_DATA is {STRING_DATA}") };

    /*
     * Another example is lazy initialization of complex data structure
     */
    thread::scope(|s| {
        for _i in 0..10 {
            s.spawn(lazy_initialization_with_indirection);
        }
    });

    // There's also Consume / Release ordering, which is the same as Acquire, but, in theory, allows more optimization.
    // Rust doesn't support Consume ordering due to its implementation complexity.
}

fn mutex_locked_data_manipulation() {
    if LOCKED
        .compare_exchange(false, true, Acquire, Relaxed)
        .is_ok()
    {
        // SAFETY: there's a LOCKED protecting STRING_DATA
        unsafe { STRING_DATA.push('!') };
        LOCKED.store(false, Release); // assure other users that manipulations with the STRING_DATA are all seen
    }
}

fn lazy_initialization_with_indirection() -> &'static Data {
    static PTR: AtomicPtr<Data> = AtomicPtr::new(std::ptr::null_mut());

    let mut p = PTR.load(Acquire);

    if p.is_null() {
        p = Box::into_raw(Box::new(Data::initialize()));
        if let Err(e) = PTR.compare_exchange(std::ptr::null_mut(), p, Release, Acquire) {
            // Release for Ok is required to bind Data::initialize() with the other threads acquire-loading the pointer
            // Acquire for Err is required to bind Data::initialize() in some other thread with the current thread so we're sure the pointer is fully initialized

            // SAFETY: p comes from Box::into_raw right above
            // and wasn't shared with any other thread
            drop(unsafe { Box::from_raw(p) }); // deallocate our unused pointer
            p = e; // take the data from the other thread who won the race
        }
    }

    println!("pointer {p:?}");

    // SAFETY: p is no null and points to a properly initialized value
    unsafe { &*p }
}

// just an example of "complex" data
#[allow(unused)]
#[derive(Debug)]
struct Data {
    x: i32,
    y: i32,
}

impl Data {
    fn initialize() -> Self {
        Data { x: 10, y: 20 }
    }
}

/*
 * # 3 Sequentially consistent ordering
 * Same as Acquire + Release + global consistent order of all operations.
 * Diff from the Relaxed is that Relaxed guarantees global consistency for a given variable,
 * when SeqCst gurantees operations consistency for all involved ops between all threads.
 * Is rarely needed in practice, could be replaced with fence.
 * A typical example is below
 */

static A: AtomicBool = AtomicBool::new(false);
static B: AtomicBool = AtomicBool::new(false);

static mut MY_STRING: String = String::new();

fn sequentially_consistent_order() {
    let a = thread::spawn(|| {
        A.store(true, SeqCst);
        if !B.load(SeqCst) {
            // SAFETY: is guarded by A + B
            unsafe { MY_STRING.push('!') };
        }
    });

    let b = thread::spawn(|| {
        B.store(true, SeqCst);
        if !A.load(SeqCst) {
            // SAFETY: is guarded by A + B
            unsafe { MY_STRING.push('!') };
        }
    });

    /* there are 6 options:
       A.store -> B.load -> B.store -> A.load => a writes, b skips
       A.store -> B.store -> B.load -> A.load => a skips,  b skips
       A.store -> B.store -> A.load -> B.load => a skips,  b skips
       B.store -> A.load -> A.store -> B.load => a skips,  b writes
       B.store -> A.store -> A.load -> B.load => a skips,  b skips
       B.store -> A.store -> B.load -> A.load => a skips,  b skips

       all of them guarantee that there will be no race, as no operations can be seen inverted between the threads
    */

    a.join().unwrap();
    b.join().unwrap();
    // SAFETY: all users are dead
    unsafe { println!("{MY_STRING}") };
}

/*
 * Ordering could be applied to atomic fences
 * all 'em live in std::sync::atomic::fence
 * 3 kinds:
 * - Release
 * - Acquire
 * - AcqRel / SeqCst
 *
 * fence has a runtime penalty
 *
 * fence is like an anchor which aligns operations:
 *
 *                         op1
 *                         op2
 *                         op3
 * Release fence <--> Acquire fence
 *      op4
 *      op5
 *      op6
 *
 * the above case shows how fences align the timeline
 * (!) but there are no guarantees that the both fences "arrive" to the current moment at the same time
 *     => if op4 doesn't see op1's result, none of op1,op2,op3 happened yet at all
 *
 * SeqCst fence is the only place of the sequentially consistent order => SeqCst fence + Related op != SeqCst op
 *
 * there are also compiler fences, but they don't have runtime guarantees
 * ... but it can be combined with membarrier (for Linux) to increase perf of a combination
 */
fn atomic_fences() {
    A.store(true, Release);
    // is the logically same as
    fence(Release); // <- here we release the fence
    A.store(true, Relaxed);

    A.load(Acquire);
    // is the logically same as
    A.load(Relaxed);
    fence(Acquire);

    // another example
    static PTR: AtomicPtr<Data> = AtomicPtr::new(std::ptr::null_mut());

    let p = PTR.load(Acquire); // let p = PTR.load(Relaxed);
    if p.is_null() {
        println!("no data");
    } else {
        ////////////////////////////////////////// fence(Acquire); - could be used instead of the Acquire ordering above
        println!("data = {:?}", unsafe { Box::from_raw(p) });
    }

    // a simpler example
    static mut DATA: [u64; 10] = [0; 10];
    const ATOMIC_FALSE: AtomicBool = AtomicBool::new(false);
    static DATA_READY: [AtomicBool; 10] = [ATOMIC_FALSE; 10];
    for i in 0..10 {
        thread::spawn(move || {
            let data = 5 * 5 * i as u64; // some complex calc
            unsafe { DATA[i] = data };
            DATA_READY[i].store(true, Release); // Release to make sure data will be there
        });
    }
    thread::sleep(Duration::from_millis(500));
    let ready: [bool; 10] = array::from_fn(|i| DATA_READY[i].load(Relaxed)); // could load in a relaxed mode so the scheduling isn't restrained
    if ready.contains(&true) {
        fence(Acquire); // here we want all data for READY[i] == true to be consistent
        for i in 0..10 {
            if ready[i] {
                println!("data {i} is ready: {}", unsafe { DATA[i] });
            }
        }
    }
}

/* # Summary
 * there's no known global order of operations
 * there's total modification order for each atomic variable - same for all threads
 * happens-before relationship defines the order
 * happens-before works as you read a single thread
 * spawning a thread is happens-before all the thread does
 * all the thread does happens-before the .join()
 * unlocking mutex happens-before locking it again
 * acquire-load followed by release-store makes a happens-before between threads
 * > there could be any # of fetch-and-modify and compare-and-exchange in between
 * there's a consume-load in theory, not in rust
 * sequentially consistent is the strongest ordering, but usually a sign for design review
 * fences are nice to save some time to align multiple atomic operations in memory
 */
