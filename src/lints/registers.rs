//! Register values lost by accident: clobbered by a call or a system call,
//! not preserved for the caller, or overwritten before being read.

use super::Context;
use crate::abi;
use crate::ast::OperandKind;
use crate::dataflow::{self, Analysis};
use crate::diagnostic::{Diagnostic, Group, Lint};
use crate::insn::Flow;
use crate::program::{Callee, Function, Program};
use crate::reg::{Reg, RegId, RegSet};
use crate::span::Span;

pub static CLOBBERED_REGISTER: Lint = Lint {
    name: "clobbered-register",
    group: Group::Suspicious,
    summary: "a register read after a `syscall` or a call destroyed its value",
};

pub static CALLEE_SAVED_CLOBBERED: Lint = Lint {
    name: "callee-saved-clobbered",
    group: Group::Suspicious,
    summary: "an exported function changes `rbx`, `rbp` or `r12`-`r15` without restoring it",
};

pub static DEAD_STORE: Lint = Lint {
    name: "dead-store",
    group: Group::Suspicious,
    summary: "a register written twice in a row without the first value being read",
};

pub fn check(cx: &Context, out: &mut Vec<Diagnostic>) {
    dataflow::run(cx.program, &Clobbers, out);
    callee_saved(cx, out);
    dead_stores(cx, out);
}

// --- clobbered-register ----------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Culprit {
    Syscall,
    External,
    /// A local function, by index.
    Local(usize),
}

/// Where a register's value was destroyed. `line` is 0-based.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Clobber {
    line: usize,
    by: Culprit,
}

/// For each register, the clobber that holds on every path, if any.
type Clobbered = [Option<Clobber>; 16];

struct Clobbers;

impl Analysis for Clobbers {
    type State = Clobbered;

    fn entry(&self, _: &Program, _: &Function) -> Clobbered {
        [None; 16]
    }

    fn unknown(&self) -> Clobbered {
        [None; 16]
    }

    fn join(&self, a: &Clobbered, b: &Clobbered) -> Clobbered {
        std::array::from_fn(|i| if b[i].is_some() { a[i] } else { None })
    }

    fn transfer(
        &self,
        program: &Program,
        index: usize,
        state: &mut Clobbered,
        mut out: Option<&mut Vec<Diagnostic>>,
    ) {
        let insn = &program.insns[index];
        let fx = &insn.fx;

        // Only registers named in the operands: implicit uses have their
        // own lints (`div-without-rdx`).
        let mut explicit: Vec<(Reg, Span)> = Vec::new();
        for operand in &insn.ins.operands {
            match &operand.kind {
                OperandKind::Reg(reg) => explicit.push((*reg, operand.span)),
                OperandKind::Mem(mem) => {
                    explicit.extend(mem.regs.iter().map(|reg| (*reg, operand.span)));
                }
                _ => {}
            }
        }
        for (reg, span) in explicit {
            if !fx.reads.contains(reg.id) {
                continue;
            }
            if let Some(clobber) = state[reg.id.index()].take()
                && fx.known
                && let Some(out) = out.as_deref_mut()
            {
                out.push(clobbered(program, reg, span, clobber));
            }
        }
        for id in fx.writes.iter() {
            state[id.index()] = None;
        }

        let (set, by) = match &fx.flow {
            Flow::Syscall => (abi::SYSCALL_CLOBBERS, Culprit::Syscall),
            Flow::Call(target) => match program.callee(target) {
                Callee::Local(callee) => {
                    (program.functions[callee].garbage(), Culprit::Local(callee))
                }
                Callee::External => {
                    let mut set = abi::CALLER_SAVED;
                    set.remove(RegId::RAX); // the return value
                    (set, Culprit::External)
                }
            },
            _ => return,
        };
        for id in set.iter() {
            state[id.index()] = Some(Clobber { line: insn.line, by });
        }
    }
}

fn clobbered(program: &Program, reg: Reg, span: Span, clobber: Clobber) -> Diagnostic {
    let name = reg.id.name64();
    let line = clobber.line + 1;
    let (what, note, help) = match clobber.by {
        Culprit::Syscall => (
            format!("the `syscall` on line {line}"),
            "the kernel uses `rcx` and `r11` to return from `syscall`: both are destroyed"
                .to_string(),
            "keep the value in another register, or `push`/`pop` it around the `syscall`",
        ),
        Culprit::External => (
            format!("the call on line {line}"),
            format!("`{name}` is caller-saved: a function is free to overwrite it"),
            "keep the value in a callee-saved register (`rbx`, `r12`-`r15`) or on the stack",
        ),
        Culprit::Local(callee) => {
            let callee = &program.functions[callee];
            let cause = if callee.makes_syscall && abi::SYSCALL_CLOBBERS.contains(reg.id) {
                "runs `syscall`, which destroys `rcx` and `r11`"
            } else {
                "calls external code, which may overwrite caller-saved registers"
            };
            (
                format!("the call to `{}` on line {line}", callee.name),
                format!("`{}` {cause}", callee.name),
                "save the register in the callee, or keep the value somewhere else",
            )
        }
    };
    Diagnostic::new(
        &CLOBBERED_REGISTER,
        span,
        format!("`{name}` is read here, but {what} destroyed its value"),
    )
    .note(note)
    .help(help)
}

