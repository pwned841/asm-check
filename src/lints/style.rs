//! Instructions that work but have a shorter or clearer form.

use super::Context;
use crate::ast::Operand;
use crate::diagnostic::{Diagnostic, Group, Lint};
use crate::insn::{Flags, Flow};
use crate::program::Program;
use crate::reg::Width;

pub static MOV_ZERO: Lint = Lint {
    name: "mov-zero",
    group: Group::Style,
    summary: "`mov reg, 0`, which `xor reg, reg` does in fewer bytes",
};

pub static CMP_ZERO: Lint = Lint {
    name: "cmp-zero",
    group: Group::Style,
    summary: "`cmp reg, 0`, which `test reg, reg` does in fewer bytes",
};

pub static SELF_MOVE: Lint = Lint {
    name: "self-move",
    group: Group::Style,
    summary: "`mov reg, reg` with the same register on both sides",
};

pub static NOOP_ARITHMETIC: Lint = Lint {
    name: "noop-arithmetic",
    group: Group::Style,
    summary: "arithmetic that leaves its operand unchanged, such as `add rax, 0`",
};

pub static NEEDLESS_REX: Lint = Lint {
    name: "needless-rex",
    group: Group::Pedantic,
    summary: "`xor r64, r64`, which the 32-bit form does in one byte less",
};

pub fn check(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;
    for (index, insn) in program.insns.iter().enumerate() {
        if cx.uses_define(insn) {
            continue;
        }
        let mnemonic = insn.ins.mnemonic.as_str();
        let (Some(first), Some(second)) = (insn.ins.operands.first(), insn.ins.operands.get(1))
        else {
            continue;
        };
        let Some(dest) = first.as_reg() else { continue };
        let constant = second.constant().filter(|_| second.size.is_none());
        let same_register = second.as_reg() == Some(dest);
        let small = dest.id.name32();

        match mnemonic {
            "mov" if constant == Some(0) && dest.width.is_full() => {
                if !flags_live_after(program, index, 3) {
                    let replacement = format!("xor {small}, {small}");
                    out.push(
                        Diagnostic::new(
                            &MOV_ZERO,
                            insn.span,
                            format!("`{}` can be written `{replacement}`", cx.code(insn)),
                        )
                        .note(
                            "`xor` is the shorter, recognised way to zero a register; \
                             unlike `mov` it changes the flags, which nothing reads here",
                        )
                        .replace(
                            format!("use `{replacement}`"),
                            insn.span,
                            replacement,
                        ),
                    );
                }
            }
            "mov" if same_register && dest.width != Width::W32 => out.push(self_move(cx, index)),
            "xor" if same_register && dest.width == Width::W64 && dest.id.index() < 8 => {
                let replacement = format!("xor {small}, {small}");
                out.push(
                    Diagnostic::new(
                        &NEEDLESS_REX,
                        insn.span,
                        format!("`{replacement}` zeroes all of `{dest}` in one byte less"),
                    )
                    .note("writing a 32-bit register clears the upper half of the 64-bit one")
                    .replace(
                        format!("use `{replacement}`"),
                        insn.span,
                        replacement,
                    ),
                );
            }
            "cmp" if constant == Some(0) => {
                let replacement = format!("test {dest}, {dest}");
                out.push(
                    Diagnostic::new(
                        &CMP_ZERO,
                        insn.span,
                        format!("`{}` can be written `{replacement}`", cx.code(insn)),
                    )
                    .note("`test` sets the same flags for a comparison with zero and is shorter")
                    .replace(
                        format!("use `{replacement}`"),
                        insn.span,
                        replacement,
                    ),
                );
            }
            "add" | "sub" | "or" | "xor" | "shl" | "sal" | "shr" | "sar" | "rol" | "ror"
                if constant == Some(0) =>
            {
                out.push(noop(cx, index, first));
            }
            "and" if constant == Some(-1) => out.push(noop(cx, index, first)),
            "imul" if constant == Some(1) && insn.ins.operands.len() == 2 => {
                out.push(noop(cx, index, first));
            }
            _ => {}
        }
    }
}

fn self_move(cx: &Context, index: usize) -> Diagnostic {
    let insn = &cx.program.insns[index];
    let mut diagnostic =
        Diagnostic::new(&SELF_MOVE, insn.span, format!("`{}` does nothing", cx.code(insn)))
            .help("remove it, or check that both operands are the ones you meant");
    if cx.program.lines[insn.line].label.is_none() {
        diagnostic = diagnostic.delete_line("remove the instruction", insn.line);
    }
    diagnostic
}

fn noop(cx: &Context, index: usize, dest: &Operand) -> Diagnostic {
    let insn = &cx.program.insns[index];
    let dest = cx.text(dest.span);
    Diagnostic::new(
        &NOOP_ARITHMETIC,
        insn.span,
        format!("`{}` leaves `{dest}` unchanged", cx.code(insn)),
    )
    .help(format!("remove it, or use `test {dest}, {dest}` if the goal is to set the flags"))
}

/// Whether something may read the flags as they are right after the
/// instruction at `index`. Follows successors up to `depth` blocks away and
/// answers `true` when it cannot tell.
fn flags_live_after(program: &Program, index: usize, depth: usize) -> bool {
    let block = &program.blocks[program.insns[index].block];
    for insn in &program.insns[index + 1..block.end] {
        if !insn.fx.known || !insn.fx.flags_read.is_empty() {
            return true;
        }
        let dies = matches!(insn.fx.flow, Flow::Call(_) | Flow::Syscall | Flow::Int(_) | Flow::Ret)
            || insn.fx.flags_written == Flags::ALL;
        if dies {
            return false;
        }
    }
    if block.succs.is_empty() {
        return false;
    }
    depth == 0
        || block
            .succs
            .iter()
            .any(|succ| flags_live_from(program, program.blocks[*succ].start, depth - 1))
}

/// Same question, starting with the instruction at `index` itself.
fn flags_live_from(program: &Program, index: usize, depth: usize) -> bool {
    let insn = &program.insns[index];
    if !insn.fx.known || !insn.fx.flags_read.is_empty() {
        return true;
    }
    let dies = matches!(insn.fx.flow, Flow::Call(_) | Flow::Syscall | Flow::Int(_) | Flow::Ret)
        || insn.fx.flags_written == Flags::ALL;
    !dies && flags_live_after(program, index, depth)
}
