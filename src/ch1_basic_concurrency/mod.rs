use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    marker::PhantomData,
    rc::Rc,
    sync::{Arc, Condvar, Mutex, RwLock},
    thread::{self, scope, sleep},
    time::Duration,
};

pub fn run() {
    spawn_threads();
    scoped_threads();
    shared_ownership_n_ref_cnt();
    borrowin_and_data_races();
    interior_mutability();
    thread_safety_send_and_sync();
    locking__mutexes_and_rwlocks();
    waiting__parking_and_condition_variables();
}

fn spawn_threads() {
    // spawn 2 threads
    let t1 = thread::spawn(f);
    let t2 = thread::spawn(f);
    // println! macro is sync - it locks std::io::Stdout
    println!("hello from the main thread!");
    // wait for both threads to finish
    t1.join().unwrap();
    t2.join().unwrap();

    // moving values to a thread
    let numbers = vec![1, 2, 3];
    thread::spawn(move || {
        // spawn has 'static lifetime bound => requires all borrows by ref to have 'static too
        for n in numbers {
            println!("another one {n}")
        }
    })
    .join()
    .unwrap();

    // return values from threads
    let numbers = Vec::from_iter(0..=1000);
    let avg = thread::spawn(move || {
        let len = numbers.len();
        let sum = numbers.into_iter().sum::<usize>();
        sum / len
    })
    .join()
    .unwrap();
    println!("avg is {avg}");

    thread::Builder::new() // new thread builded
        .name("mythread".to_string()) // will be in panic messages, could be added to logs
        .stack_size(100) // could be useful too
        .spawn(|| println!("i am a thread too!")) // with this payload
        .unwrap() // expect to spawn successfully
        .join() // wait for completion
        .unwrap(); // expect to execute sucessfully
}
fn f() {
    println!("from another thread");
    let t_id = thread::current().id();
    println!("thread id is {t_id:?}");
}

fn scoped_threads() {
    #[allow(unused_mut)]
    let mut numbers = vec![1, 2, 3];

    // allow to borrow local variables by reference
    // guarantees that threads don't outlive the scope
    thread::scope(|s| {
        // start two new threads in the scope
        s.spawn(|| println!("len is {}", numbers.len()));
        s.spawn(|| {
            // note borrow by ref
            for n in &numbers {
                println!("{n}");
            }
        });
        // produces an error - can't borrow as mutable
        // s.spawn(|| {
        //     numbers.push(1);
        // });
        //
    }); // automatic join happens here <---
}

fn shared_ownership_n_ref_cnt() {
    // static data can be shared between threads
    #[allow(non_upper_case_globals)]
    static static_x: [i32; 3] = [1, 2, 3];

    let j1 = thread::spawn(|| dbg!(&static_x));
    let j2 = thread::spawn(|| dbg!(&static_x));
    j1.join().unwrap();
    j2.join().unwrap();

    // leaking allocation
    // &'static means a static reference, returned by the Box:::leak
    // 'static === "exists from now onwards"
    let leaked_x: &'static [i32; 3] = Box::leak(Box::new([1, 2, 3]));
    // note the `move' moves a reference, which gets Copy-ed after its impl Copy
    let j1 = thread::spawn(move || dbg!(leaked_x));
    let j2 = thread::spawn(move || dbg!(leaked_x));
    j1.join().unwrap();
    j2.join().unwrap();
    // leaked_x leaks at the end of the function

    // reference counting
    // non-thread-safe
    let a = Rc::new([1, 2, 3]);
    let b = a.clone(); // Rc::clone(a) idiomatically
    assert_eq!(a.as_ptr(), b.as_ptr()); // => same allocation

    // 686 |     T: Send + 'static,
    //     |        ^^^^ required by this bound in `spawn`
    // thread::spawn(|| dbg!(b));
    let a = Arc::new([1, 2, 3]);
    let b = a.clone();

    let j1 = thread::spawn(move || {
        dbg!(Arc::strong_count(&a)); // it's sometimes 1 => j2 finished
        dbg!(a);
    });
    let j2 = thread::spawn(move || {
        dbg!(Arc::strong_count(&b));
        dbg!(b);
    });
    j1.join().unwrap();
    j2.join().unwrap();
}

