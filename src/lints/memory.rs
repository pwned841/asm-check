//! Memory operands and immediates: addresses that cannot be encoded,
//! accesses wider than the data they touch, values that do not fit.

use super::Context;
use crate::ast::{Mem, Operand, OperandKind};
use crate::diagnostic::{Diagnostic, Group, Lint};
use crate::mnemonic::condition;
use crate::program::{Insn, SymbolKind};
use crate::reg::{RegId, Width};

pub static INVALID_ADDRESS: Lint = Lint {
    name: "invalid-address",
    group: Group::Correctness,
    summary: "a memory operand no addressing mode can express, such as `[ax]` or `[rax + rbx*3]`",
};

pub static TRUNCATED_ADDRESS: Lint = Lint {
    name: "truncated-address",
    group: Group::Suspicious,
    summary: "a 32-bit register used as an address in 64-bit code, such as `[eax]`",
};

pub static DATA_SIZE_MISMATCH: Lint = Lint {
    name: "data-size-mismatch",
    group: Group::Suspicious,
    summary: "an access wider than the data it names, such as `mov rax, [flag]` with `flag db 0`",
};

pub static WRITE_TO_READONLY: Lint = Lint {
    name: "write-to-readonly",
    group: Group::Suspicious,
    summary: "a store to data placed in `.rodata` or `.text`",
};

pub static IMMEDIATE_OUT_OF_RANGE: Lint = Lint {
    name: "immediate-out-of-range",
    group: Group::Correctness,
    summary: "an immediate that does not fit its operand, such as `mov al, 300` or \
              `and rax, 0xFFFFFFFF`",
};

pub static SHIFT_OUT_OF_RANGE: Lint = Lint {
    name: "shift-out-of-range",
    group: Group::Suspicious,
    summary: "a shift by at least the width of its operand, such as `shl rax, 64`",
};

const ARITHMETIC: &[&str] = &["add", "sub", "adc", "sbb", "and", "or", "xor", "cmp", "test"];
const SHIFTS: &[&str] = &["shl", "sal", "shr", "sar", "rol", "ror", "rcl", "rcr"];

pub fn check(cx: &Context, out: &mut Vec<Diagnostic>) {
    for insn in &cx.program.insns {
        if cx.uses_define(insn) {
            continue;
        }
        for (position, operand) in insn.ins.operands.iter().enumerate() {
            if let OperandKind::Mem(mem) = &operand.kind {
                address(cx, insn, operand, mem, out);
                if insn.fx.known && insn.ins.mnemonic != "lea" {
                    access(cx, insn, position, operand, mem, out);
                }
            }
        }
        immediate(cx, insn, out);
    }
}

