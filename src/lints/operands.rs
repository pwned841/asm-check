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
        let mut diagnostic = if writes {
            Diagnostic::new(&INVALID_OPERANDS, dest.span, format!("cannot write to `{text}`"))
        } else {
            Diagnostic::new(
                &INVALID_OPERANDS,
                dest.span,
                format!("the first operand of `{mnemonic}` cannot be an immediate"),
            )
        };
        if !dest.symbols().is_empty() {
            // A label is an address. The memory it names is `[label]`.
            diagnostic = diagnostic
                .label("an address, not a memory location")
                .replace(format!("access the memory: `[{text}]`"), dest.span, format!("[{text}]"))
                .maybe_incorrect();
        } else if compares {
            diagnostic = diagnostic.label("a constant").help("swap the operands");
        } else {
            diagnostic = diagnostic.label("a constant cannot be a destination");
        }
        out.push(diagnostic);
        return true;
    }

    if let (Some(a), Some(b)) = (first, second)
        && a.is_mem()
        && b.is_mem()
        && (BINARY.contains(&mnemonic) || EXTENDS.contains(&mnemonic))
    {
        out.push(
            Diagnostic::new(
                &INVALID_OPERANDS,
                insn.span,
                format!("`{mnemonic}` cannot take two memory operands"),
            )
            .help("go through a register: load one operand first"),
        );
        return true;
    }

    if mnemonic == "lea"
        && let Some(source) = second
        && !source.is_mem()
    {
        let text = cx.text(source.span);
        out.push(
            Diagnostic::new(&INVALID_OPERANDS, source.span, "`lea` needs a memory operand")
                .label("not an address expression")
                .replace(format!("write `[{text}]`"), source.span, format!("[{text}]"))
                .maybe_incorrect(),
        );
        return true;
    }

    if matches!(mnemonic, "push" | "pop")
        && let Some(reg) = first.and_then(Operand::as_reg)
        && !matches!(reg.width, Width::W64 | Width::W16)
        && let Some(operand) = first
    {
        let wide = reg.id.name64();
        out.push(
            Diagnostic::new(
                &INVALID_OPERANDS,
                operand.span,
                format!("`{mnemonic} {reg}` does not exist in 64-bit mode"),
            )
            .label("stack slots are 64-bit")
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
        let mut diagnostic = Diagnostic::new(
            &OPERAND_SIZE_MISMATCH,
            b.span,
            format!("`{mnemonic}` cannot shift by `{count}`"),
        )
        .label("the count must be `cl` or a constant");
        if count.id == RegId::RCX {
            diagnostic = diagnostic.replace("use `cl`", b.span, "cl");
        }
        out.push(diagnostic);
        return;
    }

    let (Some(x), Some(y)) = (a.bits(), b.bits()) else { return };
    if b.is_imm() {
        return;
    }

    // `movzx`/`movsx` only take an 8- or 16-bit source.
    if EXTENDS.contains(&mnemonic) && (y >= x || y > 16) {
        let mut diagnostic = Diagnostic::new(
            &OPERAND_SIZE_MISMATCH,
            insn.span,
            format!("`{mnemonic}` cannot widen {y} bits to {x}"),
        )
        .label("the source must be 8 or 16 bits, and smaller than the destination");
        if let (32, 64, Some(dest)) = (y, x, a.as_reg()) {
            let replacement = if mnemonic == "movzx" {
                format!("mov {}, {b_text}", dest.id.name32())
            } else {
                format!("movsxd {a_text}, {b_text}")
            };
            diagnostic = diagnostic.replace(format!("use `{replacement}`"), insn.span, replacement);
        }
        out.push(diagnostic);
        return;
    }

    if (BINARY.contains(&mnemonic) || cmov) && x != y {
        let mut diagnostic = Diagnostic::new(
            &OPERAND_SIZE_MISMATCH,
            insn.span,
            format!("`{a_text}` is {x}-bit but `{b_text}` is {y}-bit"),
        );
        // Between two registers, the narrower one decides: shrink the other.
        diagnostic = match (a.as_reg(), b.as_reg()) {
            (Some(dest), Some(source)) => {
                let (operand, wide, bits) =
                    if x > y { (a, dest, source.width) } else { (b, source, dest.width) };
                match wide.id.name(bits) {
                    Some(part) => diagnostic
                        .replace(format!("use `{part}`"), operand.span, part)
                        .maybe_incorrect(),
                    None => diagnostic.help("both operands must have the same size"),
                }
            }
            _ => diagnostic.help("both operands must have the same size"),
        };
        out.push(diagnostic);
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
    if !undetermined {
        return;
    }
    let text = cx.text(target.span);
    let diagnostic = Diagnostic::new(
        &AMBIGUOUS_OPERAND_SIZE,
        target.span,
        format!("the size of `{text}` is not specified"),
    )
    .label("1, 2, 4 or 8 bytes?");
    // The declaration of the variable says what was meant.
    let declared = target
        .symbols()
        .first()
        .and_then(|name| cx.program.symbols.get(name))
        .and_then(|symbol| symbol.data.as_ref())
        .and_then(|data| match data.directive.as_str() {
            "db" | "resb" => Some("byte"),
            "dw" | "resw" => Some("word"),
            "dd" | "resd" => Some("dword"),
            "dq" | "resq" => Some("qword"),
            _ => None,
        });
    out.push(match declared {
        Some(size) => diagnostic
            .replace(
                format!("it is declared as `{size}`"),
                crate::span::Span::new(target.span.line, target.span.start, target.span.start),
                format!("{size} "),
            )
            .maybe_incorrect(),
        None => diagnostic.help(format!("prefix it: `byte {text}`, `word`, `dword` or `qword`")),
    });
}