fn borrowin_and_data_races() {
    // there are 2 borrowing ways:
    // - immutable - `let ref = &some;` - could be multiple
    // - mutable - `let ref = &mut some;` - could be only one

    let mut a = 10;
    // f2(&a, &mut a); - isn't allowed to borrow the same
    let b = 20;
    f2(&b, &mut a); // just fine
}
fn f2(x: &i32, y: &mut i32) {
    let before = *x;
    *y += 1;
    let after = *x;
    if before != after {
        panic!("somethig changed!"); // could never happen
    }
}

fn interior_mutability() {
    // some "immutable" refs mutate during lifecycle =>
    // it's more appropriate to call them "shared" and "exclusive"

    // Cell - stores a value, allow to Copy or replace it, non-thread-safe
    let c = Cell::new(10);
    f3(&c, &c); // a gets changed
    assert_eq!(11, c.get());

    // it's hard to work with
    let vc = Cell::new(vec![1, 2, 3]);
    let mut v = vc.take(); // Cell::get returns a copy
    v.push(4);
    vc.set(v);

    // RefCell - stores a value, allows to borrow it once withouut copying
    //           second borrow causes panic
    //           non-thread-safe
    let rcv = RefCell::new(vc.take());
    println!("{:?}", rcv.borrow());
    {
        // v stays valid till the end of the scope
        let mut v = rcv.borrow_mut();
        v.push(5);
    }
    rcv.borrow_mut().push(6);
    println!("{:?}", rcv.borrow());

    // RwLock - a thread-safe RefCell which waits for the other borrows to disappear
    let rwlv = RwLock::new(rcv.take());
    scope(|s| {
        let _j = s.spawn(|| {
            // borrowing is called "locking"
            // it arrives here first
            let _v = rwlv.write();
            sleep(Duration::from_millis(10));
            println!("vec has changed");
        });
    });
    // this code, most likely, waits for the thread to drop the borrow
    println!("vec is {:?}", rwlv.read());

    // Mutex - same as RwLock, but doesn't allow shared borrows

    // Atomics - a group of thread-safe Cell-s
    //           chapters 2 and 3
    //           not generic (AtomicU32, AtomicPtr, etc)
    //           platform-specific
    //           have static size
    //           usually used to share some metadata about actual data state

    // UnsafeCell - a prmitive for interior mutability in Cell, Mutex, etc
    //              .get() returns raw pointer => unsafe block is needed
}

fn f3(a: &Cell<i32>, b: &Cell<i32>) {
    let before = a.get();
    b.set(b.get() + 1); // mutate internal value here
    let after = a.get();
    assert_eq!(before + 1, after);
}

fn thread_safety_send_and_sync() {
    // Send - trait, means a var of the type can be send to another thread
    //        Arc<i32> could be transferred to another thread
    // Sync - trait, means a var of the type can be shared with another thread
    //        i32 is Sync (&i32 is Send)
    // all primitive types are Send and Sync

    let x = X {
        _handle: 10,
        _not_sync: PhantomData::default(),
    };

    // can't Sync
    // thread::spawn(|| println!("x is {:?}", x)).join().unwrap();

    // can Send
    thread::spawn(move || println!("x is {:?}", x))
        .join()
        .unwrap();

    let mut a = 10;
    let y = Y { _p: &mut a };
    // we declared them so...
    thread::spawn(move || println!("y is {:?}", y))
        .join()
        .unwrap();
}

#[derive(Debug)]
struct X {
    _handle: i32,                     // it's  Send and Sync => makes X Send & Sync
    _not_sync: PhantomData<Cell<()>>, // a special marker to make it non-Sync, as Cell is not Sync
}

#[derive(Debug)]
struct Y {
    _p: *mut i32, // raw pointer => nor Send neither Sync ...
}

// ... but can make 'em
unsafe impl Send for Y {}
unsafe impl Sync for Y {}

