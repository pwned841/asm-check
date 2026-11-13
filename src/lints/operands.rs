//! Operand combinations the assembler rejects or silently mis-assembles.

use super::Context;
use crate::ast::Operand;
use crate::diagnostic::{Diagnostic, Group, Lint};
use crate::mnemonic::condition;
use crate::program::Insn;
use crate::reg::{Reg, RegId, Width};

pub static OPERAND_SIZE_MISMATCH: Lint = Lint {
    name: "operand-size-mismatch",
    group: Group::Correctness,
    summary: "operands of different sizes, such as `mov rax, ebx`",
};

pub static AMBIGUOUS_OPERAND_SIZE: Lint = Lint {
    name: "ambiguous-operand-size",
    group: Group::Correctness,
    summary: "a memory operand whose size nothing determines, such as `mov [rdi], 0`",
};

pub static INVALID_OPERANDS: Lint = Lint {
    name: "invalid-operands",
    group: Group::Correctness,
    summary: "operands an instruction cannot take: two memory operands, an immediate destination, \
              `push eax`, ...",
};

const BINARY: &[&str] =
    &["mov", "add", "sub", "adc", "sbb", "and", "or", "xor", "cmp", "test", "xchg"];
const UNARY: &[&str] = &["inc", "dec", "neg", "not"];
const SHIFTS: &[&str] = &["shl", "sal", "shr", "sar", "rol", "ror", "rcl", "rcr"];
const EXTENDS: &[&str] = &["movzx", "movsx"];

/// The number of operands a mnemonic accepts, as `(min, max)`.
fn arity(mnemonic: &str) -> Option<(usize, usize)> {
    Some(match mnemonic {
        "mov" | "movzx" | "movsx" | "movsxd" | "lea" | "add" | "sub" | "adc" | "sbb" | "and"
        | "or" | "xor" | "cmp" | "test" | "xchg" => (2, 2),
        "inc" | "dec" | "neg" | "not" | "push" | "pop" | "jmp" | "call" | "mul" | "div"
        | "idiv" => (1, 1),
        "shl" | "sal" | "shr" | "sar" | "rol" | "ror" | "rcl" | "rcr" => (1, 2),
        "imul" => (1, 3),
        "ret" => (0, 1),
        "syscall" | "leave" | "cqo" | "cdq" => (0, 0),
        _ => match condition(mnemonic)?.0 {
            "cmov" => (2, 2),
            _ => (1, 1),
        },
    })
}

pub fn check(cx: &Context, out: &mut Vec<Diagnostic>) {
    for insn in &cx.program.insns {
        if cx.uses_define(insn) {
            continue;
        }
        let mnemonic = insn.ins.mnemonic.as_str();
        let found = insn.ins.operands.len();
        if let Some((min, max)) = arity(mnemonic)
            && !(min..=max).contains(&found)
        {
            let expected = match (min, max) {
                (0, 0) => "no operand".to_string(),
                (1, 1) => "1 operand".to_string(),
                _ if min == max => format!("{min} operands"),
                _ => format!("{min} to {max} operands"),
            };
            let message = format!("`{mnemonic}` takes {expected}, found {found}");
            out.push(Diagnostic::new(&INVALID_OPERANDS, insn.span, message));
            continue;
        }
        if !invalid(cx, insn, out) {
            sizes(cx, insn, out);
            ambiguous(cx, insn, out);
        }
    }
}

/// Reports operand kinds the instruction cannot take. Returns `true` when
/// the instruction is too broken for the size checks to mean anything.
fn invalid(cx: &Context, insn: &Insn, out: &mut Vec<Diagnostic>) -> bool {
    let mnemonic = insn.ins.mnemonic.as_str();
    let (first, second) = (insn.ins.operands.first(), insn.ins.operands.get(1));
    let cmov = condition(mnemonic).is_some_and(|(family, _)| family == "cmov");
    let binary = BINARY.contains(&mnemonic) || EXTENDS.contains(&mnemonic) || cmov;
    let compares = matches!(mnemonic, "cmp" | "test");
    let writes = (binary && !compares)
        || UNARY.contains(&mnemonic)
        || matches!(mnemonic, "pop" | "lea" | "movsxd");

    if let Some(dest) = first
        && dest.is_imm()
        && (writes || compares)
    {
        let text = cx.text(dest.span);
        let message = if writes {
            format!("`{mnemonic}` cannot write to `{text}`: it is a value, not a location")
        } else {
            format!("the first operand of `{mnemonic}` cannot be an immediate")
        };
        let mut diagnostic = Diagnostic::new(&INVALID_OPERANDS, dest.span, message);
        if !dest.symbols().is_empty() {
            diagnostic = diagnostic
                .help(format!("to access the memory at `{text}`, write `[{text}]`"))
                .replace("wrap in brackets", dest.span, format!("[{text}]"));
        } else if compares {
            diagnostic = diagnostic.help("swap the operands");
        }
        out.push(diagnostic);
        return true;
    }

    if let (Some(a), Some(b)) = (first, second)
        && a.is_mem()
        && b.is_mem()
        && (BINARY.contains(&mnemonic) || EXTENDS.contains(&mnemonic))
    {
        let message = format!("`{mnemonic}` cannot take two memory operands");
        out.push(
            Diagnostic::new(&INVALID_OPERANDS, insn.span, message)
                .help("go through a register: load one operand first, then use it"),
        );
        return true;
    }

    if mnemonic == "lea"
        && let Some(source) = second
        && !source.is_mem()
    {
        let text = cx.text(source.span);
        let message = "`lea` computes an address: its source must be a memory operand";
        out.push(
            Diagnostic::new(&INVALID_OPERANDS, source.span, message)
                .help(format!("write `[{text}]`, or use `mov` to copy a value")),
        );
        return true;
    }

    if matches!(mnemonic, "push" | "pop")
        && let Some(reg) = first.and_then(Operand::as_reg)
        && !matches!(reg.width, Width::W64 | Width::W16)
        && let Some(operand) = first
    {
        let wide = reg.id.name64();
        let message = format!("`{mnemonic} {reg}` cannot be encoded in 64-bit mode");
        out.push(
            Diagnostic::new(&INVALID_OPERANDS, operand.span, message)
                .help(format!("the stack holds 64-bit values: use `{mnemonic} {wide}`"))
                .replace(format!("use `{wide}`"), operand.span, wide),
        );
        return true;
    }

    false
}

