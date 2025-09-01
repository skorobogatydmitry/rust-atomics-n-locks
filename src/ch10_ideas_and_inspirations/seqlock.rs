use std::{
    cell::UnsafeCell,
    hint,
    sync::atomic::{
        AtomicU32,
        Ordering::{Acquire, Relaxed, Release},
    },
};

struct SeqLock<Y> {
    counter: AtomicU32, // even - safe to read, odd - is changing
    data: UnsafeCell<Y>,
}

unsafe impl<Y> Sync for SeqLock<Y> where Y: Send + Sync {}

impl<Y> SeqLock<Y> {
    fn new(data: Y) -> Self {
        Self {
            counter: AtomicU32::new(0),
            data: UnsafeCell::new(data),
        }
    }

    /// "locks" the value and executes the supplied Fn on it  
    /// it returns None in all cases when the value isn't accessible for writes
    fn write<U, F>(&self, func: F) -> Option<U>
    where
        F: FnOnce(&mut Y) -> U,
    {
        let val = self.counter.load(Relaxed);
        if val % 2 == 0 {
            // Acquire is needed to have all variables of FnOnce consistent with the counter and the data
            self.counter
                .compare_exchange(val, val + 1, Acquire, Relaxed)
                .ok()
                .map(|_| {
                    let result = func(unsafe { &mut *self.data.get() });
                    self.counter.store(val + 2, Release); // make sure all operations performed in fn are aligned
                    result
                })
        } else {
            None
        }
    }

    /// applies func to the data and checks whether the data was consistent during the operation
    fn read<U, F>(&self, func: F) -> Option<U>
    where
        F: FnOnce(&Y) -> U,
    {
        let before = self.counter.load(Relaxed);
        if before % 2 == 1 {
            return None;
        }

        let result = func(unsafe { &*self.data.get() });
        let after = self.counter.load(Acquire); // pickup all changes from the `func`

        (before == after).then_some(result)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::{
        thread::{scope, sleep},
        time::Duration,
    };

    /// 2 threads add 1 to the seqlock 10 times each with delay between the ops
    /// practice shows that the threads race for operations in this test,
    /// that's why not all 20 operations pass
    /// so the test checks the final value with the counter
    #[test]
    fn test_multiple_writers() {
        let counter = AtomicU32::new(0);
        let sl = SeqLock::new(10_u32);
        scope(|s| {
            s.spawn(|| {
                for _i in 0..10 {
                    sleep(Duration::from_millis(10));
                    if sl
                        .write(|val: &mut u32| {
                            // println!("t1 {_i} | val is {val}");
                            *val += 1;
                        })
                        .is_some()
                    {
                        counter.fetch_add(1, Relaxed);
                    }
                }
            });

            s.spawn(|| {
                for _i in 0..10 {
                    sleep(Duration::from_millis(10));
                    if sl
                        .write(|val: &mut u32| {
                            // println!("t2 {_i} | val is {val}");
                            *val += 1;
                        })
                        .is_some()
                    {
                        counter.fetch_add(1, Relaxed);
                    }
                }
            });
        });

        assert!(sl
            .read(|data| assert_eq!(counter.load(Acquire) + 10, *data))
            .is_some());
    }

    /// check that a writer blocks all the readers:
    /// - 1 writer hangs for 100 ms
    /// - another thread tries to read the same data 10 times - is expected to fail
    /// - wait for the writer to unlock then read the value successfully
    #[test]
    fn test_writer_block_readers() {
        let sl = SeqLock::new(0_u32);
        scope(|s| {
            s.spawn(|| {
                sl.write(|data: &mut u32| {
                    *data += 1;
                    sleep(Duration::from_millis(200));
                })
            });

            s.spawn(|| {
                for _ in 0..10 {
                    assert!(sl.read(|data| std::hint::black_box(data + 10)).is_none());
                }
            });

            sleep(Duration::from_millis(200));
            s.spawn(|| {
                for _ in 0..10 {
                    assert!(sl.read(|data| assert_eq!(*data, 1)).is_some());
                }
            });
        });
    }
}
