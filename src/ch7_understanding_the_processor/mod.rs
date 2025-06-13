///
/// The chapter is about what's going on on processor level.
/// - what the atomics compile down to
/// - difference in processor architectures
/// - why's the compare_exchange _weak
/// - how instructions order the memory
/// - how caches mess things up
///
/// Focus on 2 architectures:
/// (1) x86-64 - AMD's 64-bit version of Intel's x86 architecture
/// (2) ARM64 - 64-bit version of ARM architecture
///
/// They're unalike in many ways e.g. diff aproach to atomics.
///
/// rustc may reduce functions and calls, structs & enums to bytes and addresses,
/// loops and conditionals to jumps and branch instructions.
///
/// To look at what rustc does to our program:
/// - compile as usual + objdump (it leaves around labels if debug) => optimization + only same arch as it was compiled for
/// - `rustc --emit=asm` to tell compiler to make just ASM => lots of supplemental code & info for ASM compiler
/// - cargo-show-asm tool - only your code shown, may connect asm instructions with code lines
/// - https://godbolt.org Web tool for small projects
///
/// Here and later we assume x86_64-unknown-linux-musl and aarch64-unknown-linux-musl targets.
///
use std::sync::atomic::{AtomicI32, Ordering::*};

///
/// a small function
///`cargo asm --release --target=aarch64-unknown-linux-musl --lib add_ten` VS `cargo asm --release --target=x86_64-unknown-linux-musl --lib add_ten`
/// x86_64 makes just 1 instruction, ARM - 4
/// 'cause x86_64 is CISC (Complex Instruction Set Computer) while ARM is RISC (Reduced Instruction Set Computer)
///
#[unsafe(no_mangle)]
#[allow(dead_code)]
fn add_ten(num: &mut i32) {
    *num += 10;
}

///
/// ## Load & store
///
/// Atomic and non-atomic store produce the same code
/// of one `mov` or `str`.
/// cargo asm --release --target=aarch64-unknown-linux-musl --lib load_n_store
/// cargo asm --release --target=x86_64-unknown-linux-musl --lib load_n_store
/// E.g. for aarch64:
/// ```asm,no_run
/// load_n_store:
///     .cfi_startproc
///     str wzr, [x0]
///     str wzr, [x1]
///     ret
/// ```
///
/// str and mov are already atomic as for relaxed ordering.
///
/// Same holds for load - see the other 2 instructions.
///
/// Processor makes diff instructions != we ignore atomicity in the code,
/// as in some cases processor splits, joins and/or moves operations around.
/// E.g. look at the transcript below.
///
#[unsafe(no_mangle)]
#[allow(dead_code, unused_must_use)]
fn load_n_store(x: &mut i32, y: &AtomicI32) -> i32 {
    *x = 0; // #1 - str wzr, [x0]
    y.store(0, Relaxed); // #3 - str wzr, [x1]
    y.load(Relaxed); // #4 - ldr wzr, [x1]
    *x // #2 - mov w0, wzr
}

///
/// # Read & modify
///
/// See add_ten for a non-atomic example.
/// The x86_64 version is a single operation,
/// but it's not atomic, as the processor spits it into several microinstructions.
///
///
/// ## x86's lock
///
/// Intel x86 has the lock operation prefix to address the matter.
/// Its first implementation prevented all the CPU cores from accessing
/// the memory, what's not efficient. The modern lock only prevents modifications
/// of a small piece of memory in question.
///
/// The lock prefix only works for add, sub, and, not, or, xor.
/// The xchg instruction has an implicit lock prefix - it's always atomic.
///
/// The fetch_add just adds lock on x86-64's add ... in case we don't use the return value.
///
///
#[unsafe(no_mangle)]
#[allow(dead_code, unused_must_use)]
fn read_n_modify(x: &AtomicI32) {
    x.fetch_add(10, Relaxed); // lock add  dword ptr [rdi], 10
}

///
/// By returning the value we force the compiler to use a more complex instruction - xadd.
/// There's only xadd and xchg but nothing similar for sub, and, or.
/// The sub can be implemented with an inverted xadd.
///
#[unsafe(no_mangle)]
#[allow(dead_code, unused_must_use)]
fn read_n_modify_n_add(x: &AtomicI32) -> i32 {
    // mov eax, 10
    // lock xadd       dword ptr [rdi], eax
    x.fetch_add(10, Relaxed)
}