fn sizes(cx: &Context, insn: &Insn, out: &mut Vec<Diagnostic>) {
    let mnemonic = insn.ins.mnemonic.as_str();
    let (Some(a), Some(b)) = (insn.ins.operands.first(), insn.ins.operands.get(1)) else { return };
    let (a_text, b_text) = (cx.text(a.span), cx.text(b.span));
    let cmov = condition(mnemonic).is_some_and(|(family, _)| family == "cmov");

    if SHIFTS.contains(&mnemonic)
        && let Some(count) = b.as_reg()
        && count != Reg::new(RegId::RCX, Width::Low8)
    {
        let message = format!("a shift count must be `cl` or an immediate, not `{count}`");
        out.push(
            Diagnostic::new(&OPERAND_SIZE_MISMATCH, b.span, message)
                .help("move the count into `rcx` and shift by `cl`"),
        );
        return;
    }

    let (Some(x), Some(y)) = (a.bits(), b.bits()) else { return };
    if b.is_imm() {
        return;
    }

    // `movzx`/`movsx` only take an 8- or 16-bit source.
    if EXTENDS.contains(&mnemonic) && (y >= x || y > 16) {
        let message = if y >= x {
            format!(
                "`{mnemonic}` widens its source, but `{b_text}` ({y}-bit) is not smaller than \
                 `{a_text}` ({x}-bit)"
            )
        } else {
            format!("`{mnemonic}` takes an 8- or 16-bit source, but `{b_text}` is {y}-bit")
        };
        let mut diagnostic = Diagnostic::new(&OPERAND_SIZE_MISMATCH, insn.span, message);
        if let (32, 64, Some(dest)) = (y, x, a.as_reg()) {
            diagnostic = diagnostic.help(if mnemonic == "movzx" {
                format!("`mov {}, {b_text}` already clears the upper 32 bits", dest.id.name32())
            } else {
                format!("use `movsxd {a_text}, {b_text}` to sign-extend 32 bits to 64")
            });
        }
        out.push(diagnostic);
        return;
    }

    if (BINARY.contains(&mnemonic) || cmov) && x != y {
        let message = format!("`{a_text}` is {x}-bit but `{b_text}` is {y}-bit");
        let help = match (mnemonic, a.as_reg(), b.as_reg()) {
            ("mov", Some(dest), Some(_)) if y == 32 && x == 64 => format!(
                "`mov {}, {b_text}` zero-extends into `{a_text}`; use `movsxd` to sign-extend",
                dest.id.name32()
            ),
            ("mov", Some(_), _) if y < x => {
                format!("use `movzx {a_text}, {b_text}` to zero-extend, or `movsx` to sign-extend")
            }
            (_, Some(dest), Some(source)) if x < y => match source.id.name(dest.width) {
                Some(part) => format!("use `{part}`, the {x}-bit part of `{b_text}`"),
                None => "both operands must have the same size".to_string(),
            },
            _ => "both operands must have the same size".to_string(),
        };
        out.push(Diagnostic::new(&OPERAND_SIZE_MISMATCH, insn.span, message).help(help));
    }
}

fn ambiguous(cx: &Context, insn: &Insn, out: &mut Vec<Diagnostic>) {
    let mnemonic = insn.ins.mnemonic.as_str();
    let operands = &insn.ins.operands;
    let Some(target) = operands.first().filter(|op| op.is_mem() && op.size.is_none()) else {
        return;
    };
    let immediate_source =
        operands.get(1).is_some_and(|source| source.is_imm() && source.size.is_none());
    let undetermined = (BINARY.contains(&mnemonic) && immediate_source)
        || SHIFTS.contains(&mnemonic)
        || UNARY.contains(&mnemonic)
        || (matches!(mnemonic, "mul" | "div" | "idiv" | "imul") && operands.len() == 1);
    if undetermined {
        let text = cx.text(target.span);
        let message = format!("the size of `{text}` is ambiguous: it could be 1, 2, 4 or 8 bytes");
        out.push(Diagnostic::new(&AMBIGUOUS_OPERAND_SIZE, target.span, message).help(format!(
            "state it: `byte {text}`, `word {text}`, `dword {text}` or `qword {text}`"
        )));
    }
}
