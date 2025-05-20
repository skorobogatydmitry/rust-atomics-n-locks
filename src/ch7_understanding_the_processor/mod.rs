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
 * To look at what rustc does with our program:
 * - compile as usual + objdump (it leaves around labels if debug) => optimization + only same arch as it was compiled for
 * - `rustc --emit=asm` to tell compiler to make just ASM => lots of supplemental code & info for ASM compiler
 * - cargo-show-asm tool - only your code shown, may connect asm instructions with code lines
 * - https://godbolt.org Web tool for small projects
 *
 * Here and later we assume x86_64-unknown-linux-musl and aarch64-unknown-linux-musl targets.
 */

/*
 * a small function
 *`cargo asm --lib 27 --release --target=aarch64-unknown-linux-musl` VS `cargo asm --lib 27 --release --target=x86_64-unknown-linux-musl`
 * x86_64 makes just 1 instruction, ARM - ~5\
 * 'cause x86_64 is CISC (Complex Instruction Set Computer) while ARM is RISC (Reduced Instruction Set Computer)
 */
#[unsafe(no_mangle)]
#[allow(dead_code)]
fn add_ten(num: &mut i32) {
    *num += 10;
}
