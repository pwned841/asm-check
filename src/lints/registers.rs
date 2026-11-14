//! Register values lost by accident: not preserved for the caller, or
//! overwritten before being read.

use super::Context;
use crate::abi;
use crate::diagnostic::{Diagnostic, Group, Lint};
use crate::insn::Flow;
use crate::program::Callee;
use crate::reg::{Reg, RegId, RegSet};

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
    callee_saved(cx, out);
    dead_stores(cx, out);
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
                    format!("`{id}` is modified but not restored"),
                )
                .label(format!(
                    "callee-saved: the caller of `{}` expects it unchanged",
                    function.name
                ))
                .help(format!("`push {id}` on entry and `pop {id}` before returning")),
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
            // `mov rax, rax` is `self-move`'s business, not a value that goes unused.
            let copies_itself = insn.ins.mnemonic == "mov"
                && insn.ins.operands.get(1).and_then(|source| source.as_reg()) == fx.dest;
            if fx.pure
                && !copies_itself
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
    if by_syscall {
        let mut diagnostic = Diagnostic::new(
            &DEAD_STORE,
            store.span,
            format!("`{reg}` is destroyed by `syscall` before being read"),
        )
        .label("this value is lost")
        .note_at(killer.span, "`syscall` overwrites `rcx` and `r11`");
        // The classic slip: the 4th argument goes in `rcx` for a function
        // and in `r10` for a system call.
        if reg.id == RegId::RCX
            && let Some(dest) = store.ins.operands.first()
            && let Some(r10) = RegId::R10.name(reg.width)
        {
            diagnostic = diagnostic
                .replace("pass the 4th system call argument in `r10`", dest.span, r10)
                .maybe_incorrect();
        }
        return diagnostic;
    }
    let mut diagnostic = Diagnostic::new(
        &DEAD_STORE,
        store.span,
        format!("`{reg}` is overwritten before being read"),
    )
    .label("this value is never used")
    .note_at(killer.span, "overwritten here");
    if program.lines[store.line].label.is_none() {
        diagnostic = diagnostic.delete_line("remove the dead store", store.line).maybe_incorrect();
    }
    diagnostic
}
