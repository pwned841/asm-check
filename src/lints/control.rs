//! Control flow: running off the end of a function, dead code, returning
//! from the entry point.

use super::Context;
use crate::diagnostic::{Diagnostic, Group, Lint};
use crate::insn::{Flow, Target};
use crate::reg::RegId;

pub static FALLTHROUGH: Lint = Lint {
    name: "fallthrough",
    group: Group::Suspicious,
    summary: "execution runs off the end of a function, into the next one or into nothing",
};

pub static UNREACHABLE_CODE: Lint = Lint {
    name: "unreachable-code",
    group: Group::Suspicious,
    summary: "instructions after a `jmp`, `ret` or exit that no label makes reachable",
};

pub static RET_IN_START: Lint = Lint {
    name: "ret-in-start",
    group: Group::Correctness,
    summary: "`ret` in `_start`, which has no return address",
};

pub static INT80_IN_64BIT: Lint = Lint {
    name: "int80-in-64bit",
    group: Group::Suspicious,
    summary: "`int 0x80`, the 32-bit system call interface, used in 64-bit code",
};

pub static REDUNDANT_JUMP: Lint = Lint {
    name: "redundant-jump",
    group: Group::Style,
    summary: "a `jmp` to the instruction that follows it",
};

const EXIT_HELP: &str =
    "end with `ret`, a `jmp`, or the exit system call (`mov eax, 60` / `syscall`)";

pub fn check(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;

    for function in &program.functions {
        for &(block, next) in &function.falls_into {
            let last = &program.insns[program.blocks[block].end - 1];
            let next = &program.functions[next];
            let message =
                format!("`{}` falls through into the function `{}`", function.name, next.name);
            out.push(Diagnostic::new(&FALLTHROUGH, last.span, message).help(EXIT_HELP));
        }
        if function.entry_point {
            for &block in &function.blocks {
                let block = &program.blocks[block];
                for insn in &program.insns[block.start..block.end] {
                    if insn.fx.flow == Flow::Ret {
                        out.push(
                            Diagnostic::new(
                                &RET_IN_START,
                                insn.span,
                                format!("`ret` in `{}`, which nothing called", function.name),
                            )
                            .note(
                                "the top of the stack holds `argc`, not a return address: \
                                 `ret` jumps to it and crashes",
                            )
                            .help("leave with the exit system call: `mov eax, 60` / `syscall`"),
                        );
                    }
                }
            }
        }
    }

    for (index, block) in program.blocks.iter().enumerate() {
        let last_index = block.end - 1;
        let last = &program.insns[last_index];

        if block.falls_off && block.reachable {
            // A `syscall` whose number we cannot see may well be `exit`.
            let number = program.constant_before(last_index, RegId::RAX);
            if last.fx.flow != Flow::Syscall || number.is_some() {
                let mut diagnostic = Diagnostic::new(
                    &FALLTHROUGH,
                    last.span,
                    "execution runs past the last instruction",
                )
                .note("the processor goes on to execute whatever bytes come next in memory")
                .help(EXIT_HELP);
                if let (Flow::Syscall, Some(number)) = (&last.fx.flow, number) {
                    diagnostic = diagnostic
                        .note(format!("system call {number} is not `exit`: it returns here"));
                }
                out.push(diagnostic);
            }
        }

        let first = &program.insns[block.start];
        if !block.reachable && first.labels.is_empty() && block.start > 0 {
            let before = &program.insns[block.start - 1];
            let chained = index > 0 && {
                let previous = &program.blocks[index - 1];
                !previous.reachable && program.insns[previous.start].labels.is_empty()
            };
            if before.run == first.run && !chained {
                out.push(
                    Diagnostic::new(&UNREACHABLE_CODE, first.span, "unreachable code")
                        .note_at(
                            before.span,
                            format!(
                                "control never gets past `{}` on line {}, and no label leads here",
                                cx.code(before),
                                before.line + 1
                            ),
                        )
                        .help("remove it, or add the label that was meant to jump here"),
                );
            }
        }
    }

    for (index, insn) in program.insns.iter().enumerate() {
        match &insn.fx.flow {
            Flow::Int(Some(0x80)) => out.push(
                Diagnostic::new(
                    &INT80_IN_64BIT,
                    insn.span,
                    "`int 0x80` is the 32-bit system call interface",
                )
                .note(
                    "in a 64-bit program it truncates pointers to 32 bits and uses the \
                     32-bit call numbers",
                )
                .help(
                    "use `syscall`: number in `rax`, arguments in `rdi`, `rsi`, `rdx`, `r10`, \
                     `r8`, `r9`",
                ),
            ),
            Flow::Jump(Target::Label(name)) => {
                let target = program.symbols.get(name).and_then(|symbol| symbol.insn);
                let next_in_run =
                    program.insns.get(index + 1).is_some_and(|next| next.run == insn.run);
                if target == Some(index + 1) && next_in_run {
                    let mut diagnostic = Diagnostic::new(
                        &REDUNDANT_JUMP,
                        insn.span,
                        "this `jmp` goes to the very next instruction",
                    )
                    .help("remove it: execution continues there anyway");
                    if program.lines[insn.line].label.is_none() {
                        diagnostic = diagnostic.delete_line("remove the jump", insn.line);
                    }
                    out.push(diagnostic);
                }
            }
            _ => {}
        }
    }
}
