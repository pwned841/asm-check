//! Operand combinations the assembler rejects or silently mis-assembles.

use super::Context;
use crate::ast::{Operand, OperandKind};
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
        | "or" | "xor" | "cmp" | "test" | "xchg" | "bsf" | "bsr" | "popcnt" | "lzcnt" | "tzcnt"
        | "xadd" | "cmpxchg" | "bt" | "bts" | "btr" | "btc" => (2, 2),
        "inc" | "dec" | "neg" | "not" | "push" | "pop" | "jmp" | "call" | "mul" | "div"
        | "idiv" | "bswap" => (1, 1),
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
        if !invalid(cx, insn, out) && !forms(cx, insn, out) {
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

    // NASM reads `lea rax, label` and `lea rax, 8` as an address: only a
    // register source is an error.
    if mnemonic == "lea"
        && let Some(source) = second
        && matches!(source.kind, OperandKind::Reg(_) | OperandKind::OtherReg(_))
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

/// What an operand is, for checking the forms an instruction accepts.
#[derive(Clone, Copy, PartialEq)]
enum Shape {
    /// A general-purpose register of this many bits.
    Reg(u32),
    /// Memory, with its size when the source states it.
    Mem(Option<u32>),
    Imm,
    /// A vector, segment or other register this tool does not model.
    Other,
}

fn shape(operand: &Operand) -> Shape {
    match &operand.kind {
        OperandKind::Reg(reg) => Shape::Reg(reg.width.bits()),
        OperandKind::Mem(_) => Shape::Mem(operand.size),
        OperandKind::Imm(_) => Shape::Imm,
        OperandKind::OtherReg(_) => Shape::Other,
    }
}

const WIDE: &str = "needs a 16-, 32- or 64-bit register here";
const SAME: &str = "needs operands of the same size";
const NO_IMM: &str = "cannot take an immediate here";

/// The operand an instruction cannot take, and why, for the instructions
/// whose forms are listed here. `None` when the combination is valid or
/// not modelled: every entry was checked against NASM 2.16.
fn form_error(mnemonic: &str, shapes: &[Shape]) -> Option<(usize, &'static str)> {
    use Shape::{Imm, Mem, Reg};
    let wide = |shape: Shape| matches!(shape, Reg(16 | 32 | 64));
    // A register or sized memory whose size disagrees with `bits`.
    let differs = |shape: Shape, bits: u32| match shape {
        Reg(other) => other != bits,
        Mem(Some(other)) => other != bits,
        _ => false,
    };
    match (mnemonic, shapes) {
        ("bsf" | "bsr" | "popcnt" | "lzcnt" | "tzcnt", [dest, source]) => {
            if !wide(*dest) {
                Some((0, "needs a 16-, 32- or 64-bit register as destination"))
            } else if *source == Imm {
                Some((1, NO_IMM))
            } else if let Reg(bits) = dest
                && differs(*source, *bits)
            {
                Some((1, SAME))
            } else {
                None
            }
        }
        ("xadd" | "cmpxchg", [dest, source]) => match (dest, source) {
            (Imm, _) => Some((0, "cannot write to an immediate")),
            (_, Reg(bits)) if differs(*dest, *bits) => Some((1, SAME)),
            (_, Reg(_)) => None,
            _ => Some((1, "needs a register as its second operand")),
        },
        ("bt" | "bts" | "btr" | "btc", [dest, source]) => match (dest, source) {
            (Imm, _) => Some((0, NO_IMM)),
            (Reg(8), _) | (Mem(Some(8)), _) => Some((0, "needs a 16-, 32- or 64-bit operand")),
            (_, Mem(_)) | (_, Reg(8)) => Some((1, "needs a register or an immediate bit index")),
            (_, Reg(bits)) if differs(*dest, *bits) => Some((1, SAME)),
            _ => None,
        },
        ("imul", [dest, source]) => {
            if !wide(*dest) {
                Some((0, "needs a 16-, 32- or 64-bit register as destination"))
            } else if let Reg(bits) = dest
                && differs(*source, *bits)
            {
                Some((1, SAME))
            } else {
                None
            }
        }
        ("imul", [dest, source, third]) => {
            if !wide(*dest) {
                Some((0, "needs a 16-, 32- or 64-bit register as destination"))
            } else if *source == Imm {
                Some((1, "needs a register or memory here"))
            } else if *third != Imm {
                Some((2, "needs an immediate as its third operand"))
            } else if let Reg(bits) = dest
                && differs(*source, *bits)
            {
                Some((1, SAME))
            } else {
                None
            }
        }
        ("movzx" | "movsx", [dest, source]) => {
            if !wide(*dest) {
                Some((0, "needs a 16-, 32- or 64-bit register as destination"))
            } else if *source == Imm {
                Some((1, NO_IMM))
            } else {
                None
            }
        }
        ("movsxd", [dest, source]) => {
            if !matches!(dest, Reg(64)) {
                Some((0, "needs a 64-bit register as destination"))
            } else if *source == Imm {
                Some((1, NO_IMM))
            } else if differs(*source, 32) {
                Some((1, "needs a 32-bit source"))
            } else {
                None
            }
        }
        ("lea", [dest, _]) if !wide(*dest) => Some((0, WIDE)),
        ("xchg", [_, Imm]) => Some((1, NO_IMM)),
        ("mul" | "div" | "idiv" | "imul", [Imm]) => Some((0, NO_IMM)),
        ("push" | "pop", [Mem(Some(bits))]) if !matches!(bits, 16 | 64) => {
            Some((0, "has no form of this size in 64-bit mode"))
        }
        ("call" | "jmp", [Mem(Some(bits))]) if *bits != 64 => {
            Some((0, "has no form of this size in 64-bit mode"))
        }
        ("bswap", [dest]) if !matches!(dest, Reg(32 | 64)) => {
            Some((0, "needs a 32- or 64-bit register"))
        }
        ("shl" | "sal" | "shr" | "sar" | "rol" | "ror" | "rcl" | "rcr", [_, Mem(_)]) => {
            Some((1, "needs `cl` or an immediate as the count"))
        }
        ("call" | "jmp", [Reg(bits)]) if *bits != 64 => Some((0, "needs a 64-bit register")),
        (setcc, [dest]) if condition(setcc).is_some_and(|(family, _)| family == "set") => {
            match dest {
                Reg(8) | Mem(None | Some(8)) => None,
                _ => Some((0, "needs an 8-bit register or byte of memory")),
            }
        }
        (cmov, [dest, source]) if condition(cmov).is_some_and(|(family, _)| family == "cmov") => {
            if !wide(*dest) {
                Some((0, "needs a 16-, 32- or 64-bit register as destination"))
            } else if *source == Imm {
                Some((1, NO_IMM))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Reports a combination of operand kinds the instruction does not have.
/// Returns `true` when it did.
fn forms(cx: &Context, insn: &Insn, out: &mut Vec<Diagnostic>) -> bool {
    let operands = &insn.ins.operands;
    let shapes: Vec<Shape> = operands.iter().map(shape).collect();
    if shapes.contains(&Shape::Other) {
        return false;
    }
    let mnemonic = insn.ins.mnemonic.as_str();
    let Some((at, reason)) = form_error(mnemonic, &shapes) else { return false };
    let span = operands.get(at).map_or(insn.span, |operand| operand.span);
    out.push(
        Diagnostic::new(
            &INVALID_OPERANDS,
            span,
            format!("`{mnemonic}` {reason}: `{}`", cx.text(span)),
        )
        .label("no such form of the instruction"),
    );
    true
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
    // NASM accepts `movsx rax, eax` as `movsxd rax, eax`.
    let movsxd = mnemonic == "movsx" && x == 64 && y == 32;
    if EXTENDS.contains(&mnemonic) && !movsxd && (y >= x || y > 16) {
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
    let unsized_at = |index: usize| {
        operands.get(index).filter(|operand| operand.is_mem() && operand.size.is_none())
    };
    let immediate_source =
        operands.get(1).is_some_and(|source| source.is_imm() && source.size.is_none());
    // `movzx eax, [rdi]`: the memory operand is the source, and nothing says
    // whether to read a byte or a word.
    let target = if EXTENDS.contains(&mnemonic) {
        // Into a 16-bit register the source can only be a byte.
        unsized_at(1).filter(|_| operands[0].as_reg().is_none_or(|dest| dest.width.bits() > 16))
    } else {
        unsized_at(0).filter(|_| {
            (BINARY.contains(&mnemonic) && immediate_source)
                || (matches!(mnemonic, "bt" | "bts" | "btr" | "btc") && immediate_source)
                || SHIFTS.contains(&mnemonic)
                || UNARY.contains(&mnemonic)
                || (matches!(mnemonic, "mul" | "div" | "idiv" | "imul") && operands.len() == 1)
                || (matches!(mnemonic, "push" | "pop") && operands.len() == 1)
        })
    };
    let Some(target) = target else { return };
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
