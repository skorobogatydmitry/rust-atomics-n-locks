use std::{
    sync::atomic::{AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering::Relaxed},
    thread::{scope, sleep, spawn},
    time::{Duration, Instant},
};

pub fn run() {
    // modification varies from type to type:
    // fetch_add, fetch_sub, fetch_or, fetch_and, etc

    // a simple fetch_add example
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

                    // the 3 updates aren't atomic as a whole => could be in 1 mutex instead
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