// --- callee-saved-clobbered ------------------------------------------------

fn callee_saved(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;
    for function in &program.functions {
        // The convention binds functions other code calls: exported ones.
        // The entry point has no caller to preserve anything for.
        if function.entry_point || !(function.global || function.name == "main") {
            continue;
        }
        let insns = || {
            function
                .blocks
                .iter()
                .flat_map(|block| program.blocks[*block].start..program.blocks[*block].end)
                .map(|index| &program.insns[index])
        };
        if !insns().any(|insn| insn.fx.flow == Flow::Ret) {
            continue;
        }
        for id in abi::CALLEE_SAVED.intersection(function.clobbers).iter() {
            // Blame the first write, or the call that does it on our behalf.
            let culprit = insns().find(|insn| insn.fx.writes.contains(id)).or_else(|| {
                insns().find(|insn| match &insn.fx.flow {
                    Flow::Call(target) => match program.callee(target) {
                        Callee::Local(callee) => program.functions[callee].clobbers.contains(id),
                        Callee::External => false,
                    },
                    _ => false,
                })
            });
            let Some(insn) = culprit else { continue };
            out.push(
                Diagnostic::new(
                    &CALLEE_SAVED_CLOBBERED,
                    insn.span,
                    format!(
                        "`{id}` is callee-saved, but `{}` changes it without restoring it",
                        function.name
                    ),
                )
                .note("callers expect `rbx`, `rbp` and `r12`-`r15` to survive a call")
                .help(format!(
                    "`push {id}` on entry and `pop {id}` before every `ret`, \
                     or use a caller-saved register"
                )),
            );
        }
    }
}

// --- dead-store ------------------------------------------------------------

fn dead_stores(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;
    for block in &program.blocks {
        // Per register: the pure write whose value nothing has read yet.
        let mut pending: [Option<(usize, Reg)>; 16] = [None; 16];
        for index in block.start..block.end {
            let insn = &program.insns[index];
            let fx = &insn.fx;
            if !fx.known {
                pending = [None; 16];
                continue;
            }

            let reads = match &fx.flow {
                Flow::Next => fx.reads,
                Flow::Syscall => fx.reads.union(abi::SYSCALL_INPUTS),
                // A callee, an interrupt handler or the code jumped to may
                // read anything.
                _ => RegSet::of(&ALL_REGISTERS),
            };
            for id in reads.iter() {
                pending[id.index()] = None;
            }
            if !fx.flags_read.is_empty() {
                // A zero idiom also sets flags; here they are observed.
                for slot in &mut pending {
                    if slot.is_some_and(|(first, _)| program.insns[first].fx.zeroing) {
                        *slot = None;
                    }
                }
            }

            let by_syscall =
                if fx.flow == Flow::Syscall { abi::SYSCALL_CLOBBERS } else { RegSet::EMPTY };
            for id in fx.writes.union(by_syscall).iter() {
                let Some((first, reg)) = pending[id.index()].take() else { continue };
                let covers =
                    fx.full_writes.contains(id) || by_syscall.contains(id) || fx.dest == Some(reg);
                if covers && id != RegId::RSP {
                    out.push(dead_store(cx, first, index, reg, by_syscall.contains(id)));
                }
            }
            if fx.flow == Flow::Syscall {
                pending[RegId::RAX.index()] = None;
            }
            if fx.pure
                && let Some(dest) = fx.dest
            {
                pending[dest.id.index()] = Some((index, dest));
            }
        }
    }
}

const ALL_REGISTERS: [RegId; 16] = [
    RegId::RAX,
    RegId::RCX,
    RegId::RDX,
    RegId::RBX,
    RegId::RSP,
    RegId::RBP,
    RegId::RSI,
    RegId::RDI,
    RegId::R8,
    RegId::R9,
    RegId::R10,
    RegId::R11,
    RegId::R12,
    RegId::R13,
    RegId::R14,
    RegId::R15,
];

fn dead_store(cx: &Context, first: usize, second: usize, reg: Reg, by_syscall: bool) -> Diagnostic {
    let program = cx.program;
    let (store, killer) = (&program.insns[first], &program.insns[second]);
    let line = killer.line + 1;
    if by_syscall {
        let mut diagnostic = Diagnostic::new(
            &DEAD_STORE,
            store.span,
            format!("the value written to `{reg}` is destroyed by the `syscall` on line {line}"),
        )
        .note_at(killer.span, "`syscall` overwrites `rcx` and `r11`");
        if reg.id == RegId::RCX {
            diagnostic = diagnostic
                .help("the 4th system call argument goes in `r10`, not `rcx` as for functions");
        }
        return diagnostic;
    }
    let mut diagnostic = Diagnostic::new(
        &DEAD_STORE,
        store.span,
        format!("the value written to `{reg}` is never read"),
    )
    .note_at(killer.span, format!("it is overwritten by `{}` on line {line}", cx.code(killer)))
    .help("remove this instruction, or check that it targets the register you meant");
    if program.lines[store.line].label.is_none() {
        diagnostic = diagnostic.delete_line("remove the dead store", store.line);
    }
    diagnostic
}
