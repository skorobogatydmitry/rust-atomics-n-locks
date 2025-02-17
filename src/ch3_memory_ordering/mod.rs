use std::sync::atomic::Ordering::Relaxed;

// compiler and processor skims your program a lot
// => not all operations are wrote sequentially are executed sequentially

use std::sync::atomic::AtomicI32;
use std::thread::{self, sleep};
use std::time::Duration;

// an example function where compiler could do ...
fn f(x: &mut i32, y: &mut i32) {
    *x += 1;
    *y += 1;
    *x += 1;
}
// ... this:
fn f2(x: &mut i32, y: &mut i32) {
    *x += 2;
    *y += 1;
}
// important part is that the program's logic stays the same

// this optimization doesn't take threads into account
// => atomics is a sign that improper optimisation is possible
// => we always give some guidance for operations on atomics

// Ordering is the hint. There are only 3 options:
// - relaxed (Relaxed)
// - release & acquire (Release, Acquire, AcqRel)
// - sequentially consistent (SeqCst)

// Memory model is a set of abstract rules devs rely on and compiler writers follow.
// E.g. borrowing rules is a part of it.
// Memory modes establishes "happens-before" order relationship between operations.
// All the cases unpinned by the model are allowed to drift in order.
// A baseline: all in a single thread happens sequentially.
// Threads only have order guarantees when:
// - spawn / join
// - mutex lock / unlock
// - atomic ops with non-related order

// Relaxed is the most basic & performant ordering
static X: AtomicI32 = AtomicI32::new(0);
static Y: AtomicI32 = AtomicI32::new(0);
fn a() {
    X.store(10, Relaxed); // happens before the next line
    Y.store(20, Relaxed);
}
// === there're no other guarantees between the functions ===
fn b() {
    let y = Y.load(Relaxed); // happens before the next line
    let x = X.load(Relaxed);
    println!("{x} {y}");
}

// functions call outcomes are:
// 0 0 => b loaded all before a stored the values
// 10 20 => a stored all the values before b loaded 'em
// 10 0 => Y is loaded and X is stored, then X is loaded (10) and Y is stored - still intuitive
// 0 20 - is (SIC!) also possible, as there are NO guarantees between the threads
// ... so thread with a(); may fully execute the fn
// but this doesn't mean that the other thread with b(); sees the result

// spawn() creates a happens-before between what preceeeds the call and the new thread
pub fn run() {
    spawn_and_join();
    relaxed_yet_guaranteed();
    out_of_thin_air();
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

// Relaxed ordering is a gatekeeper of the simple guarantee:
// all operations on atomics happen in some order and don't interfere
// (!) this order is the same for all threads
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
    // but if we split modifications
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

// out-of-thin-air values relaxed ordering could cause
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
    // (!) assertions below might fail due to moemry model we have so far
    assert_eq!(X.load(Relaxed), 0);
    assert_eq!(Y.load(Relaxed), 0);
}
