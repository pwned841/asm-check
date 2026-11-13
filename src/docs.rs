//! One-line descriptions of instructions and registers, for hovers and
//! completion.

use crate::mnemonic::condition;
use crate::reg::RegId;

#[rustfmt::skip]
const INSTRUCTIONS: &[(&str, &str)] = &[
    ("adc", "add with carry: `dst += src + CF`"),
    ("add", "add: `dst += src`"),
    ("and", "bitwise AND: `dst &= src`"),
    ("bsf", "bit scan forward: index of the lowest set bit"),
    ("bsr", "bit scan reverse: index of the highest set bit"),
    ("bswap", "reverse the byte order of a register"),
    ("bt", "copy one bit of the operand into CF"),
    ("btc", "copy one bit into CF, then flip it"),
    ("btr", "copy one bit into CF, then clear it"),
    ("bts", "copy one bit into CF, then set it"),
    ("call", "push the return address and jump to a function"),
    ("cbw", "sign-extend `al` into `ax`"),
    ("cdq", "sign-extend `eax` into `edx:eax`, before a 32-bit `idiv`"),
    ("cdqe", "sign-extend `eax` into `rax`"),
    ("clc", "clear the carry flag"),
    ("cld", "clear the direction flag: string instructions move forward"),
    ("cmc", "flip the carry flag"),
    ("cmp", "compare: set the flags from `a - b`, without storing the result"),
    ("cmpsb", "compare the bytes at `[rsi]` and `[rdi]`, then advance both"),
    ("cmpsd", "compare the dwords at `[rsi]` and `[rdi]`, then advance both"),
    ("cmpsq", "compare the qwords at `[rsi]` and `[rdi]`, then advance both"),
    ("cmpsw", "compare the words at `[rsi]` and `[rdi]`, then advance both"),
    ("cmpxchg", "compare with `rax` and swap if equal"),
    ("cpuid", "query processor features, selected by `eax`"),
    ("cqo", "sign-extend `rax` into `rdx:rax`, before a 64-bit `idiv`"),
    ("cwd", "sign-extend `ax` into `dx:ax`"),
    ("cwde", "sign-extend `ax` into `eax`"),
    ("dec", "subtract 1; leaves the carry flag alone"),
    ("div", "unsigned divide `rdx:rax` by the operand: quotient in `rax`, remainder in `rdx`"),
    ("endbr64", "marks a valid indirect branch target (CET)"),
    ("enter", "create a stack frame"),
    ("hlt", "halt the processor (privileged)"),
    ("idiv", "signed divide `rdx:rax` by the operand: quotient in `rax`, remainder in `rdx`"),
    ("imul", "signed multiply"),
    ("inc", "add 1; leaves the carry flag alone"),
    ("int", "software interrupt"),
    ("int3", "breakpoint trap"),
    ("jecxz", "jump if `ecx` is zero"),
    ("jmp", "jump unconditionally"),
    ("jrcxz", "jump if `rcx` is zero"),
    ("lahf", "load the low flags into `ah`"),
    ("lea", "load effective address: compute the address, without touching memory"),
    ("leave", "tear down the stack frame: `mov rsp, rbp` then `pop rbp`"),
    ("lodsb", "load the byte at `[rsi]` into `al`, then advance `rsi`"),
    ("lodsd", "load the dword at `[rsi]` into `eax`, then advance `rsi`"),
    ("lodsq", "load the qword at `[rsi]` into `rax`, then advance `rsi`"),
    ("lodsw", "load the word at `[rsi]` into `ax`, then advance `rsi`"),
    ("loop", "decrement `rcx` and jump if it is not zero"),
    ("loope", "decrement `rcx` and jump if it is not zero and ZF = 1"),
    ("loopne", "decrement `rcx` and jump if it is not zero and ZF = 0"),
    ("loopnz", "decrement `rcx` and jump if it is not zero and ZF = 0"),
    ("loopz", "decrement `rcx` and jump if it is not zero and ZF = 1"),
    ("lzcnt", "count the leading zero bits"),
    ("mov", "copy `src` into `dst`"),
    ("movabs", "load a 64-bit immediate or absolute address"),
    ("movsb", "copy the byte at `[rsi]` to `[rdi]`, then advance both"),
    ("movsd", "copy the dword at `[rsi]` to `[rdi]`, then advance both"),
    ("movsq", "copy the qword at `[rsi]` to `[rdi]`, then advance both"),
    ("movsw", "copy the word at `[rsi]` to `[rdi]`, then advance both"),
    ("movsx", "copy with sign extension from a smaller operand"),
    ("movsxd", "sign-extend a 32-bit operand into a 64-bit register"),
    ("movzx", "copy with zero extension from a smaller operand"),
    ("mul", "unsigned multiply `rax` by the operand: result in `rdx:rax`"),
    ("neg", "two's complement negation: `dst = -dst`"),
    ("nop", "do nothing"),
    ("not", "flip every bit; the flags are unchanged"),
    ("or", "bitwise OR: `dst |= src`"),
    ("pause", "hint that this is a spin-wait loop"),
    ("pop", "load the top of the stack, then `rsp += 8`"),
    ("popcnt", "count the set bits"),
    ("popf", "pop the flags register"),
    ("popfq", "pop the flags register"),
    ("push", "`rsp -= 8`, then store the operand at the top of the stack"),
    ("pushf", "push the flags register"),
    ("pushfq", "push the flags register"),
    ("rcl", "rotate left through the carry flag"),
    ("rcr", "rotate right through the carry flag"),
    ("rdtsc", "read the time-stamp counter into `edx:eax`"),
    ("rdtscp", "read the time-stamp counter and the processor ID"),
    ("ret", "pop the return address and jump to it"),
    ("retf", "far return"),
    ("retn", "near return"),
    ("rol", "rotate left"),
    ("ror", "rotate right"),
    ("sahf", "store `ah` into the low flags"),
    ("sal", "shift left (same as `shl`)"),
    ("sar", "arithmetic shift right: the sign bit is replicated"),
    ("sbb", "subtract with borrow: `dst -= src + CF`"),
    ("scasb", "compare `al` with the byte at `[rdi]`, then advance `rdi`"),
    ("scasd", "compare `eax` with the dword at `[rdi]`, then advance `rdi`"),
    ("scasq", "compare `rax` with the qword at `[rdi]`, then advance `rdi`"),
    ("scasw", "compare `ax` with the word at `[rdi]`, then advance `rdi`"),
    ("shl", "shift left: multiply by a power of two"),
    ("shld", "shift left, filling from a second register"),
    ("shr", "logical shift right: unsigned divide by a power of two"),
    ("shrd", "shift right, filling from a second register"),
    ("stc", "set the carry flag"),
    ("std", "set the direction flag: string instructions move backward"),
    ("stosb", "store `al` at `[rdi]`, then advance `rdi`"),
    ("stosd", "store `eax` at `[rdi]`, then advance `rdi`"),
    ("stosq", "store `rax` at `[rdi]`, then advance `rdi`"),
    ("stosw", "store `ax` at `[rdi]`, then advance `rdi`"),
    ("sub", "subtract: `dst -= src`"),
    ("syscall", "enter the kernel: number in `rax`, arguments in `rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9`"),
    ("test", "set the flags from `a & b`, without storing the result"),
    ("tzcnt", "count the trailing zero bits"),
    ("ud2", "raise an invalid-opcode exception"),
    ("xadd", "exchange, then add"),
    ("xchg", "exchange two operands"),
    ("xor", "bitwise XOR: `dst ^= src`; `xor r, r` zeroes a register"),
];

