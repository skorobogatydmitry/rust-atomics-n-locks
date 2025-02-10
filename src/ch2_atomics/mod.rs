use std::{
    sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering::Relaxed},
    thread::{self, scope, sleep, spawn},
    time::{Duration, Instant},
};

pub fn run() {
    // atomic types live in std::sync::atomic
    // they're are all internally mutable
    // all atomics have memory ordering arg, it'll be reviewed in chapter 3

    atomic_load_and_store_operations();
    fetch_and_modify_operations();
}

fn atomic_load_and_store_operations() {
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
        x = 42; // let's say there's a long complex process to do this
        X.store(x, Relaxed);
    }
    x
}

fn fetch_and_modify_operations() {
    // modification varies from type to type:
    // fetch_add, fetch_sub, fetch_or, fetch_and, etc

    // simple fetch_add example
    let a = AtomicI32::new(100);
    let b = a.fetch_add(23, Relaxed);
    let c = a.load(Relaxed);
    assert_eq!(b, 100);
    assert_eq!(c, 123);

    // fetch_add and fetch_sub don't check for overflows
    a.fetch_add(std::i32::MAX, Relaxed);
    a.fetch_add(std::i32::MAX - 8, Relaxed);
    assert_eq!(113, a.load(Relaxed));

    // progress report enhanced
    let progress = &AtomicUsize::new(0);
    scope(|s| {
        // 4 threads, 25 items each
        for t in 0..4 {
            s.spawn(move || {
                // move t to the thread + progress (it's a reference)
                for i in 0..25 {
                    let _x = t * 10 + i; // fake the processing
                    sleep(Duration::from_millis(100));
                    progress.fetch_add(1, Relaxed);
                }
            });
        }

        // post status update every second
        loop {
            let n = progress.load(Relaxed);
            if n == 100 {
                break;
            }
            println!("done: {n}");
            sleep(Duration::from_secs(1));
        }
        println!("done..");
    });

    // statistics
    let num_done = &AtomicUsize::new(0);
    let total_time = &AtomicU64::new(0);
    let max_time = &AtomicU64::new(0);

    scope(|s| {
        for t in 0..4 {
            s.spawn(move || {
                for i in 0..25 {
                    let start = Instant::now();
                    let _x = t * 10 + i; // fake the processing
                    sleep(Duration::from_millis(254));
                    let time_taken = start.elapsed().as_micros() as u64; // Instant::now() - start;

                    // the 3 updates aren't atomic as a whole => could be in 1 mutex
                    num_done.fetch_add(1, Relaxed);
                    total_time.fetch_add(time_taken, Relaxed);
                    max_time.fetch_max(time_taken, Relaxed);
                }
            });
        }

        loop {
            let total_time = Duration::from_micros(total_time.load(Relaxed));
            let max_time = Duration::from_micros(max_time.load(Relaxed));
            let n = num_done.load(Relaxed);

            if n == 100 {
                break;
            }
            if n == 0 {
                println!("nothing processed yet");
            } else {
                println!(
                    "in progress: {n}/100 done, {:?} avg, {:?} peak",
                    total_time / n as u32,
                    max_time
                );
            }
            sleep(Duration::from_secs(1));
        }
        println!("done..");
    });

    // example - ID allocation
    for _ in 0..100_000 {
        spawn(|| {
            sleep(Duration::from_millis(100));
            let _id = allocate_new_id();
        });
    }

    loop {
        let curr_id = allocate_new_id();
        println!("id is {curr_id}");
        if curr_id >= 100000 {
            break;
        }
        sleep(Duration::from_secs(1));
    }
}

fn allocate_new_id() -> u32 {
    static NEXT_ID: AtomicU32 = AtomicU32::new(0);
    // overflow-prone
    NEXT_ID.fetch_add(1, Relaxed)
    // std::process::abort could initiate exitting if an overflow is detected
    // fetch_sub + abort could keep the counter roughly the same
}
