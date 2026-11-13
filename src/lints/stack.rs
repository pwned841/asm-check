//! Stack discipline: balance at `ret`, alignment at `call`.

use super::Context;
use crate::ast::Operand;
use crate::dataflow::{self, Analysis};
use crate::diagnostic::{Diagnostic, Edit, Group, Lint};
use crate::insn::{Flow, Target};
use crate::program::{Callee, Function, Program};
use crate::reg::RegId;

pub static UNBALANCED_STACK: Lint = Lint {
    name: "unbalanced-stack",
    group: Group::Correctness,
    summary: "`ret` reached with pushes and pops that do not match",
};

pub static MISMATCHED_POP: Lint = Lint {
    name: "mismatched-pop",
    group: Group::Suspicious,
    summary: "registers popped in the same order they were pushed, which swaps them",
};

pub static ODD_STACK_ADJUSTMENT: Lint = Lint {
    name: "odd-stack-adjustment",
    group: Group::Suspicious,
    summary: "`rsp` moved by an amount that is not a multiple of 8",
};

pub static MISALIGNED_CALL: Lint = Lint {
    name: "misaligned-call",
    group: Group::Suspicious,
    summary: "external function called with a stack that is not 16-byte aligned",
};

pub fn check(cx: &Context, out: &mut Vec<Diagnostic>) {
    dataflow::run(cx.program, &Stack, out);
}

/// What is known about `rsp`. `None` means "not the same on every path".
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Position {
    /// Bytes pushed since the function was entered.
    depth: Option<i64>,
    /// `rsp` modulo 16.
    align: Option<i64>,
}