fn address(cx: &Context, insn: &Insn, operand: &Operand, mem: &Mem, out: &mut Vec<Diagnostic>) {
    let text = cx.text(operand.span);
    let mut invalid = |message: String, help: &str| {
        out.push(Diagnostic::new(&INVALID_ADDRESS, operand.span, message).help(help));
    };

    if let Some(narrow) = mem.regs.iter().find(|reg| !reg.width.is_full()) {
        invalid(
            format!("`{narrow}` cannot be part of an address"),
            "only 64-bit (and 32-bit) registers can be used inside `[...]`",
        );
        return;
    }
    let widths = |width| mem.regs.iter().filter(|reg| reg.width == width).count();
    if widths(Width::W32) > 0 && widths(Width::W64) > 0 {
        invalid(
            format!("`{text}` mixes 32-bit and 64-bit registers"),
            "use registers of the same size",
        );
        return;
    }
    if mem.regs.len() > 2 {
        invalid(
            format!("`{text}` uses {} registers", mem.regs.len()),
            "an address is `base + index*scale + displacement`: two registers at most",
        );
        return;
    }
    // With one register, NASM splits `rax*3` into `rax + rax*2`.
    let allowed: &[i64] = if mem.regs.len() == 1 { &[1, 2, 3, 4, 5, 8, 9] } else { &[1, 2, 4, 8] };
    for (reg, factor) in &mem.scales {
        if !allowed.contains(factor) {
            invalid(
                format!("`{reg}*{factor}` is not a valid scale"),
                "the scale must be 1, 2, 4 or 8",
            );
            return;
        }
        if reg.id == RegId::RSP && *factor != 1 {
            invalid(
                "`rsp` cannot be scaled".to_string(),
                "`rsp` can only be a base register; put the index in another register",
            );
            return;
        }
    }
    if mem.scales.iter().filter(|(_, factor)| *factor != 1).count() > 1 {
        invalid(format!("`{text}` scales two registers"), "only the index register can be scaled");
        return;
    }

    if insn.ins.mnemonic != "lea" && !mem.regs.is_empty() && widths(Width::W32) == mem.regs.len() {
        // Rewrite every 32-bit register of the operand as its 64-bit form.
        let mut wide = text.to_string();
        for reg in &mem.regs {
            wide = wide.replace(reg.name(), reg.id.name64());
        }
        out.push(
            Diagnostic::new(&TRUNCATED_ADDRESS, operand.span, format!("32-bit address `{text}`"))
                .label("the upper half of the pointer is dropped")
                .replace(format!("use `{wide}`"), operand.span, wide)
                .maybe_incorrect(),
        );
    }
}

/// Whether the instruction stores to its first operand.
fn stores(insn: &Insn) -> bool {
    let mnemonic = insn.ins.mnemonic.as_str();
    matches!(
        mnemonic,
        "mov"
            | "add"
            | "sub"
            | "adc"
            | "sbb"
            | "and"
            | "or"
            | "xor"
            | "inc"
            | "dec"
            | "neg"
            | "not"
            | "xchg"
            | "pop"
            | "bts"
            | "btr"
            | "btc"
            | "xadd"
            | "cmpxchg"
    ) || SHIFTS.contains(&mnemonic)
        || condition(mnemonic).is_some_and(|(family, _)| family == "set")
}

fn access(
    cx: &Context,
    insn: &Insn,
    position: usize,
    operand: &Operand,
    mem: &Mem,
    out: &mut Vec<Diagnostic>,
) {
    let Some(name) = mem.symbols.first() else { return };
    let Some(symbol) = cx.program.symbols.get(name) else { return };
    if symbol.kind != SymbolKind::Data {
        return;
    }
    let writes = position == 0 && stores(insn);

    let readonly = [".rodata", ".rdata", ".text"].iter().any(|s| symbol.section.starts_with(s));
    if writes && readonly {
        out.push(
            Diagnostic::new(
                &WRITE_TO_READONLY,
                operand.span,
                format!("write to read-only `{name}`"),
            )
            .label(format!("`{}` is not writable: this faults at run time", symbol.section))
            .note_at(symbol.span, "defined here")
            .help(format!("define `{name}` in `.data` or `.bss`")),
        );
    }

    // The access width: stated, or implied by the register on the other side.
    let other = insn.ins.operands.get(1 - position.min(1)).filter(|_| insn.ins.operands.len() == 2);
    let bits = operand.size.or_else(|| other.and_then(Operand::as_reg).map(|reg| reg.width.bits()));
    if let (true, Some(bits), Some(data)) = (mem.direct, bits, &symbol.data)
        && let Some(size) = data.size
        && u64::from(bits / 8) > size
        && size > 0
    {
        let bytes = bits / 8;
        let verb = if writes { "writes" } else { "reads" };
        let plural = if size == 1 { "" } else { "s" };
        let mut diagnostic = Diagnostic::new(
            &DATA_SIZE_MISMATCH,
            operand.span,
            format!("{verb} {bytes} bytes at `{name}`, which is {size} byte{plural}"),
        )
        .label(if writes { "overwrites what follows it" } else { "also reads what follows it" })
        .note_at(symbol.span, format!("declared with `{}`", data.directive));
        // A load into a register: widen it explicitly.
        let keyword = match size {
            1 => Some("byte"),
            2 => Some("word"),
            _ => None,
        };
        if let (false, "mov", Some(dest), Some(keyword)) =
            (writes, insn.ins.mnemonic.as_str(), other.and_then(Operand::as_reg), keyword)
        {
            let replacement =
                format!("movzx {}, {keyword} {}", dest.id.name32(), cx.text(operand.span));
            diagnostic = diagnostic
                .replace(
                    format!("load {size} byte{plural} and zero-extend"),
                    insn.span,
                    replacement,
                )
                .maybe_incorrect();
        }
        out.push(diagnostic);
    }
}

