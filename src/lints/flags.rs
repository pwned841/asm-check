//! Comparisons whose flags are lost before anything tests them.

use super::Context;
use crate::diagnostic::{Diagnostic, Group, Lint};
use crate::insn::{Flags, Flow};
use crate::program::Insn;

pub static FLAGS_CLOBBERED: Lint = Lint {
    name: "flags-clobbered",
    group: Group::Suspicious,
    summary: "a conditional jump that tests flags an instruction overwrote after the `cmp`",
};

pub static UNUSED_COMPARISON: Lint = Lint {
    name: "unused-comparison",
    group: Group::Suspicious,
    summary: "a `cmp` or `test` whose result nothing reads",
};

/// A `cmp`/`test` whose flags have not all been overwritten or consumed.
struct Comparison {
    index: usize,
    /// Flags that still hold the comparison's result.
    live: Flags,
    used: bool,
    /// The first instruction that overwrote some of its flags.
    overwriter: Option<usize>,
}

pub fn check(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;
    for block in &program.blocks {
        let mut current: Option<Comparison> = None;
        for index in block.start..block.end {
            let insn = &program.insns[index];
            let fx = &insn.fx;
            if !fx.known {
                current = None;
                continue;
            }

            if !fx.flags_read.is_empty()
                && let Some(comparison) = &mut current
            {
                if fx.flags_read.intersects(comparison.live) {
                    comparison.used = true;
                } else if let (false, Some(overwriter)) = (comparison.used, comparison.overwriter) {
                    out.push(clobbered(cx, insn, comparison.index, overwriter));
                    current = None;
                }
            }

            // Calls and system calls leave the flags in an unspecified state.
            let written = match fx.flow {
                Flow::Call(_) | Flow::Syscall | Flow::Int(_) => Flags::ALL,
                _ => fx.flags_written,
            };
            if written.is_empty() {
                continue;
            }
            if let Some(comparison) = &mut current {
                let remaining = comparison.live.difference(written);
                if remaining != comparison.live {
                    comparison.overwriter.get_or_insert(index);
                }
                comparison.live = remaining;
            }
            if matches!(insn.ins.mnemonic.as_str(), "cmp" | "test") {
                finish(cx, current.take(), out);
                current =
                    Some(Comparison { index, live: Flags::ALL, used: false, overwriter: None });
            }
        }
        finish(cx, current, out);
    }
}

/// Reports a comparison that was fully overwritten without being read.
/// One that still has live flags may be tested in a later block.
fn finish(cx: &Context, comparison: Option<Comparison>, out: &mut Vec<Diagnostic>) {
    let Some(comparison) = comparison else { return };
    let Some(overwriter) = comparison.overwriter else { return };
    if comparison.used || !comparison.live.is_empty() {
        return;
    }
    let program = cx.program;
    let (insn, by) = (&program.insns[comparison.index], &program.insns[overwriter]);
    let mnemonic = &insn.ins.mnemonic;
    out.push(
        Diagnostic::new(
            &UNUSED_COMPARISON,
            insn.span,
            format!("the result of this `{mnemonic}` is never used"),
        )
        .note_at(
            by.span,
            format!(
                "`{}` on line {} overwrites the flags before anything tests them",
                by.ins.mnemonic,
                by.line + 1
            ),
        )
        .help("remove it, or move it right before the jump that should test it"),
    );
}

fn clobbered(cx: &Context, reader: &Insn, comparison: usize, overwriter: usize) -> Diagnostic {
    let program = cx.program;
    let (cmp, by) = (&program.insns[comparison], &program.insns[overwriter]);
    Diagnostic::new(
        &FLAGS_CLOBBERED,
        reader.span,
        format!(
            "`{}` tests the flags of `{}` (line {}), not those of the `{}` on line {}",
            reader.ins.mnemonic,
            by.ins.mnemonic,
            by.line + 1,
            cmp.ins.mnemonic,
            cmp.line + 1
        ),
    )
    .note_at(by.span, format!("`{}` overwrites the flags here", cx.code(by)))
    .help(format!(
        "move the `{}` right before `{}`; `mov` and `lea` can sit in between, they leave \
         the flags alone",
        cmp.ins.mnemonic, reader.ins.mnemonic
    ))
}
