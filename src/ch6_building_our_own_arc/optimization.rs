use super::weak::Arc as WeakArc;
use std::{
    cell::UnsafeCell,
    mem::ManuallyDrop,
    ops::Deref,
    ptr::NonNull,
    sync::atomic::{
        fence, AtomicUsize,
        Ordering::{Acquire, Relaxed, Release},
    },
};

/* All Arc's in the weak.rs keep 2 counters up2date for clonning and dropping.
 * It's suboptiomal. Also, it's hard to keep separated counters for Arc and Weak, as ...
 *
 * Let's say we have a fn that bounces an Arc and Weak
 */
fn annoying(mut arc: WeakArc<i32>) {
    // let's say it's infinite
    for _ in 0..10 {
        let weak = WeakArc::downgrade(&arc); // make weak
        drop(arc);
        println!("no arc at this point"); // <-- a moment in time when the thread have no Arc, but about to make it
        arc = weak.upgrade().unwrap(); // restore arc
        drop(weak);
        println!("no weak at this point");
    }
}

#[cfg(test)]
mod test {
    use crate::ch6_building_our_own_arc::weak::Arc as WeakArc;

    use super::annoying;

    #[test]
    fn test_annoying() {
        let x = WeakArc::new(0);
        let y = x.clone();
        std::thread::spawn(move || annoying(x));
        drop(y); // it could actually drop the data or not, depending on what happens in the thread
    }

    // copy-paste from weak.rs
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
        let w1 = Arc::downgrade(&x);
        let w2 = Arc::downgrade(&x);

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

/*
 * An approach: count all Arc's as 1 weak pointer, not N => alloc_ref_count is always >= 1
 */

pub struct Arc<Y> {
    ptr: NonNull<ArcData<Y>>, // can't re-use Weak due to ... ???
}

unsafe impl<Y: Send + Sync> Send for Arc<Y> {}
unsafe impl<Y: Send + Sync> Sync for Arc<Y> {}

pub struct Weak<Y> {
    ptr: NonNull<ArcData<Y>>,
}

unsafe impl<Y: Send + Sync> Send for Weak<Y> {}
unsafe impl<Y: Send + Sync> Sync for Weak<Y> {}

// there's a special type - ManuallyDrop<Y>, which is used instead of Option<Y> in our case to save some space
struct ArcData<Y> {
    /// Number of `Arc`'s
    data_ref_cnt: AtomicUsize,
    /// Number of `Weak`'s, plus one if there're any `Arc`s
    alloc_ref_cnt: AtomicUsize,
    /// actual data
    data: UnsafeCell<ManuallyDrop<Y>>,
}

impl<Y> Arc<Y> {
    /// same as in weak.rs, but with ManuallyDrop instead of Some
    pub fn new(data: Y) -> Self {
        Self {
            ptr: NonNull::from(Box::leak(Box::new(ArcData {
                data_ref_cnt: AtomicUsize::new(1),
                alloc_ref_cnt: AtomicUsize::new(1),
                data: UnsafeCell::new(ManuallyDrop::new(data)),
            }))),
        }
    }

    // now we don't have Weak behind to reuse code
    fn data(&self) -> &ArcData<Y> {
        unsafe { self.ptr.as_ref() }
    }

