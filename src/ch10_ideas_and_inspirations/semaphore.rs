use std::sync::atomic::{
    fence, AtomicU32,
    Ordering::{Acquire, Relaxed, Release},
};

use atomic_wait::{wait, wake_one};

#[allow(unused)]
pub struct Semaphore {
    counter: AtomicU32,
    limit: u32,
}

#[allow(unused)]
impl Semaphore {
    pub fn new(limit: u32) -> Self {
        if limit == 0 {
            panic!("semaphore with 0 as a treshold isn't usable");
        }
        Self {
            counter: AtomicU32::new(0),
            limit,
        }
    }

    /// decrement the counter or wait if it's 0  
    /// Acquire context from all the [Semaphore::up] callers on the other side
    pub fn down(&self) {
        let mut s = self.counter.load(Relaxed);
        loop {
            if s == 0 {
                wait(&self.counter, 0);
                s = self.counter.load(Relaxed);
            } else {
                match self
                    .counter
                    .compare_exchange_weak(s, s - 1, Acquire, Relaxed)
                {
                    Ok(_) => return,
                    Err(e) => s = e,
                }
            }
        }
    }

    /// check the value for the limit then increment if it's not there yet
    pub fn up(&self) -> u32 {
        let mut s = self.counter.load(Relaxed);
        loop {
            if s >= self.limit {
                // guarantee that, despite that there's nobody to wake up,
                // we release the outer contex for the possible consumers
                fence(Release);
                return s;
            } else {
                // Release ordering for success: the other side may've been waiting for our data
                match self.counter.compare_exchange(s, s + 1, Release, Relaxed) {
                    Ok(os) => {
                        // the only case when we may need to wake smb up is if the original value was 0
                        if os == 0 {
                            wake_one(&self.counter);
                        }
                        return s + 1;
                    }
                    Err(e) => s = e,
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::{
        sync::atomic::Ordering::Relaxed,
        thread::{scope, sleep},
        time::Duration,
    };

    use super::*;

    #[test]
    fn test_semaphore() {
        let sph = Semaphore::new(5);
        assert_eq!(sph.counter.load(Relaxed), 0);

        scope(|s| {
            for _ in 0..10 {
                s.spawn(|| {
                    sph.down();
                });
            }
            assert_eq!(sph.counter.load(Relaxed), 0);

            for _ in 0..10 {
                assert_eq!(1, s.spawn(|| sph.up()).join().unwrap());
            }

            // wait for the above to soak
            sleep(Duration::from_millis(100));
            assert_eq!(sph.counter.load(Relaxed), 0);

            for i in 0..10 {
                assert_eq!(
                    std::cmp::min(5, i + 1),
                    s.spawn(|| sph.up()).join().unwrap()
                );
            }
            // no waits or checks - let 'em race a bit
            for _ in 0..5 {
                s.spawn(|| {
                    sph.down();
                });
            }

            // wait for the above to soak
            sleep(Duration::from_millis(100));

            assert_eq!(sph.counter.load(Relaxed), 0);
        });
    }

    #[test]
    fn test_semaphore_limit() {
        let sph = Semaphore::new(2);
        assert_eq!(0, sph.counter.load(Relaxed));
        sph.up();
        assert_eq!(1, sph.counter.load(Relaxed));
        sph.up();
        sph.up();
        assert_eq!(2, sph.counter.load(Relaxed));
        sph.down();
        assert_eq!(1, sph.counter.load(Relaxed));
        sph.up();
        sph.up();
        assert_eq!(2, sph.counter.load(Relaxed));
    }

    #[test]
    #[should_panic(expected = "semaphore with 0 as a treshold isn't usable")]
    fn test_new_panics() {
        Semaphore::new(0);
    }
}
