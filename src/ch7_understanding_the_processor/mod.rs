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
use std::{
    hint::black_box,
    sync::atomic::{AtomicI32, AtomicU64, Ordering::*},
    thread,
    time::Instant,
};

///
/// a small function
///`cargo asm --release --target=aarch64-unknown-linux-musl --lib add_ten` VS `cargo asm --release --target=x86_64-unknown-linux-musl --lib add_ten`
/// x86_64 makes just 1 instruction, ARM - 4
/// 'cause x86_64 is CISC (Complex Instruction Set Computer) while ARM is RISC (Reduced Instruction Set Computer)
///
#[unsafe(no_mangle)]
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
#[allow(unused_must_use)]
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
#[allow(unused_must_use)]
fn read_n_modify(x: &AtomicI32) {
    x.fetch_add(10, Relaxed); // lock add  dword ptr [rdi], 10
}

///
/// By returning the value we force the compiler to use a more complex instruction - xadd.
/// There's only xadd and xchg but nothing similar for sub, and, or.
/// The sub can be implemented with an inverted xadd.
///
#[unsafe(no_mangle)]
#[allow(unused_must_use)]
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
#[allow(unused_must_use)]
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
#[allow(unused_must_use)]
fn read_n_modify_and_or_min_max(x: &AtomicI32) -> i32 {
    x.fetch_and(0xfe, Relaxed)
}

/// The code below get translated into exaclty the same asm as [`read_n_modify_and_or_min_max`]
#[unsafe(no_mangle)]
#[allow(unused_must_use)]
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
/// There'are new instructions in ARMv8.1 to squeeze even more perf. Right away `cas` (compare and exchange) is there and some more.
#[unsafe(no_mangle)]
#[allow(unused_must_use)]
pub fn fetch_add_ten_arm(x: &AtomicI32) -> i32 {
    x.fetch_add(10, Relaxed)
}

/// ## Compare and exchange @ ARM
/// It fits quite well to the LL/SC:
/// ```asm,no_run
/// cmp_n_xchg_arm:
///         .cfi_startproc
///         ldxr w8, [x0]
///         cmp w8, #5
///         b.ne .LBB200_2
///         mov w8, #6
///         stxr wzr, w8, [x0]
///         ret
/// .LBB200_2:
///         clrex
///         ret
/// ```
///
#[unsafe(no_mangle)]
#[allow(unused_must_use)]
pub fn cmp_n_xchg_arm_weak(x: &AtomicI32) {
    x.compare_exchange_weak(5, 6, Relaxed, Relaxed);
}

/// non-weak version makes a more complex code:
/// ```asm,no_run
/// cmp_n_xchg_arm:
///         .cfi_startproc
///         mov w8, #6
/// .LBB200_1:
///         ldxr w9, [x0]
///         cmp w9, #5
///         b.ne .LBB200_4
///         stxr w9, w8, [x0]
///         cbnz w9, .LBB200_1
///         ret
/// .LBB200_4:
///         clrex
///         ret
/// ```
///
/// 1. it retries on the store failures
/// 2. mov is now above all to have it outside of the LL/SC loop
/// In the case of ARM, a manually written compare-n-exchange loop isn't converted
/// to a nice LL/SC due to possibly complex intructions in the middle
/// which could clash with other optimizations.
///
#[unsafe(no_mangle)]
#[allow(unused_must_use)]
pub fn cmp_n_xchg_arm(x: &AtomicI32) {
    x.compare_exchange(5, 6, Relaxed, Relaxed);
}

