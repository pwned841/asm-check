//! Which assembler a source file is written for.
//!
//! Editors hand every `.asm` and `.s` file to the same language server, and
//! the folder of a Python project may well hold MASM or ARM assembly. This
//! tool only understands NASM, so anything else is recognised and left
//! alone rather than reported on as if it were NASM.

use crate::mnemonic::{is_data_directive, is_directive, is_instruction};
use crate::parser::comment_start;
use crate::reg::Reg;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Nasm,
    /// The GNU assembler: AT&T operands or dot-directives.
    Gnu,
    /// Microsoft's assembler: `name PROC`, `.code`, `MACRO`.
    Masm,
    /// ARM assembly, in armasm or GNU flavour.
    Arm,
}

impl Dialect {
    pub fn name(self) -> &'static str {
        match self {
            Dialect::Nasm => "NASM",
            Dialect::Gnu => "GNU assembler",
            Dialect::Masm => "MASM",
            Dialect::Arm => "ARM",
        }
    }
}

const GNU_DIRECTIVES: &[&str] = &[
    ".globl",
    ".global",
    ".text",
    ".data",
    ".bss",
    ".section",
    ".intel_syntax",
    ".att_syntax",
    ".file",
    ".type",
    ".size",
    ".align",
    ".p2align",
    ".balign",
    ".string",
    ".asciz",
    ".ascii",
    ".quad",
    ".long",
    ".byte",
    ".code64",
    ".macro",
    ".include",
    ".equ",
    ".set",
    ".cfi_startproc",
];

/// Words that only MASM puts second on a line (`main PROC`).
const MASM_SECOND: &[&str] = &["proc", "endp", "macro", "proto", "ends"];

/// Words that only MASM starts a line with.
const MASM_FIRST: &[&str] = &[
    ".model",
    ".code",
    ".686",
    ".386",
    ".486",
    ".xmm",
    ".stack",
    ".const",
    "assume",
    "option",
    ".endprolog",
    ".allocstack",
    ".pushreg",
    ".setframe",
    ".savereg",
];

/// Instructions that exist on ARM and nowhere in x86.
const ARM_ONLY: &[&str] = &[
    "stp", "ldp", "ldr", "ldrb", "ldrh", "ldrsw", "strb", "strh", "stur", "ldur", "cbz", "cbnz",
    "tbz", "tbnz", "adrp", "svc", "mrs", "msr", "movz", "movk", "csel", "cset", "ubfx", "sxtw",
    "uxtw", "bl", "blr", "area",
];

/// The dialect a source file is written in.
pub fn detect(source: &str) -> Dialect {
    let (mut gnu, mut masm, mut arm) = (false, false, false);
    for line in source.lines() {
        let code = &line[..comment_start(line)];
        let code = code.split("//").next().unwrap_or(code);
        let mut words = code.split_whitespace();
        let (Some(first), second) = (words.next(), words.next()) else { continue };
        let first = first.to_ascii_lowercase();
        let second = second.map(str::to_ascii_lowercase);

        if GNU_DIRECTIVES.contains(&first.as_str()) {
            gnu = true;
        }
        // `%rax`, but not `10 % 3`: the register name must follow directly.
        gnu |= code.match_indices('%').any(|(at, _)| {
            let name: String =
                code[at + 1..].chars().take_while(char::is_ascii_alphanumeric).collect();
            at > 0 && Reg::parse(&name).is_some()
        });

        // `main PROC` names something; `jmp proc` and `extern proc` only use a name.
        let names_something = !is_instruction(&first)
            && !is_directive(&first)
            && !is_data_directive(&first)
            && !first.starts_with('%')
            && !first.ends_with(':');
        masm |= MASM_FIRST.contains(&first.as_str())
            || (names_something
                && second.as_deref().is_some_and(|word| MASM_SECOND.contains(&word)));
        // A label may come first on the line: `entry: stp x29, x30, [sp]`.
        let mnemonic = if first.ends_with(':') { second.as_deref() } else { Some(first.as_str()) };
        arm |= mnemonic.is_some_and(|word| ARM_ONLY.contains(&word));
    }
    // MASM and ARM files do not mix; GNU syntax shows up in both ARM and x86.
    if masm {
        Dialect::Masm
    } else if arm {
        Dialect::Arm
    } else if gnu {
        Dialect::Gnu
    } else {
        Dialect::Nasm
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_each_dialect() {
        assert_eq!(detect("global _start\n_start:\n    mov rax, 60\n    syscall\n"), Dialect::Nasm);
        assert_eq!(detect(".globl _start\n_start:\n    movq $60, %rax\n"), Dialect::Gnu);
        assert_eq!(detect("main:\n    movq $60, %rax\n"), Dialect::Gnu);
        assert_eq!(
            detect("slp_save PROTO\npushxmm MACRO reg\n    sub rsp, 16\nENDM\n"),
            Dialect::Masm
        );
        assert_eq!(detect(".code\nmain PROC\n    ret\nmain ENDP\nEND\n"), Dialect::Masm);
        assert_eq!(
            detect("  AREA sw, CODE, READONLY;\nsw\n    stp x19, x20, [sp, #-16]!\n"),
            Dialect::Arm
        );
        assert_eq!(detect("f:\n    ldr x0, [x1]\n    bl g\n"), Dialect::Arm);
        assert_eq!(detect("start: stp x29, x30, [sp]\n"), Dialect::Arm);
    }

    #[test]
    fn nasm_is_not_mistaken_for_something_else() {
        let nasm = "\
section .text
global main
extern printf
main:
    mov eax, 10 % 3     ; not a %register
    mov rax, [rel msg]
    lea rsi, [rsp + 8]
    call printf
    jmp proc            ; a label named like a MASM keyword
    call proc
    extern proc
proc:
    ret
    ; ldr x0, [x1]  in a comment
end:
    ret
section .data
msg db 'stp', 0
";
        assert_eq!(detect(nasm), Dialect::Nasm);
        assert_eq!(detect(""), Dialect::Nasm);
    }
}
