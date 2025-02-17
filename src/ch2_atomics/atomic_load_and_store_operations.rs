use std::{
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed},
    thread::{self, sleep, spawn},
    time::Duration,
};

pub fn run() {
    // load - gets data from an atomic
    // store - puts new data to an atomic

    // use-case: coordinated shutdown
    static STOP: AtomicBool = AtomicBool::new(false);

    let background_worker = spawn(|| {
        while !STOP.load(Relaxed) {
            // do some work in the loop
            sleep(Duration::from_millis(200));
            println!(".. keep going");
        }
    });

    println!("do some job in the main thread");
    sleep(Duration::from_millis(1000));
    println!("shutdown");
    STOP.store(true, Relaxed);

    background_worker.join().unwrap();

    // use-case: process tracking
    let progress = AtomicUsize::new(0);
    let main_thread = thread::current();

    thread::scope(|s| {
        s.spawn(|| {
            for i in 0..100 {
                // println!("processing {i}");
                sleep(Duration::from_millis(60)); // let's say an item takes 100ms to be processed
                progress.store(i + 1, Relaxed);
                main_thread.unpark(); // let the main thread know there's something to check
            }
        });
        // check status every second
        loop {
            let n = progress.load(Relaxed);
            if n == 100 {
                break;
            }
            println!("still working: {n}%");
            // sleep(Duration::from_secs(1)); - this introduces a delay at exit
            thread::park_timeout(Duration::from_secs(2)); // parking works a lot better as a symaphore to know there's data
        }
    });
    println!("done");

    // lazy initialization
    let y = get_x();
    let z = get_x(); // this call will take a lot less time
    println!("y {y}, z {z}");
}

// computes X once
// two threads could have a race withing the func
// but data will stay consistent anyways
fn get_x() -> u64 {
    static X: AtomicU64 = AtomicU64::new(0); // let's say an actual X is never 0
    let mut x = X.load(Relaxed);
    if x == 0 {
        x = 42; // let's say there's a long complex process to get this
        X.store(x, Relaxed);
    }
    x
}