/// # Cache
/// Memory is slow => CPUs leverage caching.
/// Cache almost isolates processor from memory: all reads and writes go through cache.
/// The overall memory read logic: in cache & relevant?
///     yes - return from cache
///     no  - goto memory -> save to cache & return
/// Cache has less bytes than memory => there's a cleanup mechanism.
/// The overall memory write logic:
///     - keep in cache
///     - store upon expelling
/// Cache works with blocks of usually 64 bytes (cache lines).
///
/// ## Cache coherence
/// There are usually several caching levels: L1 talks to L2, L2 to L3.
/// There could be even 4 levels. Speed: L1 > L2, size: L1 < L2.
///
/// It works like a 1-level cache with 1-core systems.
/// It starts to make difference in case there are multiple cores with its own L1 cache each.
/// L2, L3, etc are usually shared.
///
/// This means L1 can't assume it has the freshest data (another core could change it)
/// nor can keep writes for itself (another core can't see the write).
///
/// It's solved using a cache coherence protocol. Its implementation differs.
///
/// ## The write-through protocol
/// - writes aren't cached but are sent to the next layer
/// - other caches invalidate respictive lines on observing a write
/// => no benefit for writes on L1
///
/// ## The MESI protocol
/// It's named after possible cache line states:
/// - M - modified (but not yet written to memory)
/// - E - exclusive (has unmodified data, which is not stored in any other cache of the level)
/// - S - shared (has unmodified data, which may be stored in some other cache of the level)
/// - I - invalid (unused lines with zeroes or garbage)
///
/// Caches in MESI communicate to each other directly to maintain coherency.
/// 1. Read + cache miss + not in any other cache of the level => (N) get from the next level and make E
/// 2. Write + E => just write and make M, as other caches of the level don't have the line
/// 3. Read + cache miss + E is another cache level => get and make it S
/// 4. Read + cache miss + M is another cache level => flush to the next level and make it S
/// 5. Read as E (to make a write later) => ask others to I then make own line E
/// 6. Convert S to E => ask others to I then make own line E
///
/// There're variations:
/// - MOESI - allows sharing M to avoid flushes
/// - MESIF - determine what exact cache to read for an S
///
/// ## Impact on performance
/// Caching has significant impact on performance of atomics. Atomics are fast =>
/// need to make a ton of ops to see it. Optimization has to be switched off, of course.
///
/// Do `cargo build --release` then run it several times to see example's results.
///
pub fn cache() {
    example_1();
    example_2();
    example_3();
    example_4();
    example_5();
    example_6();
}

/// It illustrates how much a 10^9 load of U64 take.  
/// The books says 300ms, I've got 235+/-2ms.
fn example_1() {
    // black_box tells compiler that
    // (1) the value is used and
    // (2) no assumptions about the nature of changes can be made
    black_box(&A);
    let start = Instant::now();
    // just a lot of operations
    for _ in 0..1_000_000_000 {
        black_box(A.load(Relaxed));
    }
    println!("example 1: {:?}", start.elapsed());
}

/// This example illustrates how accessing (loading) atomic from
/// a background thread affects prefomance.
/// It takes ~+2ms comparing to the example 1.
/// The book says there shouldn't be any.
fn example_2() {
    black_box(&A);
    thread::spawn(|| loop {
        black_box(A.load(Relaxed));
    });
    let start = Instant::now();
    for _ in 0..1_000_000_000 {
        black_box(A.load(Relaxed));
    }
    println!("example 2: {:?}", start.elapsed());
}

/// Background thread does store now
/// instead of load from the example 2.
/// It takes longer - from ~700ms to 1.5s,
/// what's caused by that store
/// requires exclusive access to a cache line =>
/// it forces the loads to refresh cache lines (or wait for it to unlock).
fn example_3() {
    black_box(&A);
    thread::spawn(|| loop {
        black_box(A.store(0, Relaxed));
    });

    let start = Instant::now();
    for _ in 0..1_000_000_000 {
        black_box(A.load(Relaxed));
    }
    println!("example 3: {:?}", start.elapsed());
}

/// Same as 3, but with compare&exchange that ever only fails.  
/// Technically, it may not affect prtormance, but some processors
/// use operations that lock cache lines for these operations
/// disregard the outcome.
/// On x86 the example shows the same effect as the 3rd.
/// => It's sometimes beneficial to
#[allow(unused_must_use)]
fn example_4() {
    black_box(&A);
    thread::spawn(|| loop {
        black_box(A.compare_exchange(1, 2, Relaxed, Relaxed));
    });

    let start = Instant::now();
    for _ in 0..1_000_000_000 {
        black_box(A.load(Relaxed));
    }
    println!("example 4: {:?}", start.elapsed());
}

/// compiler might assume it's always 0
/// => reference is used in the examples above
/// to express the fact that there may be other operations happen to other refs
static A: AtomicU64 = AtomicU64::new(0);

/// Cache works on the level of lines, not individual variables.
static B: [AtomicU64; 3] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

