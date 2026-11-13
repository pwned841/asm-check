//! Instructions that work but have a shorter or clearer form.

use super::Context;
use crate::ast::{Operand, OperandKind};
use crate::diagnostic::{Diagnostic, Edit, Group, Lint};
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

pub static LEA_AS_MOV: Lint = Lint {
    name: "lea-as-mov",
    group: Group::Style,
    summary: "`lea reg, [other]` with a bare register, which is just `mov reg, other`",
};

pub static REDUNDANT_TEST: Lint = Lint {
    name: "redundant-test",
    group: Group::Style,
    summary: "a `test reg, reg` right after arithmetic that already set the flags for `reg`",
};

pub static REDUNDANT_PUSH_POP: Lint = Lint {
    name: "redundant-push-pop",
    group: Group::Style,
    summary: "`push reg` immediately followed by `pop reg`",
};

pub static REDUNDANT_MOVE: Lint = Lint {
    name: "redundant-move",
    group: Group::Style,
    summary: "a `mov` that copies back what the previous `mov` just copied",
};

pub static SLOW_LOOP: Lint = Lint {
    name: "slow-loop",
    group: Group::Pedantic,
    summary: "the `loop` instruction, slower than `dec` and `jnz` on current processors",
};

pub static ABSOLUTE_ADDRESS: Lint = Lint {
    name: "absolute-address",
    group: Group::Pedantic,
    summary: "`[label]` without `rel`, which is not position-independent",
};

/// Lints that look at an instruction and its neighbour.
fn neighbours(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;
    for (index, insn) in program.insns.iter().enumerate() {
        if cx.uses_define(insn) {
            continue;
        }
        let mnemonic = insn.ins.mnemonic.as_str();
        let operands = &insn.ins.operands;
        let deletable = |line: usize| program.lines[line].label.is_none();
        // The instruction right before, when nothing can jump in between.
        let previous = (index > 0)
            .then(|| &program.insns[index - 1])
            .filter(|previous| insn.labels.is_empty() && previous.run == insn.run);
        let normalise = |operand: &Operand| {
            cx.text(operand.span).replace(char::is_whitespace, "").to_lowercase()
        };

        if mnemonic == "loop" {
            let mut diagnostic =
                Diagnostic::new(&SLOW_LOOP, insn.span, "`loop` is slow on current processors");
            if let Some(target) = operands.first() {
                // Not safe: `dec` changes the flags, `loop` does not.
                diagnostic = diagnostic
                    .replace(
                        "use `dec rcx` and `jnz`",
                        insn.span,
                        format!("dec rcx\njnz {}", cx.text(target.span)),
                    )
                    .maybe_incorrect();
            }
            out.push(diagnostic);
        }

        for operand in operands {
            if let OperandKind::Mem(mem) = &operand.kind
                && mem.direct
                && !mem.relative
                && !program.default_rel
            {
                let text = cx.text(operand.span);
                let mut diagnostic = Diagnostic::new(
                    &ABSOLUTE_ADDRESS,
                    operand.span,
                    format!("absolute address `{text}`"),
                )
                .label("not position-independent");
                if let Some(bracket) = text.find('[') {
                    let at = operand.span.start + bracket + 1;
                    diagnostic = diagnostic.replace(
                        "address it relative to `rip`",
                        crate::span::Span::new(operand.span.line, at, at),
                        "rel ",
                    );
                }
                out.push(diagnostic);
            }
        }

        if mnemonic == "lea"
            && let (Some(dest), Some(source)) = (operands.first(), operands.get(1))
            && let (Some(to), OperandKind::Mem(mem)) = (dest.as_reg(), &source.kind)
            && let [from] = mem.regs.as_slice()
            && to.width == from.width
            && normalise(source) == format!("[{from}]")
        {
            let replacement = format!("mov {to}, {from}");
            out.push(
                Diagnostic::new(
                    &LEA_AS_MOV,
                    insn.span,
                    format!("`{}` is just `{replacement}`", cx.code(insn)),
                )
                .replace(format!("use `{replacement}`"), insn.span, replacement),
            );
        }

        let Some(previous) = previous else { continue };
        let before = previous.ins.mnemonic.as_str();

        if mnemonic == "pop"
            && before == "push"
            && let (Some(a), Some(b)) = (previous.ins.operands.first(), operands.first())
            && a.as_reg().is_some()
            && a.as_reg() == b.as_reg()
        {
            let mut diagnostic = Diagnostic::new(
                &REDUNDANT_PUSH_POP,
                previous.span,
                format!("`{}` is undone by the next instruction", cx.code(previous)),
            )
            .note_at(insn.span, "popped straight back");
            if deletable(previous.line) && deletable(insn.line) {
                diagnostic = diagnostic.fix(
                    "remove both",
                    vec![Edit::DeleteLine(previous.line), Edit::DeleteLine(insn.line)],
                );
            }
            out.push(diagnostic);
        }

        if mnemonic == "mov"
            && before == "mov"
            && let ([a, b], [c, d]) = (previous.ins.operands.as_slice(), operands.as_slice())
            && normalise(a) == normalise(d)
            && normalise(b) == normalise(c)
            && !(a.is_mem() && b.is_mem())
        {
            let mut diagnostic = Diagnostic::new(
                &REDUNDANT_MOVE,
                insn.span,
                format!("`{}` copies the same value back", cx.code(insn)),
            )
            .note_at(previous.span, "already copied here");
            if deletable(insn.line) {
                diagnostic = diagnostic.delete_line("remove the redundant move", insn.line);
            }
            out.push(diagnostic);
        }

        // `dec rcx` / `test rcx, rcx` / `jz`: the arithmetic already set ZF and SF.
        let tested = match (mnemonic, operands.as_slice()) {
            ("test", [a, b]) if a.as_reg().is_some() && a.as_reg() == b.as_reg() => a.as_reg(),
            ("cmp", [a, b]) if b.constant() == Some(0) => a.as_reg(),
            _ => None,
        };
        let sets_result_flags =
            matches!(before, "add" | "sub" | "and" | "or" | "xor" | "neg" | "inc" | "dec");
        if let Some(reg) = tested
            && sets_result_flags
            && !previous.fx.zeroing
            && previous.fx.dest == Some(reg)
        {
            // CF and OF differ between the arithmetic and a `test`: only
            // suggest when what follows looks at the result flags alone.
            let result = Flags::ZF.union(Flags::SF).union(Flags::PF);
            let block = &program.blocks[insn.block];
            let reader = program.insns[index + 1..block.end].iter().find(|later| {
                !later.fx.flags_read.is_empty() || !later.fx.flags_written.is_empty()
            });
            if reader.is_some_and(|later| {
                !later.fx.flags_read.is_empty() && later.fx.flags_read.difference(result).is_empty()
            }) {
                let mut diagnostic = Diagnostic::new(
                    &REDUNDANT_TEST,
                    insn.span,
                    format!(
                        "`{}` already set the flags this `{mnemonic}` computes",
                        cx.code(previous)
                    ),
                )
                .help("remove it and branch on the flags of the arithmetic");
                if deletable(insn.line) {
                    diagnostic =
                        diagnostic.delete_line(format!("remove the `{mnemonic}`"), insn.line);
                }
                out.push(diagnostic);
            }
        }
    }
}

pub fn check(cx: &Context, out: &mut Vec<Diagnostic>) {
    neighbours(cx, out);
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