/// A one-line description of a mnemonic, if it is one we know.
pub fn instruction(mnemonic: &str) -> Option<String> {
    if let Ok(index) = INSTRUCTIONS.binary_search_by_key(&mnemonic, |entry| entry.0) {
        return Some(INSTRUCTIONS[index].1.to_string());
    }
    let (family, code) = condition(mnemonic)?;
    let when = match code {
        "e" | "z" => "equal / zero (ZF = 1)",
        "ne" | "nz" => "not equal / not zero (ZF = 0)",
        "l" | "nge" => "less, signed (SF != OF)",
        "ge" | "nl" => "greater or equal, signed (SF = OF)",
        "le" | "ng" => "less or equal, signed (ZF = 1 or SF != OF)",
        "g" | "nle" => "greater, signed (ZF = 0 and SF = OF)",
        "b" | "c" | "nae" => "below, unsigned (CF = 1)",
        "ae" | "nb" | "nc" => "above or equal, unsigned (CF = 0)",
        "be" | "na" => "below or equal, unsigned (CF = 1 or ZF = 1)",
        "a" | "nbe" => "above, unsigned (CF = 0 and ZF = 0)",
        "s" => "negative (SF = 1)",
        "ns" => "not negative (SF = 0)",
        "o" => "overflow (OF = 1)",
        "no" => "no overflow (OF = 0)",
        "p" | "pe" => "parity even (PF = 1)",
        _ => "parity odd (PF = 0)",
    };
    let action = match family {
        "j" => "jump if",
        "set" => "set the byte operand to 1 if",
        _ => "copy `src` into `dst` if",
    };
    Some(format!("{action} {when}"))
}

/// What a register is conventionally used for on x86-64 Linux.
pub fn register(id: RegId) -> &'static str {
    match id {
        RegId::RAX => {
            "return value; system call number and result; implicit operand of `mul` and `div`. Caller-saved."
        }
        RegId::RCX => {
            "4th function argument; shift and `rep` counter. Destroyed by `syscall`. Caller-saved."
        }
        RegId::RDX => "3rd argument; upper half of `mul` and `div`. Caller-saved.",
        RegId::RBX => "general purpose. Callee-saved: a function must restore it.",
        RegId::RSP => "stack pointer.",
        RegId::RBP => "frame pointer, by convention. Callee-saved.",
        RegId::RSI => "2nd argument; source of string instructions. Caller-saved.",
        RegId::RDI => "1st argument; destination of string instructions. Caller-saved.",
        RegId::R8 => "5th argument. Caller-saved.",
        RegId::R9 => "6th argument. Caller-saved.",
        RegId::R10 => "4th system call argument, where a function uses `rcx`. Caller-saved.",
        RegId::R11 => "scratch. Destroyed by `syscall`. Caller-saved.",
        _ => "general purpose. Callee-saved: a function must restore it.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mnemonic::known_mnemonics;

    #[test]
    fn every_modelled_instruction_is_described() {
        assert!(INSTRUCTIONS.windows(2).all(|pair| pair[0].0 < pair[1].0));
        for mnemonic in known_mnemonics() {
            assert!(instruction(mnemonic).is_some(), "`{mnemonic}` has no description");
        }
        assert_eq!(instruction("jne").unwrap(), "jump if not equal / not zero (ZF = 0)");
        assert!(instruction("cmovg").unwrap().starts_with("copy"));
        assert_eq!(instruction("vaddps"), None);
    }
}
