//! Control flow: running off the end of a function, dead code, returning
//! from the entry point.

use super::Context;
use crate::diagnostic::{Diagnostic, Edit, Group, Lint};
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

pub static JUMP_OVER_JUMP: Lint = Lint {
    name: "jump-over-jump",
    group: Group::Style,
    summary: "a conditional jump that only skips a `jmp`, where the opposite condition would do",
};

pub static JUMP_TO_JUMP: Lint = Lint {
    name: "jump-to-jump",
    group: Group::Style,
    summary: "a jump to a label that immediately jumps somewhere else",
};

pub static INFINITE_LOOP: Lint = Lint {
    name: "infinite-loop",
    group: Group::Suspicious,
    summary: "a loop whose condition tests values the loop never changes",
};

/// A block that jumps back to its own start on a comparison of things it
/// never modifies: once taken, the jump is always taken.
fn infinite_loops(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;
    for (index, block) in program.blocks.iter().enumerate() {
        let jump = &program.insns[block.end - 1];
        if !matches!(jump.fx.flow, Flow::CondJump(_)) || !block.succs.contains(&index) {
            continue;
        }
        let body = &program.insns[block.start..block.end - 1];
        // The comparison the jump depends on must be the last flag writer.
        let Some(position) = body.iter().rposition(|insn| !insn.fx.flags_written.is_empty()) else {
            continue;
        };
        let test = &body[position];
        if !matches!(test.ins.mnemonic.as_str(), "cmp" | "test") {
            continue;
        }
        let compares_memory = test.ins.operands.iter().any(|operand| operand.is_mem());
        let unchanged = body.iter().all(|insn| {
            insn.fx.known
                && insn.fx.flow == Flow::Next
                && insn.fx.writes.intersection(test.fx.reads).is_empty()
                // With a memory operand, any store could be the update.
                && !(compares_memory
                    && (insn.ins.operands.first().is_some_and(|operand| operand.is_mem())
                        && insn.ins.mnemonic != "cmp"
                        && insn.ins.mnemonic != "test"
                        || insn.ins.operands.is_empty()))
        });
        if unchanged {
            out.push(
                Diagnostic::new(&INFINITE_LOOP, jump.span, "this loop never changes what it tests")
                    .label("once taken, always taken")
                    .note_at(test.span, "nothing in the loop modifies these operands"),
            );
        }
    }
}

/// The condition code that is true exactly when `code` is false.
fn opposite(code: &str) -> Option<&'static str> {
    const PAIRS: &[(&str, &str)] = &[
        ("e", "ne"),
        ("z", "nz"),
        ("l", "ge"),
        ("le", "g"),
        ("b", "ae"),
        ("be", "a"),
        ("s", "ns"),
        ("o", "no"),
        ("p", "np"),
        ("c", "nc"),
    ];
    PAIRS.iter().find_map(|(yes, no)| {
        if *yes == code {
            Some(*no)
        } else if *no == code {
            Some(*yes)
        } else {
            None
        }
    })
}

/// How a function should end: the entry point exits, the others return.
fn ending(entry_point: bool) -> (&'static str, &'static str) {
    if entry_point {
        ("exit the program here", "mov eax, 60\nxor edi, edi\nsyscall")
    } else {
        ("return here", "ret")
    }
}

