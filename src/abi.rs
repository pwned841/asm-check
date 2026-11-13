//! Calling conventions of x86-64 Linux: the System V ABI for functions and
//! the kernel ABI for `syscall`.

use crate::reg::{RegId, RegSet};

/// Registers a `syscall` may read: the number in `rax`, then six arguments.
pub const SYSCALL_INPUTS: RegSet =
    RegSet::of(&[RegId::RAX, RegId::RDI, RegId::RSI, RegId::RDX, RegId::R10, RegId::R8, RegId::R9]);

/// The registers holding system call arguments, in order. The fourth is
/// `r10`, where a function call uses `rcx`.
pub const SYSCALL_ARGUMENTS: [RegId; 6] =
    [RegId::RDI, RegId::RSI, RegId::RDX, RegId::R10, RegId::R8, RegId::R9];

/// Registers the kernel destroys on `syscall`: `rcx` receives the return
/// address and `r11` the saved flags.
pub const SYSCALL_CLOBBERS: RegSet = RegSet::of(&[RegId::RCX, RegId::R11]);

/// Registers a function may overwrite without restoring them.
pub const CALLER_SAVED: RegSet = RegSet::of(&[
    RegId::RAX,
    RegId::RCX,
    RegId::RDX,
    RegId::RSI,
    RegId::RDI,
    RegId::R8,
    RegId::R9,
    RegId::R10,
    RegId::R11,
]);

/// Registers a function must hand back unchanged.
pub const CALLEE_SAVED: RegSet =
    RegSet::of(&[RegId::RBX, RegId::RBP, RegId::R12, RegId::R13, RegId::R14, RegId::R15]);

/// `exit` and `exit_group`.
pub const EXIT_SYSCALLS: [i64; 2] = [60, 231];

/// Library functions that never return to their caller.
pub const NORETURN_FUNCTIONS: &[&str] =
    &["exit", "_exit", "_Exit", "abort", "quick_exit", "__stack_chk_fail", "ExitProcess"];

/// Entry points: they are not called, so they have no return address and no
/// caller whose registers must be preserved.
pub fn is_entry_point(name: &str) -> bool {
    name == "_start"
}