///
/// bts, btr, btc: bit test and set/reverse/complement - can implement 1-bit changes
/// They all support the lock prefix ... but are not used.
/// _... to avoid complexity within the compiler, I think_
///
#[unsafe(no_mangle)]
#[allow(dead_code, unused_must_use)]
fn read_n_modify_n_or(x: &AtomicI32) -> i32 {
    x.fetch_and(!1, Relaxed)
}

/// In addition to fetch_or and fetch_and, fetch_min and fetch_max require more complex logic to be atomic
/// => lock prefix isn't sufficient.
///
/// ## x86 compare-and-exchange
/// Any fetch-and-modify can be implemented with compare-and-exchange in a loop (see [`crate::ch2_atomics::compare_and_exchange_operations::increment`]).
/// The `cmpxchg` isn't lock-prefix-able => compiler has to use a loop.
/// ```asm,no_run
/// read_n_modify_and_or_min_max:
///         .cfi_startproc
///         mov eax, dword ptr [rdi]
///         .p2align        4, 0x90
/// .LBB198_1:
///         mov ecx, eax
///         and ecx, 254
///         lock cmpxchg    dword ptr [rdi], ecx
///         jne .LBB198_1
///         ret
/// ```
/// 1. eax is initialized with the current value
/// 2. ecx is used to calc a new value from what's in eax ATM
/// 3. cmpxchg
///     - compares first argument (memory) with eax (an implicit convention) then
///     - dump memory content to eax
///     - if eql => update memory from ecx
///     - if not => just set the operation status register to "false"
/// 4. jne check that the cmpxchg did the job
///     - if not - it returns back to `2.` (eax is already initialized with the new value from memory)
///
#[unsafe(no_mangle)]
#[allow(dead_code, unused_must_use)]
fn read_n_modify_and_or_min_max(x: &AtomicI32) -> i32 {
    x.fetch_and(0xfe, Relaxed)
}

/// The code below get translated into exaclty the same asm as [`read_n_modify_and_or_min_max`]
#[unsafe(no_mangle)]
#[allow(dead_code, unused_must_use)]
fn read_n_modify_and_or_min_max_long_story(x: &AtomicI32) -> i32 {
    let mut current = x.load(Relaxed);
    loop {
        let new = current | 0xfe;
        match x.compare_exchange(current, new, Relaxed, Relaxed) {
            Ok(v) => return v,
            Err(v) => current = v,
        }
    }
}

/// # RISC
/// Load-linked/store-conditional (LL/SC loop) is the closest analogue to the above loop in RISC.
/// The store checks whether memory was overwritten
/// by another thread between now and the previous linked instruction,
/// load-linked in this case.
/// The pair could be retried up until the store succeedes.
///
/// There are 2 gotchas with the pair:
/// - only 1 address / core can be tracked
/// - the store part could have false-negatives (nobody touched the mem, but the store says otherwise)
///
/// False-negatives are caused by checking a chunk for modifications instead of a byte.
///
/// ## Load-exclusive and store-exclusive
///
/// There are no complex operations in RISC - all needs some sync between several operations.
/// ldxr, strx, clrex (load, store, clear exclusive register) are atomic instructions to implement operations.
/// clrex is the same as strx but doesn't actually store anything, just links to the prior command.
///
/// ```asm,no_run
/// fetch_add_ten_arm:
///         .cfi_startproc
/// .LBB199_1:
///         ldxr w8, [x0]
///         add w8, w8, #10
///         stxr w9, w8, [x0]
///         cbnz w9, .LBB199_1
///         ret
/// ```
///
/// The asm code is similar to non-atomic version but ...
/// with ldrx instead of ldr, strx instead of str and a comparison: cbnz - compare and branch on nonzero.
/// stxr stores result to x9: 0 => success, 1 => failure.
///
/// LL/SC is very flexible, as it creates a sort of "critical section" in between allowing to implement many other atomic operations.
/// Although extending the gap may cause the loop to spin forever.
///
/// There'are new instructions in ARMv8.1 to squeeze even more perf. Right away `cas`` (compare and exchange) is there and some more.
#[unsafe(no_mangle)]
#[allow(dead_code, unused_must_use)]
pub fn fetch_add_ten_arm(x: &AtomicI32) -> i32 {
    x.fetch_add(10, Relaxed)
}