pub fn check(cx: &Context, out: &mut Vec<Diagnostic>) {
    infinite_loops(cx, out);
    let program = cx.program;

    for function in &program.functions {
        for &(block, next) in &function.falls_into {
            let last = &program.insns[program.blocks[block].end - 1];
            let next = &program.functions[next];
            let (title, text) = ending(function.entry_point);
            out.push(
                Diagnostic::new(
                    &FALLTHROUGH,
                    last.span,
                    format!("`{}` falls through into `{}`", function.name, next.name),
                )
                .label("nothing stops execution after this")
                .fix(title, vec![Edit::InsertAfter { line: last.line, text: text.to_string() }])
                .maybe_incorrect(),
            );
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
                                format!("`ret` in `{}`", function.name),
                            )
                            .label("there is no return address: the stack holds `argc`")
                            .replace(
                                "exit with the system call",
                                insn.span,
                                "mov eax, 60\nxor edi, edi\nsyscall",
                            )
                            .maybe_incorrect(),
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
                let entry = program.functions_of(index).any(|function| function.entry_point);
                let (title, text) = ending(entry);
                let mut diagnostic = Diagnostic::new(
                    &FALLTHROUGH,
                    last.span,
                    "execution runs past the end of the code",
                )
                .label("the processor goes on with whatever bytes follow");
                if let (Flow::Syscall, Some(number)) = (&last.fx.flow, number) {
                    let name = crate::syscalls::lookup(number).map_or("it", |syscall| syscall.name);
                    diagnostic = diagnostic
                        .note(format!("system call {number} is not `exit`: {name} returns"));
                }
                out.push(
                    diagnostic
                        .fix(
                            title,
                            vec![Edit::InsertAfter { line: last.line, text: text.to_string() }],
                        )
                        .maybe_incorrect(),
                );
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
                        .label("never executed")
                        .note_at(before.span, "control does not continue past this"),
                );
            }
        }
    }

    for (index, insn) in program.insns.iter().enumerate() {
        let target_of = |name: &String| program.symbols.get(name).and_then(|symbol| symbol.insn);
        let deletable = |line: usize| program.lines[line].label.is_none();

        // jcc .skip / jmp target / .skip:
        if let Flow::CondJump(Target::Label(skip)) = &insn.fx.flow
            && let Some(("j", code)) = crate::mnemonic::condition(&insn.ins.mnemonic)
            && let Some(next) = program.insns.get(index + 1)
            && next.run == insn.run
            && next.labels.is_empty()
            && let Flow::Jump(Target::Label(_)) = &next.fx.flow
            && target_of(skip) == Some(index + 2)
            && let Some(operand) = next.ins.operands.first()
        {
            let mut diagnostic = Diagnostic::new(
                &JUMP_OVER_JUMP,
                insn.span,
                format!("`{}` only skips a `jmp`", insn.ins.mnemonic),
            );
            if let Some(opposite) = opposite(code)
                && deletable(next.line)
            {
                let direct = format!("j{opposite} {}", cx.text(operand.span));
                diagnostic = diagnostic.fix(
                    format!("use `{direct}`"),
                    vec![
                        Edit::Replace { span: insn.span, text: direct },
                        Edit::DeleteLine(next.line),
                    ],
                );
            }
            out.push(diagnostic);
        }

        // A jump whose target is itself an unconditional jump.
        if let Flow::Jump(Target::Label(name)) | Flow::CondJump(Target::Label(name)) = &insn.fx.flow
            && let Some(landing) = target_of(name)
            && landing != index
            && let Flow::Jump(Target::Label(onward)) = &program.insns[landing].fx.flow
            && onward != name
            && let (Some(from), Some(to)) =
                (insn.ins.operands.first(), program.insns[landing].ins.operands.first())
        {
            let (from_text, to_text) = (cx.text(from.span), cx.text(to.span));
            let mut diagnostic = Diagnostic::new(
                &JUMP_TO_JUMP,
                from.span,
                format!("`{from_text}` only jumps on to `{to_text}`"),
            )
            .note_at(program.insns[landing].span, "the jump lands here");
            // A local label means the same thing only inside its own scope.
            let scope = |name: &str| name.split('.').next().unwrap_or("").to_string();
            if !to_text.starts_with('.') || scope(name) == scope(onward) {
                diagnostic =
                    diagnostic.replace(format!("jump to `{to_text}` directly"), from.span, to_text);
            }
            out.push(diagnostic);
        }

        match &insn.fx.flow {
            Flow::Int(Some(0x80)) => out.push(
                Diagnostic::new(&INT80_IN_64BIT, insn.span, "`int 0x80` in 64-bit code")
                    .label("the 32-bit system call interface")
                    .note("it truncates pointers to 32 bits and uses different call numbers")
                    .help("use `syscall`, with the number in `rax` and arguments in `rdi`, `rsi`, `rdx`"),
            ),
            Flow::Jump(Target::Label(name)) => {
                let next_in_run =
                    program.insns.get(index + 1).is_some_and(|next| next.run == insn.run);
                if target_of(name) == Some(index + 1) && next_in_run {
                    let mut diagnostic = Diagnostic::new(
                        &REDUNDANT_JUMP,
                        insn.span,
                        "`jmp` to the next instruction",
                    );
                    if deletable(insn.line) {
                        diagnostic = diagnostic.delete_line("remove the jump", insn.line);
                    }
                    out.push(diagnostic);
                }
            }
            _ => {}
        }
    }
}