    /// have to check 2 counters atomically (or so) to avoid count skews due to upgrade / downgrade / clone skews
    pub fn get_mut(arc: &mut Self) -> Option<&mut Y> {
        // trying to "lock" the alloc_ref_cnt briefly
        // Acquire matches Weak::drop decrement => if there're no Weak pointers ATM, we see all upgrades to Arc's happened-before the Drop
        // => no pointers are left uncounted
        // otherwise, alloc_ref_cnt can be 1 (no weak pointers),
        // but data_ref_cnt may be still low and doesn't count an upgrade happened before the Weak's Drop in another thread
        if arc
            .data()
            .alloc_ref_cnt // it can't become lower, as only Arc may exist (alloc_ref_cnt == 1), but can become higher => see downgrade
            .compare_exchange_weak(1, usize::MAX, Acquire, Relaxed)
            .is_err()
        {
            return None;
        }
        let is_unique = arc.data().data_ref_cnt.load(Relaxed) == 1;
        // Release matches Acquire increment in `downgrade`, to make sure any
        // changes to the data_ref_cnt that come after `downgrade` don't
        // change the is_unique result above.
        // In other words, it couples the 'Arc::downgrade -> Arc::drop' chain in another thread
        // so we can't see already dropped Arc (is_unique == true) but no weak pointer made before (alloc_ref_cnt += 1)
        arc.data().alloc_ref_cnt.store(1, Release);
        if !is_unique {
            return None;
        }
        // Acquire to match `Arc::drop`'s Release decrement, to make sure
        // nothing else is accessing the data => the Arc's are actually dropped or is_unique == false.
        fence(Acquire);
        unsafe { Some(&mut *arc.data().data.get()) }
    }

    /// it should check for special value usize::MAX and spinloop until it returns to smth normal
    pub fn downgrade(arc: &Self) -> Weak<Y> {
        let mut n = arc.data().alloc_ref_cnt.load(Relaxed);
        loop {
            if n == usize::MAX {
                // wait for the gete_mut to finish if we hit the spot
                std::hint::spin_loop();
                n = arc.data().alloc_ref_cnt.load(Relaxed);
                continue;
            }
            assert!(n < usize::MAX - 1);
            // Acquire synchronises with get_mut's release-store
            if let Err(e) =
                arc.data()
                    .alloc_ref_cnt
                    .compare_exchange_weak(n, n + 1, Acquire, Relaxed)
            {
                n = e;
                continue;
            }
            return Weak { ptr: arc.ptr };
        }
    }
}

impl<Y> Weak<Y> {
    // now we don't have Weak behind to reuse code
    fn data(&self) -> &ArcData<Y> {
        unsafe { self.ptr.as_ref() }
    }

    /// very similar, but no `Weak::clone` involved
    pub fn upgrade(&self) -> Option<Arc<Y>> {
        let mut n = self.data().data_ref_cnt.load(Relaxed);
        loop {
            if n == 0 {
                return None;
            }
            assert!(n < usize::MAX);
            if let Err(e) =
                self.data()
                    .data_ref_cnt
                    .compare_exchange_weak(n, n + 1, Relaxed, Relaxed)
            {
                n = e;
                continue;
            }
            return Some(Arc { ptr: self.ptr }); // implicit clone happens here
        }
    }
}

// same as in Weak
impl<Y> Deref for Arc<Y> {
    type Target = Y;
    fn deref(&self) -> &Y {
        // SAFETY: since there's an &Arc, data is there and can be shared
        unsafe { &*self.data().data.get() }
    }
}

/// same as in weak.rs
impl<Y> Clone for Weak<Y> {
    fn clone(&self) -> Self {
        // advances only allocation, not data (Arc's business)
        if self.data().alloc_ref_cnt.fetch_add(1, Relaxed) > usize::MAX / 2 {
            std::process::abort();
        }
        Weak { ptr: self.ptr }
    }
}

/// same as in weak.rs
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

/// a new impl which touches only 1 counter
/// impl in weak.rs makes another Weak pointer, which inc's alloc_ref_cnt too
impl<Y> Clone for Arc<Y> {
    fn clone(&self) -> Self {
        // advances only allocation, not data (Arc's business)
        if self.data().data_ref_cnt.fetch_add(1, Relaxed) > usize::MAX / 2 {
            std::process::abort();
        }
        Self { ptr: self.ptr }
    }
}

/// and the most impacted part
impl<Y> Drop for Arc<Y> {
    fn drop(&mut self) {
        if self.data().data_ref_cnt.fetch_sub(1, Relaxed) == 1 {
            fence(Acquire);
            // SAFETY: the data ref count is 0 => there're no other Arc's
            unsafe {
                ManuallyDrop::drop(&mut *self.data().data.get());
            }
            // no `Arc<Y>`'s left - downgrade our pointer to decrement the value
            drop(Weak { ptr: self.ptr });
        }
    }
}