/// Despite the fact that the bg thread doesn't touch B[1],
/// its load gets slowed down by operations on B[0] and B[2].
/// It's called false-sharing.
fn example_5() {
    black_box(&A);
    thread::spawn(|| loop {
        B[0].store(0, Relaxed);
        B[2].store(0, Relaxed);
    });
    let start = Instant::now();
    for _ in 0..1_000_000_000 {
        black_box(B[1].load(Relaxed));
    }
    println!("with neighbours: {:?}", start.elapsed());
}

/// False-sharing can be evaded manually
/// by adding a padding of cache line size
#[repr(align(64))] // +56 bytes to the structure to make it 1-cache-line-long approx
struct Spaced(AtomicU64);

static C: [Spaced; 3] = [
    Spaced(AtomicU64::new(0)),
    Spaced(AtomicU64::new(0)),
    Spaced(AtomicU64::new(0)),
];

/// This example, although does exactly the same
/// what the 5th in regard of logic,
/// uses spaced array => isn't affected by the false-sharing effect.
/// Takeaways:
/// - don't compress unrelated atomics in memory, it may cause perf drops
/// - having logically related atomics close may be beneficial
///     due to high probability for the 2-nd atomic to be already
///     on an exclusively acquired line
fn example_6() {
    black_box(&A);
    thread::spawn(|| loop {
        C[0].0.store(0, Relaxed);
        C[2].0.store(0, Relaxed);
    });
    let start = Instant::now();
    for _ in 0..1_000_000_000 {
        black_box(C[1].0.load(Relaxed));
    }
    println!("with spaced neighbours: {:?}", start.elapsed());
}

/// # Reordering
/// Processor and compiler can reorder instructions. Some examples caused by CPU:
/// ## Store buffers
/// CPU may have ones.
/// It's used to temporary store writes, so CPU can proceed right away.
/// Store buffer gets emptied in background.  
/// They're per-core => other cores may not see the writes for a moment.
///
/// ## Invalidation queues
/// It's common for processors to have a queue for cache lines invalidation requests =>
/// there could be moments when a line invalidation request is sent,
/// but it's in the queue and other cores aren't aware of the write yet.
///
/// ## Pipelining
/// CPU usually analyze instructions in its execution queue and run them in parallel if it's safe =>
/// a slow memory operation may still be running while a quick operation on unrelated value in register is already done.
///
/// ## Etc
///
/// There are other technics and proprietary technologies that may also affect execution order.  
#[allow(dead_code)]
fn reordering_placeholder() {}

/// # Memory Ordering
/// In rust we have tech to tell the compiler what instructions it shouldn't reorder.
/// Operation type also tells what reordering is allowed:
/// - Relaxed || non-atomic => any reordering
/// - Acquire => can't be pushed forward
/// - Release => can't be pushed back
/// - SeqCst => no reordering at all
///
/// > Other-Multi-Copy atomicity is a feature of a CPU architecture which guarantees that
/// > a write visible to one core is visible to all cores in the same moment. Reordering is the only thing
/// > that causes memory ordering question in Other-Multi-Copy -atomic CPUs. But there are more
/// > trisky CPUs (like GPUs) which only fit memory model from Chapter 3.
///
/// X86 and ARM are very different in regard of ordering:
/// - weakly ordered: ARM allows to reorder any memory instructions unless cannot
/// - strongly ordered: X86 is picky about reordering out of the box
///
/// ## X86: Strongly Ordered
///
/// It doesn't allow:
/// - pushing load operations forward (as we might expect the code to use just loaded value)
/// - pulling store operations back (as we might expect the code to compute the value to store)
///
/// So:
/// 1. the only race is could be if a store gets delayed until after a later load
/// 2. we have load-acquire and store-release for free on X86 => rust doesn't use lock in these cases
///
/// In order to make a store SeqCst, rustc uses xchg, which is store & load at the same time
/// => CPU doesn't re-order it and no lock needed!
///
/// Load SeqCst is the same as load-acqure.
///
/// tl;dr: X86 has the same cost of all ordering but store SeqCst.
///
/// ## ARM64: Weakly Ordered
///
/// No blanket guarantees => Release and Acquire are different from Relaxed.
///
///
#[allow(dead_code)]
fn memory_ordering_placeholder() {}
