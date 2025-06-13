mod atomic_load_and_store_operations;
mod compare_and_exchange_operations;
mod fetch_and_modify_operations;

/**
 * atomic types live in std::sync::atomic
 * they're are all internally mutable
 * all atomics have memory ordering arg, it'll be reviewed in chapter 3
 */
pub fn run() {
    atomic_load_and_store_operations::run();
    fetch_and_modify_operations::run();
    compare_and_exchange_operations::run();
}
