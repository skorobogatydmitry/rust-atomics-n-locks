use std::{
    sync::atomic::{AtomicU32, Ordering::Relaxed},
    thread::scope,
};

// compare & exchange is the most advanced and flexible
// it takes an expected value and returs Result whether the value was replaced or not
//
// one of the use-cases is to make sure value stayed untouched in-between it was loaded and stored
pub fn run() {
    let x = &AtomicU32::new(0);
    increment(x);
    increment(x);
    assert_eq!(2, x.load(Relaxed));
    println!("x is {x:?}");

    id_allocation_without_overflow();
    lazy_onetime_initialization();
}

// it's a fetch_add, but using compare_replace
fn increment(a: &AtomicU32) {
    let mut current = a.load(Relaxed);
    // a loop of waiting for success
    loop {
        current += 1; // thread-unsafe code

        // (!) it's a place for ABA problem: a could change to +1 then -1 right here
        // (!) the problem doesn't affect this exact code, but may be relevant for other places

        // there's a compare_exchange_weak, which should be preferred sometimes, see chapter 7
        match a.compare_exchange(current - 1, current, Relaxed, Relaxed) {
            Ok(_) => return,       // the value didn't change in-between
            Err(v) => current = v, // another attempt with the new value
        }
    }
}

fn id_allocation_without_overflow() {
    scope(|s| {
        for _ in 0..15 {
            s.spawn(|| println!("my id is {}", allocate_new_id()));
        }
    });
}

fn allocate_new_id() -> u32 {
    static NEXT_ID: AtomicU32 = AtomicU32::new(0);
    let mut id = NEXT_ID.load(Relaxed);
    loop {
        // assertions now goes before manipulations
        // => no overflows possible
        assert!(id < u32::MAX, "too many IDs!");
        match NEXT_ID.compare_exchange_weak(id, id + 1, Relaxed, Relaxed) {
            Ok(_) => return id,
            Err(v) => id = v,
        }
    }

    // there's also a fetch_update which makes be above match block just a oneliner:
    // NEXT_ID.fetch_update(Relaxed, Relaxed, |n| {
    //     n.checked_add(1).expect("too many IDs!")
    // })
}

fn lazy_onetime_initialization() {
    scope(|s| {
        for _ in 0..15 {
            // (!) it possibly generates lots of throwaway keys internally
            // => it's better to block threads during the initial generation
            // to make sure the generation happens only once: std::sync::Once and std::sync::OnceLock
            s.spawn(|| println!("our key is {}", get_key()));
        }
    });
}

// expected to return the same number withing one "round"
// yet change it every program run
// => all functions should receive the same "encryption salt"
fn get_key() -> u32 {
    static KEY: AtomicU32 = AtomicU32::new(0);
    let key = KEY.load(Relaxed);
    if key == 0 {
        let new_key = 12345; // generate_secure_key();
        match KEY.compare_exchange(0, new_key, Relaxed, Relaxed) {
            Ok(_) => new_key, // our key was set
            Err(k) => k,      // someone else's key
        }
    } else {
        key // someone set it "long" ago
    }
}
