use std::sync::atomic::fence;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::{ops::Deref, ptr::NonNull, sync::atomic::AtomicUsize};

// internal structure for the impl
struct ArcData<Y> {
    ref_cnt: AtomicUsize,
    data: Y,
}

/*
 * Arc should be a pointer to the above.
 * It shouldn't be a wrapper over box (it's non-shared), nor reference (lifetime doesn't match) => need a pointer
 *
 * NonNull guarantees that there's something behind the reference
 * while providing same capabilities as a reference.
 */
pub struct Arc<Y> {
    ptr: NonNull<ArcData<Y>>,
}

/*
 * `Send'ing Arc means sharing Y => Y needs Sync
 * then dropping Arc "moves" Y to another thread with the Arc => Y needs Send
 * The same stands for sharing &Arc.
 */
unsafe impl<Y: Send + Sync> Send for Arc<Y> {}
unsafe impl<Y: Send + Sync> Sync for Arc<Y> {}

impl<Y> Arc<Y> {
    /*
     * Make allocation using Box, then leak it and store
     * It guarantees data validity, as the allocation hides from Rust's radars
     */
    pub fn new(data: Y) -> Self {
        Self {
            ptr: NonNull::from(Box::leak(Box::new(ArcData {
                data,
                ref_cnt: AtomicUsize::new(1),
            }))),
        }
    }

    // a helper to dig to the data quickly, see Deref
    fn data(&self) -> &ArcData<Y> {
        // SAFETY: data is there unless we're in Drop
        unsafe { self.ptr.as_ref() }
    }
}

// make the Arc behave like pointers
impl<Y> Deref for Arc<Y> {
    type Target = Y;
    fn deref(&self) -> &Self::Target {
        &self.data().data
    }
}

// Some core functionality - make a copy of the ref counter
impl<Y> Clone for Arc<Y> {
    fn clone(&self) -> Self {
        if self.data().ref_cnt.fetch_add(1, Relaxed) > usize::MAX / 2
        // a reasonable limit, as # of threads is limited by mem (~9*10^18 threads, 11 bytes each => ~10^11 GB)
        {
            // aborting the whole process
            // it takes time => the limit should not be too close to usize::MAX
            std::process::abort();
        }
        Arc {
            ptr: self.ptr, /* clonning happens here implicitly */
        }
    }
}

// ... and dropping the counter
impl<Y> Drop for Arc<Y> {
    fn drop(&mut self) {
        /*
         * Relaxed ordering doesn't work, as another thread may be clonning or dropping the Arc.
         * All but last drops should happens-before the last one =>
         *  Release for all other subs and a fence to wait for the main drop
         */
        if self.data().ref_cnt.fetch_sub(1, Release) == 1 {
            fence(Acquire);
            drop(unsafe { Box::from_raw(self.ptr.as_ptr()) });
        }
    }
}

// Testing it
// the best way to run it is `cargo +nightly miri test'
#[cfg(test)]
mod test {
    use super::*;
    use std::{sync::atomic::AtomicUsize, thread::spawn};

    // a special drops counter to be enveloped to Arc
    static NUM_DROPS: AtomicUsize = AtomicUsize::new(0);
    struct DetectDrop;
    impl Drop for DetectDrop {
        fn drop(&mut self) {
            NUM_DROPS.fetch_add(1, Relaxed);
        }
    }

    #[test]
    fn test() {
        // two Arc's
        let x = Arc::new(("msg", DetectDrop));
        let y = x.clone();

        // one thread
        let jh = spawn(move || assert_eq!(x.0, "msg"));

        // it's still usable
        assert_eq!(y.0, "msg");

        // join the thread
        jh.join().unwrap();

        // x is droped by now, but not y
        assert_eq!(0, NUM_DROPS.load(Relaxed));

        // now drop Y
        drop(y);
        assert_eq!(1, NUM_DROPS.load(Relaxed));
    }
}

/// # Mutation
/// A simple case - allow accessing data only when ref counter is 1  
/// get_mut also should only ensure exclusiveness => &mut self as an input

impl<Y> Arc<Y> {
    // the parameter isn't `&mut self', to allow the underlying Y to be allowed to have get_mut of his own called
    pub fn get_mut(the_arc: &mut Self) -> Option<&mut Y> {
        // Relaxed works just fine, as we don't care about the actual moment
        // and nothing could "steal" our Arc anymore, as it's &mut
        if the_arc.data().ref_cnt.load(Relaxed) == 1 {
            // make sure all the drops are actually finished
            fence(Acquire);
            // SAFETY: Nothing else can access the data, since there's only 1 Arc
            // and we have exclusive access to it.
            unsafe { Some(&mut the_arc.ptr.as_mut().data) }
        } else {
            None
        }
    }
}
