//! Comparisons whose flags are lost before anything tests them.

use super::Context;
use crate::dataflow::{self, Analysis};
use crate::diagnostic::{Diagnostic, Group, Lint};
use crate::insn::{Flags, Flow};
use crate::program::{Function, Insn, Program};

pub static UNDEFINED_FLAGS: Lint = Lint {
    name: "undefined-flags",
    group: Group::Suspicious,
    summary: "a conditional jump with no `cmp`, `test` or arithmetic before it, such as \
              `mov rax, [x]` followed by `jz`",
};

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
    dataflow::run(program, &FlagsSet, out);
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

            // A call leaves the flags unspecified: `undefined-flags` reports
            // whatever tests them next.
            if matches!(fx.flow, Flow::Call(_) | Flow::Int(_)) {
                current = None;
                continue;
            }
            let written = fx.flags_written;
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

// --- undefined-flags -------------------------------------------------------

/// Why the flags hold nothing meaningful. Lines are 0-based.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Unset {
    /// Nothing has set them since the program started.
    Entry,
    /// A call on this line left them unspecified.
    Call(usize),
}

/// `None` when some instruction has set the flags.
type FlagState = Option<Unset>;

struct FlagsSet;

impl Analysis for FlagsSet {
    type State = FlagState;

    fn entry(&self, _: &Program, function: &Function) -> FlagState {
        // A function may be handed flags by its caller; the entry point is not.
        function.entry_point.then_some(Unset::Entry)
    }

    fn unknown(&self) -> FlagState {
        None
    }

    /// Unset only when unset on every path.
    fn join(&self, a: &FlagState, b: &FlagState) -> FlagState {
        if b.is_some() { *a } else { None }
    }

    fn transfer(
        &self,
        program: &Program,
        index: usize,
        state: &mut FlagState,
        out: Option<&mut Vec<Diagnostic>>,
    ) {
        let insn = &program.insns[index];
        let fx = &insn.fx;
        if !fx.known {
            *state = None;
            return;
        }
        if !fx.flags_read.is_empty()
            && let Some(why) = state.take()
            && let Some(out) = out
        {
            out.push(undefined(program, index, why));
        }
        if let Flow::Call(_) = fx.flow {
            *state = Some(Unset::Call(insn.line));
        } else if !fx.flags_written.is_empty() {
            *state = None;
        }
    }
}

fn undefined(program: &Program, index: usize, why: Unset) -> Diagnostic {
    let insn = &program.insns[index];
    let mnemonic = &insn.ins.mnemonic;
    let since = match why {
        Unset::Entry => "the program started".to_string(),
        Unset::Call(line) => format!("the call on line {}", line + 1),
    };
    let mut diagnostic = Diagnostic::new(
        &UNDEFINED_FLAGS,
        insn.span,
        format!("`{mnemonic}` tests the flags, but nothing has set them since {since}"),
    );
    // The usual cause: expecting a data move to set the zero flag.
    let block = &program.blocks[insn.block];
    if index > block.start {
        let previous = &program.insns[index - 1].ins.mnemonic;
        if matches!(
            previous.as_str(),
            "mov" | "movzx" | "movsx" | "lea" | "pop" | "push" | "xchg" | "not"
        ) {
            diagnostic = diagnostic.note(format!("`{previous}` does not change the flags"));
        }
    }
    diagnostic.help(format!("put a `cmp` or a `test` right before `{mnemonic}`"))
}
