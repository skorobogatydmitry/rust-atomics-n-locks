/*
 * std::sync::Arc is a thread-safe shared ref counter with heap allocation
 */

mod basic;

mod weak;

mod optimization;

/*
 * takeaways:
 * - memory ordering is hard
 * - Arc is useful - it's like Rc but for threads
 * - Arc provides exclusive access if you ask politely
 * - the Weak avoids cycles
 * - there're NonNull and ManuallyDrop - tricky low-level wrappers
 * - thanks God there's Mutex - sync'ing multiple atomics is hard
 * - spin locks are not to be feared
 */
