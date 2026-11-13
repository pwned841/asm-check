//! Comparisons whose flags are lost before anything tests them.

use super::Context;
use crate::dataflow::{self, Analysis};
use crate::diagnostic::{Diagnostic, Edit, Group, Lint};
use crate::insn::{Flags, Flow};
use crate::program::{Function, Insn, Program};

pub static CONSTANT_COMPARISON: Lint = Lint {
    name: "constant-comparison",
    group: Group::Suspicious,
    summary: "a comparison whose result never changes, such as `cmp rax, rax` or `test rax, 0`",
};

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
    for insn in &program.insns {
        let (Some(a), Some(b)) = (insn.ins.operands.first(), insn.ins.operands.get(1)) else {
            continue;
        };
        let code = cx.code(insn);
        match insn.ins.mnemonic.as_str() {
            "cmp" if a.as_reg().is_some() && a.as_reg() == b.as_reg() => {
                let test = format!("test {0}, {0}", cx.text(a.span));
                out.push(
                    Diagnostic::new(
                        &CONSTANT_COMPARISON,
                        insn.span,
                        format!("`{code}` is always equal"),
                    )
                    .label("a register compared with itself")
                    .replace(format!("check for zero with `{test}`"), insn.span, test)
                    .maybe_incorrect(),
                );
            }
            "test" if b.constant() == Some(0) => out.push(
                Diagnostic::new(
                    &CONSTANT_COMPARISON,
                    insn.span,
                    format!("`{code}` is always zero"),
                )
                .label("`test` ANDs its operands")
                .help("test against the bits you care about, or the operand against itself"),
            ),
            _ => {}
        }
    }
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
    let mut diagnostic = Diagnostic::new(
        &UNUSED_COMPARISON,
        insn.span,
        format!("this `{}` has no effect", insn.ins.mnemonic),
    )
    .label("nothing tests its result")
    .note_at(by.span, format!("`{}` overwrites the flags first", by.ins.mnemonic));
    if program.lines[insn.line].label.is_none() {
        diagnostic = diagnostic.delete_line("remove it", insn.line).maybe_incorrect();
    }
    out.push(diagnostic);
}

fn clobbered(cx: &Context, reader: &Insn, comparison: usize, overwriter: usize) -> Diagnostic {
    let program = cx.program;
    let (cmp, by) = (&program.insns[comparison], &program.insns[overwriter]);
    let (jump, test) = (&reader.ins.mnemonic, &cmp.ins.mnemonic);
    let mut diagnostic = Diagnostic::new(
        &FLAGS_CLOBBERED,
        reader.span,
        format!("`{jump}` tests `{}`, not the `{test}`", by.ins.mnemonic),
    )
    .label("reads the flags of the instruction above")
    .note_at(cmp.span, "this result is lost")
    .note_at(by.span, "overwrites the flags");

    // Moving the comparison down is safe when nothing in between changes
    // what it compares.
    let reader_index = program.at_line(reader.line).unwrap_or(comparison);
    let between = &program.insns[comparison + 1..reader_index.max(comparison + 1)];
    let stable = cmp.ins.operands.iter().all(|operand| !operand.is_mem())
        && between
            .iter()
            .all(|insn| insn.fx.known && insn.fx.writes.intersection(cmp.fx.reads).is_empty());
    if stable && program.lines[cmp.line].label.is_none() {
        diagnostic = diagnostic
            .fix(
                format!("move the `{test}` next to `{jump}`"),
                vec![
                    Edit::DeleteLine(cmp.line),
                    Edit::InsertBefore { line: reader.line, text: cx.code(cmp).to_string() },
                ],
            )
            .maybe_incorrect();
    }
    diagnostic
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
    let mut diagnostic = Diagnostic::new(
        &UNDEFINED_FLAGS,
        insn.span,
        format!("`{mnemonic}` tests flags that nothing set"),
    )
    .label("no `cmp`, `test` or arithmetic before it");
    if let Unset::Call(line) = why
        && let Some(call) = program.at_line(line)
    {
        diagnostic =
            diagnostic.note_at(program.insns[call].span, "a call leaves the flags undefined");
    }
    // The usual cause: expecting a data move to set the zero flag. Then
    // the value just moved is what was meant to be tested.
    let block = &program.blocks[insn.block];
    if index > block.start {
        let previous = &program.insns[index - 1];
        let moved =
            matches!(previous.ins.mnemonic.as_str(), "mov" | "movzx" | "movsx" | "lea" | "pop");
        if let (true, Some(dest)) = (moved, previous.fx.dest) {
            let test = format!("test {dest}, {dest}");
            return diagnostic
                .note_at(
                    previous.span,
                    format!("`{}` does not change the flags", previous.ins.mnemonic),
                )
                .insert_before(format!("test `{dest}` first"), insn.line, test)
                .maybe_incorrect();
        }
    }
    diagnostic.help(format!("put a `cmp` or a `test` right before `{mnemonic}`"))
}
