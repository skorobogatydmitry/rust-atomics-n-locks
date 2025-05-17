/*
 * # Circular dependencies
 * What if we have an Arc contains another Arc that contains the former?
 * In this case, the pair will never be dropped unless directed to.
 * Weak pointer is a solutions. They don't count as references when `Drop'ping the data behind it.
 * Only Arc contributes to the reference count.
 * Weak pointer doesn't provide &T if Arc is gone.
 *
 * Weak point could be upgraded to an Arc.
 *
 * But they do prevent ArcData deallocation to avoid UB in other operations.
 * It means there are 2 inner counters: for Y and for ArcData<Y>
 */

use std::{
    cell::UnsafeCell,
    ops::Deref,
    ptr::NonNull,
    sync::atomic::{
        fence, AtomicUsize,
        Ordering::{Acquire, Relaxed, Release},
    },
};

struct ArcData<Y> {
    /// number of Arc's
    data_ref_cnt: AtomicUsize,
    /// number of Arc's and Weak's
    alloc_ref_cnt: AtomicUsize,
    /// the data, None for dropped Arc's
    data: UnsafeCell<Option<Y>>,
}

/// Arc is effectively Weak with some extra
pub struct Arc<Y> {
    weak: Weak<Y>,
}

pub struct Weak<Y> {
    ptr: NonNull<ArcData<Y>>, // it also has ArcData
}

unsafe impl<Y: Send + Sync> Send for Weak<Y> {}
unsafe impl<Y: Send + Sync> Sync for Weak<Y> {}

impl<Y> Arc<Y> {
    /// same as in basic.rs, but with 1 more counter
    pub fn new(data: Y) -> Self {
        Self {
            weak: Weak {
                ptr: NonNull::from(Box::leak(Box::new(ArcData {
                    data_ref_cnt: AtomicUsize::new(1),
                    alloc_ref_cnt: AtomicUsize::new(1),
                    data: UnsafeCell::new(Some(data)),
                }))),
            },
        }
    }

    /// almost the same, just take another counter into account
    pub fn get_mut(arc: &mut Self) -> Option<&mut Y> {
        if arc.weak.data().alloc_ref_cnt.load(Relaxed) == 1 {
            fence(Acquire);
            // SAFETY: no one else can access the data:
            // (1) we own &mut on Arc
            // (2) there's no Weak (only 1 ref total)
            let arcdata = unsafe { arc.weak.ptr.as_mut() };
            let option = arcdata.data.get_mut();
            // there's one reference to the data, it's Arc (us) => safe to unwrap
            let data = option.as_mut().unwrap();
            Some(data)
        } else {
            None
        }
    }

    /// downgrading is a simple op
    pub fn downgrade(&self) -> Weak<Y> {
        self.weak.clone()
    }
}

impl<Y> Weak<Y> {
    fn data(&self) -> &ArcData<Y> {
        // SAFETY: there's always a valid (but possibly empty) ArcData
        unsafe { self.ptr.as_ref() }
    }

    /// Upgrading is only possible if there's >0 Arc's around
    pub fn upgrade(&self) -> Option<Arc<Y>> {
        let mut n = self.data().data_ref_cnt.load(Relaxed);
        loop {
            if n == 0 {
                return None;
            }
            assert!(n < usize::MAX);
            if let Err(e) = self
                .data()
                .data_ref_cnt // increment the Arc's counter
                .compare_exchange(n, n + 1, Relaxed, Relaxed)
            {
                n = e;
                continue;
            }
            // .clone increments the Weak's counter
            return Some(Arc { weak: self.clone() });
        }
    }
}

impl<Y> Deref for Arc<Y> {
    type Target = Y;
    fn deref(&self) -> &Y {
        let ptr = self.weak.data().data.get();
        // SAFETY: since there's an &Arc, data is there and can be shared
        unsafe { (*ptr).as_ref().unwrap() }
    }
}

/// similar to Arc's Clone from basic.rs
impl<Y> Clone for Weak<Y> {
    fn clone(&self) -> Self {
        // advances only allocation, not data (Arc's business)
        if self.data().alloc_ref_cnt.fetch_add(1, Relaxed) > usize::MAX / 2 {
            std::process::abort();
        }
        Weak { ptr: self.ptr }
    }
}

/// increment another counter on the top of Weak's impl
impl<Y> Clone for Arc<Y> {
    fn clone(&self) -> Self {
        let weak = self.weak.clone();
        if weak.data().data_ref_cnt.fetch_add(1, Relaxed) > usize::MAX / 2 {
            std::process::abort();
        }
        Arc { weak }
    }
}

/// Dropping the Weak similar to the basic.rs -s Arc
/// It drops the whole thing if there are no references at all
impl<Y> Drop for Weak<Y> {
    fn drop(&mut self) {
        if self.data().alloc_ref_cnt.fetch_sub(1, Release) == 1 {
            fence(Acquire);
            unsafe {
                drop(Box::from_raw(self.ptr.as_ptr()));
            }
        }
    }
}

/// Just drops the data, weak gets dropped as one of the self's fields
impl<Y> Drop for Arc<Y> {
    fn drop(&mut self) {
        if self.weak.data().data_ref_cnt.fetch_sub(1, Release) == 1 {
            fence(Acquire);
            // acquire mutable pointer to the underlying value
            let ptr = self.weak.data().data.get();
            // SAFETY: data ref counter is 0, no one have access
            unsafe {
                (*ptr) = None; // forget the Some(...) => it gets dropped
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering::Relaxed;

    use super::*;

    static NUM_DROPS: AtomicUsize = AtomicUsize::new(0);

    struct DetectDrop;

    impl Drop for DetectDrop {
        fn drop(&mut self) {
            NUM_DROPS.fetch_add(1, Relaxed);
        }
    }

    #[test]
    fn test() {
        // an Arc and two weak pointers
        let x = Arc::new(("hello", DetectDrop));
        let w1 = x.downgrade();
        let w2 = x.downgrade();

        let jh = std::thread::spawn(move || {
            // sould be upgradable here
            let s1 = w1.upgrade().unwrap();
            assert_eq!(s1.0, "hello");
        });

        assert_eq!(x.0, "hello");
        jh.join().unwrap();

        // the data isn't dropped yet - there's w2
        assert_eq!(NUM_DROPS.load(Relaxed), 0);
        // there's an Arc around to upgrade - x
        assert!(w2.upgrade().is_some());

        drop(x);

        // now the data is dropped
        assert_eq!(NUM_DROPS.load(Relaxed), 1);
        // and upgrade doesn't happen
        assert!(w2.upgrade().is_none());
    }
}