impl Position {
    /// Moves `rsp` down by `bytes` (a push), or up when negative.
    fn grow(&mut self, bytes: i64) {
        self.depth = self.depth.map(|depth| depth + bytes);
        self.align = self.align.map(|align| (align - bytes).rem_euclid(16));
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
struct State {
    rsp: Position,
    /// Where `rsp` was when it was copied to `rbp`, while `rbp` still holds it.
    frame: Option<Position>,
    /// What each `push` since the function entry put on the stack: the
    /// register it came from, when it is one. `None` once `rsp` has moved
    /// in a way pushes and pops do not explain.
    pushed: Option<Vec<Option<RegId>>>,
}

struct Stack;

impl Analysis for Stack {
    type State = State;

    fn entry(&self, _: &Program, function: &Function) -> State {
        // The kernel starts a process with an aligned stack; a `call`
        // pushes an 8-byte return address on top of an aligned one.
        let align = if function.entry_point { 0 } else { 8 };
        State {
            rsp: Position { depth: Some(0), align: Some(align) },
            frame: None,
            pushed: Some(Vec::new()),
        }
    }

    fn unknown(&self) -> State {
        State::default()
    }

    fn join(&self, a: &State, b: &State) -> State {
        let same = |x: Option<i64>, y: Option<i64>| if x == y { x } else { None };
        State {
            rsp: Position {
                depth: same(a.rsp.depth, b.rsp.depth),
                align: same(a.rsp.align, b.rsp.align),
            },
            frame: if a.frame == b.frame { a.frame } else { None },
            pushed: if a.pushed == b.pushed { a.pushed.clone() } else { None },
        }
    }

    fn transfer(
        &self,
        program: &Program,
        index: usize,
        state: &mut State,
        mut out: Option<&mut Vec<Diagnostic>>,
    ) {
        let insn = &program.insns[index];
        let operands = &insn.ins.operands;
        let reg = |i: usize| operands.get(i).and_then(Operand::as_reg).map(|reg| reg.id);
        let constant = operands.get(1).and_then(Operand::constant);
        let mut keeps_frame = false;
        let mnemonic = insn.ins.mnemonic.as_str();

        // Pushes and pops keep the model of the stack contents; anything
        // else that moves `rsp` invalidates it.
        match mnemonic {
            "push" | "pushf" | "pushfq" => {
                if let Some(stack) = &mut state.pushed {
                    stack.push(reg(0));
                }
            }
            "pop" | "popf" | "popfq" => {
                let top = state.pushed.as_mut().and_then(Vec::pop);
                match (top, reg(0)) {
                    (None, _) => state.pushed = None,
                    (Some(Some(pushed)), Some(popped)) => {
                        let waiting = |stack: &Vec<Option<RegId>>| stack.contains(&Some(popped));
                        if pushed != popped
                            && state.pushed.as_ref().is_some_and(waiting)
                            && let Some(out) = out.as_deref_mut()
                        {
                            out.push(mismatched_pop(insn, pushed, popped));
                        }
                    }
                    _ => {}
                }
            }
            _ if insn.fx.writes.contains(RegId::RSP) && !matches!(mnemonic, "call" | "ret") => {
                state.pushed = None;
            }
            _ => {}
        }
        if matches!(mnemonic, "sub" | "add")
            && reg(0) == Some(RegId::RSP)
            && let Some(bytes) = constant.filter(|bytes| bytes % 8 != 0)
            && let Some(out) = out.as_deref_mut()
        {
            let rounded = (bytes + 7) / 8 * 8;
            let mut diagnostic = Diagnostic::new(
                &ODD_STACK_ADJUSTMENT,
                insn.span,
                format!("`rsp` moved by {bytes} bytes"),
            )
            .label("not a multiple of 8, the size of a stack slot");
            if let Some(operand) = operands.get(1) {
                diagnostic = diagnostic
                    .replace(format!("round up to {rounded}"), operand.span, rounded.to_string())
                    .maybe_incorrect();
            }
            out.push(diagnostic);
        }

        match (mnemonic, reg(0), reg(1)) {
            ("push" | "pushf" | "pushfq", _, _) => state.rsp.grow(8),
            ("pop", Some(RegId::RSP), _) => state.rsp = Position::default(),
            ("pop" | "popf" | "popfq", _, _) => state.rsp.grow(-8),
            ("sub", Some(RegId::RSP), _) => match constant {
                Some(bytes) => state.rsp.grow(bytes),
                None => state.rsp = Position::default(),
            },
            ("add", Some(RegId::RSP), _) => match constant {
                Some(bytes) => state.rsp.grow(-bytes),
                None => state.rsp = Position::default(),
            },
            ("and", Some(RegId::RSP), _) => {
                let aligned = constant.is_some_and(|mask| mask & 0xF == 0);
                state.rsp = Position { depth: None, align: aligned.then_some(0) };
            }
            ("mov", Some(RegId::RBP), Some(RegId::RSP)) => {
                state.frame = Some(state.rsp);
                keeps_frame = true;
            }
            ("mov", Some(RegId::RSP), Some(RegId::RBP)) => {
                state.rsp = state.frame.unwrap_or_default();
                keeps_frame = true;
            }
            ("leave", _, _) => {
                state.rsp = state.frame.unwrap_or_default();
                state.rsp.grow(-8);
            }
            ("call", _, _) => {
                if let Some(out) = out {
                    check_call(program, index, state, out);
                }
            }
            ("ret" | "retn", _, _) => {
                if let Some(out) = out {
                    check_ret(program, index, state, out);
                }
            }
            _ => {
                if insn.fx.writes.contains(RegId::RSP) {
                    state.rsp = Position::default();
                }
            }
        }
        if insn.fx.writes.contains(RegId::RBP) && !keeps_frame {
            state.frame = None;
        }
    }
}

fn mismatched_pop(insn: &crate::program::Insn, pushed: RegId, popped: RegId) -> Diagnostic {
    let diagnostic = Diagnostic::new(
        &MISMATCHED_POP,
        insn.span,
        format!("`pop {popped}` receives the saved `{pushed}`"),
    )
    .label("pops must mirror the pushes, last one first");
    match insn.ins.operands.first() {
        Some(operand) => diagnostic
            .replace(format!("pop `{pushed}` here"), operand.span, pushed.name64())
            .maybe_incorrect(),
        None => diagnostic,
    }
}

fn check_ret(program: &Program, index: usize, state: &State, out: &mut Vec<Diagnostic>) {
    let insn = &program.insns[index];
    // `ret` in the entry point is wrong whatever the stack looks like;
    // `ret-in-start` reports it.
    if program.functions_of(insn.block).any(|function| function.entry_point) {
        return;
    }
    let Some(depth) = state.rsp.depth.filter(|depth| *depth != 0) else { return };
    if depth < 0 {
        out.push(
            Diagnostic::new(
                &UNBALANCED_STACK,
                insn.span,
                format!("`ret` after popping {} bytes too many", -depth),
            )
            .label("the return address is already gone"),
        );
        return;
    }
    // When every slot is a pushed register, the fix is to pop them back.
    let pops = state.pushed.as_ref().and_then(|stack| {
        let registers: Option<Vec<String>> =
            stack.iter().rev().map(|slot| slot.map(|id| format!("pop {id}"))).collect();
        registers.filter(|pops| pops.len() as i64 * 8 == depth)
    });
    let (title, text) = match pops {
        Some(pops) => ("pop what was pushed", pops.join("\n")),
        None => ("release the stack space", format!("add rsp, {depth}")),
    };
    out.push(
        Diagnostic::new(
            &UNBALANCED_STACK,
            insn.span,
            format!("`ret` with {depth} bytes left on the stack"),
        )
        .label("jumps to the top of the stack, not to the caller")
        .insert_before(title, insn.line, text)
        .maybe_incorrect(),
    );
}

fn check_call(program: &Program, index: usize, state: &State, out: &mut Vec<Diagnostic>) {
    let insn = &program.insns[index];
    let Flow::Call(target @ Target::Label(name)) = &insn.fx.flow else { return };
    // An undeclared name is `undefined-symbol`'s business, not a libc call.
    let declared = program.externs.contains_key(name) || program.opaque;
    if program.callee(target) != Callee::External || !declared || program.is_noreturn(index) {
        return;
    }
    let Some(off) = state.rsp.align.filter(|align| *align != 0) else { return };
    out.push(
        Diagnostic::new(
            &MISALIGNED_CALL,
            insn.span,
            format!("stack misaligned by {off} bytes at `call {name}`"),
        )
        .label("`rsp` must be a multiple of 16 here")
        .note("library functions that use SSE crash on a misaligned stack")
        .fix(
            "realign around the call",
            vec![
                Edit::InsertBefore { line: insn.line, text: "sub rsp, 8".to_string() },
                Edit::InsertAfter { line: insn.line, text: "add rsp, 8".to_string() },
            ],
        )
        .maybe_incorrect(),
    );
}
