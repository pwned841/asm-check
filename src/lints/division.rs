//! `div` and `idiv` divide the 128-bit value `rdx:rax`: forgetting to
//! prepare `rdx` is the classic division bug.

use super::Context;
use crate::ast::Operand;
use crate::dataflow::{self, Analysis};
use crate::diagnostic::{Diagnostic, Group, Lint};
use crate::insn::Flow;
use crate::program::{Callee, Function, Program};
use crate::reg::RegId;

pub static DIV_WITHOUT_RDX: Lint = Lint {
    name: "div-without-rdx",
    group: Group::Suspicious,
    summary: "`div` or `idiv` reached without `rdx` having been prepared",
};

pub static IDIV_ZERO_EXTENDED: Lint = Lint {
    name: "idiv-zero-extended",
    group: Group::Suspicious,
    summary: "`idiv` with `rdx` zeroed instead of sign-extended",
};

pub fn check(cx: &Context, out: &mut Vec<Diagnostic>) {
    dataflow::run(cx.program, &Dividend, out);
}

/// Why `rdx` does not hold the upper half of a dividend. Lines are 0-based.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Stale {
    /// Never written since the function was entered.
    Entry,
    /// Left over from a call on this line.
    Call(usize),
    /// The remainder of the division on this line.
    Division(usize),
    /// Set as the third argument of the system call on this line.
    Argument(usize),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Rdx {
    Zeroed,
    SignExtended,
    /// Written on purpose, in some other way.
    Set,
    Stale(Stale),
}

struct Dividend;

impl Analysis for Dividend {
    type State = Rdx;

    fn entry(&self, _: &Program, _: &Function) -> Rdx {
        Rdx::Stale(Stale::Entry)
    }

    fn unknown(&self) -> Rdx {
        Rdx::Set
    }

    /// `rdx` is stale if it is stale on any path.
    fn join(&self, a: &Rdx, b: &Rdx) -> Rdx {
        match (a, b) {
            _ if a == b => *a,
            (Rdx::Stale(_), _) => *a,
            (_, Rdx::Stale(_)) => *b,
            _ => Rdx::Set,
        }
    }

    fn transfer(
        &self,
        program: &Program,
        index: usize,
        state: &mut Rdx,
        out: Option<&mut Vec<Diagnostic>>,
    ) {
        let insn = &program.insns[index];
        let mnemonic = insn.ins.mnemonic.as_str();
        let operands = &insn.ins.operands;

        // The 8-bit forms divide `ax` and leave `rdx` alone.
        let wide = operands.first().and_then(Operand::bits) != Some(8);
        if matches!(mnemonic, "div" | "idiv") && wide {
            if let Some(out) = out {
                report(program, index, *state, out);
            }
            *state = Rdx::Stale(Stale::Division(insn.line));
            return;
        }

        if let Flow::Call(target) = &insn.fx.flow {
            match program.callee(target) {
                Callee::External => *state = Rdx::Stale(Stale::Call(insn.line)),
                Callee::Local(callee) => {
                    let callee = &program.functions[callee];
                    if callee.garbage().contains(RegId::RDX) {
                        *state = Rdx::Stale(Stale::Call(insn.line));
                    } else if callee.clobbers.contains(RegId::RDX) {
                        *state = Rdx::Set; // possibly a result
                    }
                }
            }
            return;
        }

        // A value loaded for a system call is an argument, not a dividend.
        // A zeroed `rdx` is left alone: the kernel preserves it.
        if insn.fx.flow == Flow::Syscall && *state == Rdx::Set {
            *state = Rdx::Stale(Stale::Argument(insn.line));
            return;
        }

        if !insn.fx.writes.contains(RegId::RDX) {
            return;
        }
        let zero_constant =
            mnemonic == "mov" && operands.get(1).and_then(Operand::constant) == Some(0);
        *state = match insn.fx.dest {
            _ if matches!(mnemonic, "cqo" | "cdq" | "cwd") => Rdx::SignExtended,
            // `mov dl, 0` leaves the upper bits as they were.
            Some(dest) if !dest.width.is_full() => *state,
            Some(_) if insn.fx.zeroing || zero_constant => Rdx::Zeroed,
            _ => Rdx::Set,
        };
    }
}

fn report(program: &Program, index: usize, state: Rdx, out: &mut Vec<Diagnostic>) {
    let insn = &program.insns[index];
    let mnemonic = insn.ins.mnemonic.as_str();
    let signed = mnemonic == "idiv";
    match state {
        Rdx::Stale(reason) => {
            let because = match reason {
                Stale::Entry => "is never set before this point".to_string(),
                Stale::Call(line) => format!("was left undefined by the call on line {}", line + 1),
                Stale::Argument(line) => format!(
                    "still holds the third argument of the system call on line {}",
                    line + 1
                ),
                Stale::Division(line) if line == insn.line => {
                    "still holds the remainder of the previous iteration".to_string()
                }
                Stale::Division(line) => {
                    format!("still holds the remainder of the division on line {}", line + 1)
                }
            };
            let fix = if signed {
                "sign-extend `rax` into it first with `cqo` (`cdq` for a 32-bit division)"
            } else {
                "clear it first with `xor edx, edx`"
            };
            out.push(
                Diagnostic::new(
                    &DIV_WITHOUT_RDX,
                    insn.span,
                    format!("`{mnemonic}` divides `rdx:rax`, but `rdx` {because}"),
                )
                .note(
                    "a leftover `rdx` gives a wrong quotient, or a divide error (SIGFPE) \
                     when the quotient does not fit",
                )
                .help(fix),
            );
        }
        Rdx::Zeroed if signed => out.push(
            Diagnostic::new(
                &IDIV_ZERO_EXTENDED,
                insn.span,
                "`idiv` is a signed division, but `rdx` was zeroed instead of sign-extended",
            )
            .note("with `rdx` = 0 a negative `rax` is read as a huge positive number")
            .help("use `cqo` (`cdq` for a 32-bit division) right before `idiv`"),
        ),
        _ => {}
    }
}
