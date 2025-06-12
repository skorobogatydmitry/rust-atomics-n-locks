/*
 * The chapter is about what's going on on processor level.
 * - what the atomics compile down to
 * - difference in processor architectures
 * - why's the compare_exchange _weak
 * - how instructions order the memory
 * - how caches mess things up
 *
 * Focus on 2 architectures:
 * (1) x86-64 - AMD's 64-bit version of Intel's x86 architecture
 * (2) ARM64 - 64-bit version of ARM architecture
 *
 * They're unalike in many ways e.g. diff aproach to atomics.
 *
 * rustc may reduce functions and calls, structs & enums to bytes and addresses,
 * loops and conditionals to jumps and branch instructions.
 *
 * To look at what rustc does to our program:
 * - compile as usual + objdump (it leaves around labels if debug) => optimization + only same arch as it was compiled for
 * - `rustc --emit=asm` to tell compiler to make just ASM => lots of supplemental code & info for ASM compiler
 * - cargo-show-asm tool - only your code shown, may connect asm instructions with code lines
 * - https://godbolt.org Web tool for small projects
 *
 * Here and later we assume x86_64-unknown-linux-musl and aarch64-unknown-linux-musl targets.
 */

use std::sync::atomic::{AtomicI32, Ordering::*};

/*
 * a small function
 *`cargo asm --release --target=aarch64-unknown-linux-musl --lib add_ten` VS `cargo asm --release --target=x86_64-unknown-linux-musl --lib add_ten`
 * x86_64 makes just 1 instruction, ARM - 4
 * 'cause x86_64 is CISC (Complex Instruction Set Computer) while ARM is RISC (Reduced Instruction Set Computer)
 */
#[unsafe(no_mangle)]
#[allow(dead_code)]
fn add_ten(num: &mut i32) {
    *num += 10;
}

/* Load & store
 * both operations below produces the same code
 * of one `mov' or `str'.
 * cargo asm --release --target=aarch64-unknown-linux-musl --lib load_n_store
 * cargo asm --release --target=x86_64-unknown-linux-musl --lib load_n_store
 * ```
 * load_n_store:
 *     .cfi_startproc
 *     str wzr, [x0]
 *     str wzr, [x1]
 *     ret
 * ```
 *
 * str and mov are already atomic as for relaxed ordering.
 *
 * Processor makes diff instructions != we ignore atomicity in the code,
 * as in some cases processor splits, joins and/or moves operations around.
 * E.g. look at the transcript below.
 */
#[unsafe(no_mangle)]
#[allow(dead_code, unused_must_use)]
fn load_n_store(x: &mut i32, y: &mut AtomicI32) -> i32 {
    *x = 0; // #1 - str wzr, [x0]
    y.store(0, Relaxed); // #3 - str wzr, [x1]
    y.load(Relaxed); // #4 - ldr wzr, [x1]
    *x // #2 - mov w0, wzr
}