fn immediate(cx: &Context, insn: &Insn, out: &mut Vec<Diagnostic>) {
    let mnemonic = insn.ins.mnemonic.as_str();
    let (Some(dest), Some(source)) = (insn.ins.operands.first(), insn.ins.operands.get(1)) else {
        return;
    };
    let (Some(bits), Some(value)) = (dest.bits(), source.constant()) else { return };
    if source.size.is_some() || insn.ins.operands.len() != 2 {
        return;
    }
    let text = cx.text(source.span);

    if SHIFTS.contains(&mnemonic) {
        // The processor masks the count to 5 bits, or 6 for 64-bit operands.
        let mask: i64 = if bits == 64 { 63 } else { 31 };
        if value < 0 || value > mask {
            out.push(
                Diagnostic::new(
                    &SHIFT_OUT_OF_RANGE,
                    source.span,
                    format!("a shift count of {value} is taken modulo {}", mask + 1),
                )
                .note(format!("this shifts by {}, not by {value}", value & mask)),
            );
        } else if value >= i64::from(bits) {
            out.push(
                Diagnostic::new(
                    &SHIFT_OUT_OF_RANGE,
                    source.span,
                    format!("shifting a {bits}-bit operand by {value} shifts every bit out"),
                )
                .help("to clear a register, use `xor`"),
            );
        }
        return;
    }

    if !(mnemonic == "mov" || ARITHMETIC.contains(&mnemonic)) {
        return;
    }
    if bits == 64 {
        // Only `mov r64, imm64` has a 64-bit immediate. Everything else
        // takes 32 bits and sign-extends, so `and rax, 0xFFFFFFFF` does
        // not mask.
        let wide = mnemonic == "mov" && dest.as_reg().is_some();
        if !wide && i32::try_from(value).is_err() {
            let mut diagnostic = Diagnostic::new(
                &IMMEDIATE_OUT_OF_RANGE,
                source.span,
                format!("`{text}` does not fit in the 32-bit immediate of `{mnemonic}`"),
            )
            .label(if u32::try_from(value).is_ok() {
                format!("sign-extended to {:#x}", value as u32 as i32 as i64)
            } else {
                "truncated to 32 bits".to_string()
            });
            // `and rax, 0xFFFFFFFF` means "keep the low half": a 32-bit
            // move of the register onto itself does exactly that.
            diagnostic = match (mnemonic, dest.as_reg(), value) {
                ("and", Some(reg), 0xFFFF_FFFF) => {
                    let low = reg.id.name32();
                    let replacement = format!("mov {low}, {low}");
                    diagnostic
                        .replace(
                            format!("keep the low 32 bits with `{replacement}`"),
                            insn.span,
                            replacement,
                        )
                        .maybe_incorrect()
                }
                _ => diagnostic.help("load the constant into a register with `mov` first"),
            };
            out.push(diagnostic);
        }
    } else if bits < 64 {
        let (min, max) = (-(1i64 << (bits - 1)), (1i64 << bits) - 1);
        if value < min || value > max {
            out.push(
                Diagnostic::new(
                    &IMMEDIATE_OUT_OF_RANGE,
                    source.span,
                    format!("`{text}` does not fit in {bits} bits"),
                )
                .label(format!("stored as {}", value & max)),
            );
        }
    }
}