#[allow(non_snake_case)]
fn locking__mutexes_and_rwlocks() {
    // Mutex - ensure exclusive thread access to mutable data
    //         lock is a blocking action
    let m = Mutex::new(3);
    // lock makes a guard
    {
        let mut mguard = m.lock().unwrap();
        *mguard += 1; // mutex guard implements DerefMut to ease access to the data
    } // the guard goes out of the scope here => mutex unlocks
    assert_eq!(true, m.try_lock().is_ok());
    assert_eq!(4, m.into_inner().unwrap());

    // example for Mutex with threads
    let n = Mutex::new(0);
    scope(|s| {
        for _ in 0..10 {
            s.spawn(|| {
                let mut guard = n.lock().unwrap();

                for _ in 0..100 {
                    *guard += 1;
                }
                println!("n is {guard}");
                // drop(guard); // uncomment to drop mutex without delay
                sleep(Duration::from_millis(100));
            });
        }
    });
    assert_eq!(1000, n.into_inner().unwrap());

    // a mutex can be poisoned => .lock() may not succeed
    // poisoning caused by that a thread holding the lock panicked

    let m2 = Mutex::new(10);
    scope(|s| {
        let jh = s.spawn(|| {
            // this thread poisons the mutex
            let _g = m2.lock();
            panic!("poisoning the lock");
        });
        sleep(Duration::from_millis(100));
        // mutex can still be locked
        let guard = m2.lock();
        assert_eq!(true, guard.is_err()); // but it returns an err
        assert_eq!(10, *guard.unwrap_err().into_inner());
        let _ = jh.join(); // ignore panicked scoped thread
    });

    // RwLock - same as Mutex, but behaves like a borrow checker
    //          can be taken by 1 writer OR N readers at a time
    let rwl = RwLock::new(10);
    scope(|s| {
        s.spawn(|| {
            let mut rw = rwl.write().unwrap();
            println!("rwl has {}", *rw);
            *rw += 1;
        });
        s.spawn(|| {
            sleep(Duration::from_millis(200));
            println!("getting another ro lock");
            let _ro = rwl.read();
            println!("have an ro lock, dropping");
        });
        sleep(Duration::from_millis(100));
        // the below will wait for the thread's writer to drop access
        println!("getting an ro lock");
        let ro = rwl.read().unwrap();
        println!("rwl has {}", *ro);
    });

    // Mutex requires Send, RwLock - Send + Sync
}

#[allow(non_snake_case)]
fn waiting__parking_and_condition_variables() {
    // threads may want to wait for events

    // parking - thread can park itself and wait to be awaken
    let queue = Mutex::new(VecDeque::new());

    scope(|s| {
        let consumer = s.spawn(|| {
            let mut parks = 0;
            loop {
                let item = queue.lock().unwrap().pop_front();
                if let Some(item) = item {
                    dbg!(item);
                } else {
                    println!("no items, parking...");
                    // park current thread if there are no items
                    thread::park(); // thread actually waits here
                    parks += 1;
                    if parks >= 3 {
                        println!("parked 3+ times, I'm done");
                        break;
                    }
                }
            }
        });

        consumer.thread().unpark(); // a single unpark operation always gets remebered and executed on the next park
                                    // 2 unparks have the same effect as one - there's no queue
        thread::sleep(Duration::from_millis(1000));

        // produce numbers
        for i in 0..30 {
            queue.lock().unwrap().push_back(i);
            if (i + 1) % 10 == 0 {
                // let the consumer know there's new data
                println!("unparking on {i}");
                consumer.thread().unpark(); // unpark operations gets stored, remebered and executed!
                thread::sleep(Duration::from_millis(1000));
            }
        }
        consumer.join().unwrap();
    });

    // Condition variables - more common than parking
    //                       serves the same use-case
    //                       many threads could wait on one variable
    //                       all or 1 thread could be notified
    //                       it releases and re-locks the provided Mutex using its MutexGuard
    //                       !!! can wait on the same mutex only

    queue.lock().unwrap().clear();
    let not_empty = Condvar::new();

    println!("condvar example");
    scope(|s| {
        let consumer = s.spawn(|| {
            let mut iterations = 0;
            loop {
                let mut q = queue.lock().unwrap();
                let item = loop {
                    if let Some(item) = q.pop_front() {
                        break item; // retur the item
                    } else {
                        // wait on the queue and refresh it!
                        println!("waiting for items");
                        q = not_empty.wait(q).unwrap();
                    }
                };
                drop(q); // release the lock
                dbg!(item);
                iterations += 1;
                if iterations == 29 {
                    println!("i'm done");
                    break;
                }
            }
        });

        for i in 0..30 {
            queue.lock().unwrap().push_back(i);
            if (i + 1) % 10 == 0 {
                println!("letting the consumer know");
                not_empty.notify_one(); // let the thread know
            }
            thread::sleep(Duration::from_millis(100));
        }

        consumer.join().unwrap();
    });
}
