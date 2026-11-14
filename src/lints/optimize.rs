//! Optimisations that follow from understanding what the code does.
//!
//! Nothing here looks for a pattern of instructions in isolation. The
//! analyses work out what each register holds, what a comparison compares
//! and which flags something still reads, and only then say that an
//! instruction is useless or that a cheaper one does the same.

use std::collections::HashSet;

use super::Context;
use crate::abi;
use crate::ast::{Operand, OperandKind};
use crate::dataflow::{self, Analysis};
use crate::diagnostic::{Diagnostic, Edit, Group, Lint};
use crate::insn::{Flags, Flow, Target};
use crate::mnemonic::condition;
use crate::parser::parse_number;
use crate::program::{Callee, Function, Insn, Program};
use crate::reg::{Reg, RegId, RegSet, Width};

pub static REDUNDANT_LOAD: Lint = Lint {
    name: "redundant-load",
    group: Group::Performance,
    summary: "a register loaded with a value it already holds, such as `mov edi, 1` after a \
              `write` that left `edi` at 1",
};

pub static CONSTANT_CONDITION: Lint = Lint {
    name: "constant-condition",
    group: Group::Suspicious,
    summary: "a conditional jump whose outcome is known because both sides of the comparison \
              are constants",
};

pub static DIV_BY_POWER_OF_TWO: Lint = Lint {
    name: "div-by-power-of-two",
    group: Group::Performance,
    summary: "`div` by a register holding a power of two, which a shift and a mask replace",
};

pub static MUL_BY_POWER_OF_TWO: Lint = Lint {
    name: "mul-by-power-of-two",
    group: Group::Performance,
    summary: "`imul` by a power of two, which `shl` replaces",
};

pub static FOLD_CONSTANTS: Lint = Lint {
    name: "fold-constants",
    group: Group::Performance,
    summary: "two constant adjustments of a register in a row, such as `add rax, 1` then \
              `add rax, 2`",
};

pub static USE_LEA: Lint = Lint {
    name: "use-lea",
    group: Group::Performance,
    summary: "a `mov` followed by an `add` into the same register, which one `lea` does",
};

pub static REDUNDANT_MEMORY_LOAD: Lint = Lint {
    name: "redundant-memory-load",
    group: Group::Performance,
    summary: "the same memory loaded twice with nothing in between that could change it",
};

pub static MERGEABLE_STORES: Lint = Lint {
    name: "mergeable-stores",
    group: Group::Performance,
    summary: "consecutive byte stores of constants to adjacent addresses, which one wider \
              store does",
};

pub static JUMP_TO_RET: Lint = Lint {
    name: "jump-to-ret",
    group: Group::Performance,
    summary: "a `jmp` to a `ret`, which can return directly",
};

pub fn check(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;
    // Without a full picture of where control can come from, what a
    // register holds at a given point cannot be known.
    let indirect = program.insns.iter().any(|insn| {
        matches!(insn.fx.flow, Flow::Jump(Target::Indirect) | Flow::CondJump(Target::Indirect))
    });
    if !indirect && !program.opaque {
        dataflow::run(program, &Values { in_loop: loop_blocks(program) }, out);
    }
    folds(cx, out);
    leas(cx, out);
    multiplications(cx, out);
    memory_loads(cx, out);
    stores(cx, out);
    jumps_to_ret(cx, out);
}

// --- helpers ---------------------------------------------------------------

/// The instruction right before `index`, when control cannot enter between
/// them: same run, no label, and it falls straight through.
fn adjacent<'p>(program: &'p Program<'p>, index: usize) -> Option<&'p Insn<'p>> {
    let insn = &program.insns[index];
    let previous = program.insns.get(index.checked_sub(1)?)?;
    (insn.labels.is_empty()
        && previous.run == insn.run
        && previous.fx.known
        && previous.fx.flow == Flow::Next)
        .then_some(previous)
}

/// The flags that something may still read right after the instruction at
/// `index`, before they are overwritten. Everything the code might read
/// when it cannot be followed.
fn flags_needed_after(program: &Program, index: usize) -> Flags {
    let mut needed = Flags::NONE;
    let mut seen: HashSet<(usize, Flags)> = HashSet::new();
    let mut work = vec![(program.insns[index].block, index + 1, Flags::ALL)];
    while let Some((block, from, mut live)) = work.pop() {
        for insn in &program.insns[from..program.blocks[block].end] {
            if !insn.fx.known {
                needed = needed.union(live);
                live = Flags::NONE;
                break;
            }
            needed = needed.union(insn.fx.flags_read.intersection(live));
            live = live.difference(insn.fx.flags_written);
            // Nothing reads the flags across a call, a system call or a return.
            if matches!(insn.fx.flow, Flow::Call(_) | Flow::Syscall | Flow::Int(_) | Flow::Ret) {
                live = Flags::NONE;
            }
            if live.is_empty() {
                break;
            }
        }
        if live.is_empty() {
            continue;
        }
        for &succ in &program.blocks[block].succs {
            if seen.insert((succ, live)) {
                work.push((succ, program.blocks[succ].start, live));
            }
        }
    }
    needed
}

fn deletable(program: &Program, insn: &Insn) -> bool {
    insn.labels.is_empty() && program.lines[insn.line].label.is_none()
}

fn span_at(program: &Program, line: usize) -> Option<crate::span::Span> {
    program.at_line(line).map(|at| program.insns[at].span)
}

/// A number the way an assembly programmer writes it.
fn show(value: i64) -> String {
    if (-1024..1024).contains(&value) { value.to_string() } else { format!("{value:#x}") }
}

// --- what registers hold ----------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Known {
    Const(i64),
    /// The address of a symbol.
    Addr(String),
}

#[derive(Debug, Clone, PartialEq)]
struct Held {
    value: Known,
    /// The line that set it.
    line: usize,
}

/// The flags of a comparison whose operands are known.
#[derive(Debug, Clone, PartialEq)]
struct Cond {
    zf: bool,
    sf: bool,
    cf: bool,
    of: bool,
    /// "`rax` is 1", for the message.
    why: String,
    line: usize,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct State {
    regs: [Option<Held>; 16],
    cond: Option<Cond>,
}

struct Values {
    /// Which blocks can run again after they have run.
    in_loop: Vec<bool>,
}

/// For each block, whether control can come back to it.
fn loop_blocks(program: &Program) -> Vec<bool> {
    let count = program.blocks.len();
    (0..count)
        .map(|start| {
            let mut seen = vec![false; count];
            let mut stack = program.blocks[start].succs.clone();
            while let Some(block) = stack.pop() {
                if block == start {
                    return true;
                }
                if !std::mem::replace(&mut seen[block], true) {
                    stack.extend(&program.blocks[block].succs);
                }
            }
            false
        })
        .collect()
}

impl Analysis for Values {
    type State = State;

    fn entry(&self, _: &Program, _: &Function) -> State {
        State::default()
    }

    fn unknown(&self) -> State {
        State::default()
    }

    /// A register keeps its value only when every path agrees on it.
    fn join(&self, a: &State, b: &State) -> State {
        let regs = std::array::from_fn(|i| match (&a.regs[i], &b.regs[i]) {
            (Some(x), Some(y)) if x.value == y.value => Some(x.clone()),
            _ => None,
        });
        let cond = match (&a.cond, &b.cond) {
            (Some(x), Some(y)) if (x.zf, x.sf, x.cf, x.of) == (y.zf, y.sf, y.cf, y.of) => {
                Some(x.clone())
            }
            _ => None,
        };
        State { regs, cond }
    }

    fn transfer(
        &self,
        program: &Program,
        index: usize,
        state: &mut State,
        out: Option<&mut Vec<Diagnostic>>,
    ) {
        let insn = &program.insns[index];
        let fx = &insn.fx;
        let defined = insn
            .ins
            .operands
            .iter()
            .flat_map(|operand| operand.symbols())
            .any(|symbol| program.defines.contains(symbol));
        if !fx.known || defined {
            *state = State::default();
            return;
        }
        if let Some(out) = out {
            report(program, index, state, self.in_loop[insn.block], out);
        }

        // Computed from the state before the instruction runs.
        let result = result_of(program, insn, state);
        let compared = compare_of(insn, state);

        for id in fx.writes.iter() {
            state.regs[id.index()] = None;
        }
        if !fx.flags_written.is_empty() {
            state.cond = None;
        }
        match &fx.flow {
            // The kernel preserves everything but the result and the two
            // registers `syscall` itself uses.
            Flow::Syscall => {
                for id in [RegId::RAX, RegId::RCX, RegId::R11] {
                    state.regs[id.index()] = None;
                }
                state.cond = None;
            }
            Flow::Int(_) => {
                state.regs[RegId::RAX.index()] = None;
                state.cond = None;
            }
            Flow::Call(target) => {
                let lost = match program.callee(target) {
                    Callee::Local(callee) => {
                        let function = &program.functions[callee];
                        function.clobbers.union(function.garbage())
                    }
                    Callee::External => abi::CALLER_SAVED,
                };
                for id in lost.iter() {
                    state.regs[id.index()] = None;
                }
                state.cond = None;
            }
            _ => {}
        }
        if let Some((id, value)) = result {
            state.regs[id.index()] = Some(Held { value, line: insn.line });
        }
        if let Some(cond) = compared {
            state.cond = Some(cond);
        }
    }
}

fn constant_of(state: &State, reg: Reg) -> Option<i64> {
    match &state.regs[reg.id.index()].as_ref()?.value {
        Known::Const(value) => Some(*value),
        Known::Addr(_) => None,
    }
}

/// The value the instruction leaves in its destination, when it is plain
/// enough to tell: a load of a constant or an address, a copy of a known
/// register, or arithmetic on a known register.
fn result_of(program: &Program, insn: &Insn, state: &State) -> Option<(RegId, Known)> {
    let operands = &insn.ins.operands;
    let dest = operands.first()?.as_reg()?;
    let source = operands.get(1);
    let mnemonic = insn.ins.mnemonic.as_str();
    let held = |reg: Reg| state.regs[reg.id.index()].as_ref().map(|held| held.value.clone());
    let wide = dest.width == Width::W64;

    let value = match mnemonic {
        "xor" | "sub" if insn.fx.zeroing => Known::Const(0),
        "mov" => match &source?.kind {
            OperandKind::Imm(expr) => match (expr.value, expr.symbols.as_slice()) {
                (Some(value), _) => match dest.width {
                    Width::W64 => Known::Const(value),
                    Width::W32 => Known::Const(value as u32 as i64),
                    Width::W16 | Width::Low8 => {
                        let mask = if dest.width == Width::W16 { 0xFFFF } else { 0xFF };
                        let Known::Const(old) = held(dest)? else { return None };
                        Known::Const((old & !mask) | (value & mask))
                    }
                    Width::High8 => return None,
                },
                (None, [name]) if wide && program.is_plain(source?, name) => {
                    Known::Addr(name.clone())
                }
                _ => return None,
            },
            OperandKind::Reg(from) => match (dest.width, from.width) {
                (Width::W64, Width::W64) => held(*from)?,
                (Width::W32, Width::W32) => match held(*from)? {
                    Known::Const(value) => Known::Const(value as u32 as i64),
                    Known::Addr(_) => return None,
                },
                _ => return None,
            },
            _ => return None,
        },
        "lea" if wide => match &source?.kind {
            OperandKind::Mem(mem) if mem.direct => Known::Addr(mem.symbols[0].clone()),
            _ => return None,
        },
        "add" | "sub" | "and" | "or" | "xor" | "inc" | "dec" | "shl" | "shr"
            if dest.width.is_full() =>
        {
            let Known::Const(a) = held(dest)? else { return None };
            let b = match mnemonic {
                "inc" | "dec" => 1,
                _ => {
                    let source = source?;
                    match source.as_reg() {
                        Some(reg) if reg.width == dest.width => constant_of(state, reg)?,
                        Some(_) => return None,
                        None => source.constant()?,
                    }
                }
            };
            if wide {
                Known::Const(match mnemonic {
                    "add" | "inc" => a.wrapping_add(b),
                    "sub" | "dec" => a.wrapping_sub(b),
                    "and" => a & b,
                    "or" => a | b,
                    "xor" => a ^ b,
                    "shl" => a.wrapping_shl((b & 63) as u32),
                    _ => ((a as u64) >> (b & 63)) as i64,
                })
            } else {
                let (a, b) = (a as u32, b as u32);
                Known::Const(i64::from(match mnemonic {
                    "add" | "inc" => a.wrapping_add(b),
                    "sub" | "dec" => a.wrapping_sub(b),
                    "and" => a & b,
                    "or" => a | b,
                    "xor" => a ^ b,
                    "shl" => a.wrapping_shl(b & 31),
                    _ => a >> (b & 31),
                }))
            }
        }
        _ => return None,
    };
    Some((dest.id, value))
}

/// The flags of a `cmp` or `test` whose operands are both known.
fn compare_of(insn: &Insn, state: &State) -> Option<Cond> {
    let test = match insn.ins.mnemonic.as_str() {
        "cmp" => false,
        "test" => true,
        _ => return None,
    };
    let [a, b] = insn.ins.operands.as_slice() else { return None };
    let first = a.as_reg().filter(|reg| reg.width.is_full())?;
    let bits = first.width.bits();
    let left = constant_of(state, first)?;
    let mut why = vec![format!("`{first}` is {}", show_width(left, bits))];
    let right = match &b.kind {
        OperandKind::Imm(_) => {
            let value = b.constant()?;
            // A 64-bit comparison only has a 32-bit immediate.
            if bits == 64 && i32::try_from(value).is_err() {
                return None;
            }
            value
        }
        OperandKind::Reg(reg) if reg.width == first.width => {
            let value = constant_of(state, *reg)?;
            why.push(format!("`{reg}` is {}", show_width(value, bits)));
            value
        }
        _ => return None,
    };

    let mask = if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 };
    let sign = 1u64 << (bits - 1);
    let (a, b) = (left as u64 & mask, right as u64 & mask);
    let (result, cf, of) = if test {
        (a & b, false, false)
    } else {
        let result = a.wrapping_sub(b) & mask;
        (result, a < b, (a ^ b) & (a ^ result) & sign != 0)
    };
    Some(Cond {
        zf: result == 0,
        sf: result & sign != 0,
        cf,
        of,
        why: why.join(" and "),
        line: insn.line,
    })
}

fn show_width(value: i64, bits: u32) -> String {
    show(if bits == 32 { i64::from(value as u32) } else { value })
}

/// Whether a condition code holds, when the flags are known.
fn holds(code: &str, cond: &Cond) -> Option<bool> {
    Some(match code {
        "e" | "z" => cond.zf,
        "ne" | "nz" => !cond.zf,
        "l" | "nge" => cond.sf != cond.of,
        "ge" | "nl" => cond.sf == cond.of,
        "le" | "ng" => cond.zf || cond.sf != cond.of,
        "g" | "nle" => !cond.zf && cond.sf == cond.of,
        "b" | "c" | "nae" => cond.cf,
        "ae" | "nb" | "nc" => !cond.cf,
        "be" | "na" => cond.cf || cond.zf,
        "a" | "nbe" => !cond.cf && !cond.zf,
        "s" => cond.sf,
        "ns" => !cond.sf,
        "o" => cond.of,
        "no" => !cond.of,
        _ => return None,
    })
}

fn report(
    program: &Program,
    index: usize,
    state: &State,
    in_loop: bool,
    out: &mut Vec<Diagnostic>,
) {
    let insn = &program.insns[index];
    let mnemonic = insn.ins.mnemonic.as_str();

    // A register loaded with what it already holds. Outside a loop the
    // cost is one instruction, once, and reloading the arguments before each
    // system call is a common way to keep code readable: only a loop,
    // where the load runs on every iteration, is worth a remark.
    if in_loop
        && matches!(mnemonic, "mov" | "lea" | "xor" | "sub")
        && let Some((id, known)) = result_of(program, insn, state)
        && id != RegId::RSP
        && let Some(held) = &state.regs[id.index()]
        && held.value == known
        && let Some(dest) = insn.fx.dest
        // `xor r, r` also sets the flags: only drop it when nothing reads them.
        && (!insn.fx.zeroing || flags_needed_after(program, index).is_empty())
    {
        let name = id.name64();
        let what = match &known {
            Known::Const(value) => show(*value),
            Known::Addr(symbol) => format!("the address of `{symbol}`"),
        };
        let mut diagnostic =
            Diagnostic::new(&REDUNDANT_LOAD, insn.span, format!("`{name}` already holds {what}"))
                .label(format!("`{dest}` is loaded again on every iteration"));
        if held.line != insn.line
            && let Some(span) = span_at(program, held.line)
        {
            diagnostic = diagnostic.note_at(span, format!("`{name}` is set here"));
        }
        if deletable(program, insn) {
            diagnostic = diagnostic.delete_line("remove the redundant load", insn.line);
        }
        out.push(diagnostic);
    }

    // A jump whose condition is already decided.
    if let Some(("j", code)) = condition(mnemonic)
        && let Some(cond) = &state.cond
        && let Some(taken) = holds(code, cond)
    {
        let mut diagnostic = Diagnostic::new(
            &CONSTANT_CONDITION,
            insn.span,
            format!(
                "`{mnemonic}` is {}: {}",
                if taken { "always taken" } else { "never taken" },
                cond.why
            ),
        )
        .label("the outcome never changes");
        if let Some(span) = span_at(program, cond.line) {
            diagnostic = diagnostic.note_at(span, "compared here");
        }
        diagnostic = if taken {
            diagnostic
                .replace("jump unconditionally", insn.ins.mnemonic_span, "jmp")
                .maybe_incorrect()
        } else if deletable(program, insn) {
            diagnostic.delete_line("remove the jump", insn.line).maybe_incorrect()
        } else {
            diagnostic
        };
        out.push(diagnostic);
    }

    // An unsigned division by a power of two.
    if mnemonic == "div"
        && let [operand] = insn.ins.operands.as_slice()
        && let Some(divisor) = operand.as_reg().filter(|reg| reg.width == Width::W64)
        && !matches!(divisor.id, RegId::RAX | RegId::RDX)
        && let Some(Held { value: Known::Const(value), line }) = &state.regs[divisor.id.index()]
        && (2..=(1i64 << 31)).contains(value)
        && value.count_ones() == 1
        && matches!(&state.regs[RegId::RDX.index()], Some(Held { value: Known::Const(0), .. }))
    {
        let (shift, mask) = (value.trailing_zeros(), value - 1);
        let mut edits = vec![Edit::Replace {
            span: insn.span,
            text: format!("mov rdx, rax\nand rdx, {mask}\nshr rax, {shift}"),
        }];
        // The `xor edx, edx` that prepared the division has nothing left to do.
        if let Some(previous) = index.checked_sub(1).map(|at| &program.insns[at])
            && previous.run == insn.run
            && insn.labels.is_empty()
            && previous.fx.zeroing
            && previous.fx.dest.is_some_and(|dest| dest.id == RegId::RDX)
            && deletable(program, previous)
        {
            edits.insert(0, Edit::DeleteLine(previous.line));
        }
        let mut diagnostic = Diagnostic::new(
            &DIV_BY_POWER_OF_TWO,
            insn.span,
            format!("unsigned division by {value}"),
        )
        .label("a shift and a mask give the same quotient and remainder");
        if let Some(span) = span_at(program, *line) {
            diagnostic = diagnostic.note_at(span, format!("`{divisor}` is {value} here"));
        }
        out.push(diagnostic.fix(
            format!("use `shr rax, {shift}` for the quotient and `and` for the remainder"),
            edits,
        ));
    }
}

// --- arithmetic -------------------------------------------------------------

/// `add r, 3`, `sub r, 1`, `inc r`, `dec r` as a signed adjustment of `r`.
fn adjustment(insn: &Insn) -> Option<(Reg, i64)> {
    let operands = &insn.ins.operands;
    let reg = operands.first()?.as_reg().filter(|reg| reg.width.is_full())?;
    match (insn.ins.mnemonic.as_str(), operands.len()) {
        ("inc", 1) => Some((reg, 1)),
        ("dec", 1) => Some((reg, -1)),
        ("add", 2) => Some((reg, operands[1].constant()?)),
        ("sub", 2) => Some((reg, operands[1].constant()?.checked_neg()?)),
        _ => None,
    }
}

fn adjust_text(reg: Reg, total: i64) -> Option<String> {
    let magnitude = i32::try_from(total.checked_abs()?).ok()?;
    Some(if total > 0 {
        format!("add {reg}, {magnitude}")
    } else {
        format!("sub {reg}, {magnitude}")
    })
}

fn folds(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;
    let results_only = Flags::ZF.union(Flags::SF).union(Flags::PF);
    // The second instruction of the last pair reported: a chain is
    // reported one pair at a time.
    let mut last: Option<usize> = None;

    for (index, insn) in program.insns.iter().enumerate() {
        let Some(previous) = adjacent(program, index) else { continue };
        if !insn.fx.known || cx.uses_define(insn) || cx.uses_define(previous) {
            continue;
        }
        let Some((reg, second)) = adjustment(insn) else { continue };
        let needed = flags_needed_after(program, index);

        let plan: Option<(String, Vec<Edit>)> =
            if let Some((other, first)) = adjustment(previous).filter(|(other, _)| *other == reg) {
                let _ = other;
                if last == Some(index - 1) {
                    continue;
                }
                match first.checked_add(second) {
                    // The two cancel out: dropping both leaves the flags as they were.
                    Some(0) if needed.is_empty() && deletable(program, previous) => Some((
                        String::new(),
                        vec![Edit::DeleteLine(previous.line), Edit::DeleteLine(insn.line)],
                    )),
                    // Zero, sign and parity depend only on the final value.
                    Some(total) if total != 0 && needed.difference(results_only).is_empty() => {
                        adjust_text(reg, total).map(|text| {
                            let edits = vec![
                                Edit::Replace { span: previous.span, text: text.clone() },
                                Edit::DeleteLine(insn.line),
                            ];
                            (text, edits)
                        })
                    }
                    _ => None,
                }
            } else if previous.ins.mnemonic == "mov"
                && let [dest, source] = previous.ins.operands.as_slice()
                && dest.as_reg() == Some(reg)
                && let Some(base) = source.constant()
                && needed.is_empty()
            {
                let value = if reg.width == Width::W64 {
                    base.wrapping_add(second)
                } else {
                    i64::from((base as u32).wrapping_add(second as u32))
                };
                if last == Some(index - 1) {
                    continue;
                }
                let text = format!("mov {reg}, {}", show(value));
                let edits = vec![
                    Edit::Replace { span: previous.span, text: text.clone() },
                    Edit::DeleteLine(insn.line),
                ];
                Some((text, edits))
            } else {
                None
            };
        let Some((text, edits)) = plan else { continue };

        let (previous_code, code) = (cx.code(previous), cx.code(insn));
        let diagnostic = if text.is_empty() {
            Diagnostic::new(
                &FOLD_CONSTANTS,
                previous.span,
                format!("`{previous_code}` and `{code}` cancel out"),
            )
            .label("together they change nothing")
            .note_at(insn.span, "undone here")
            .fix("remove both", edits)
        } else {
            Diagnostic::new(
                &FOLD_CONSTANTS,
                previous.span,
                format!("`{previous_code}` and `{code}` can be one instruction"),
            )
            .label(format!("`{text}` does both"))
            .note_at(insn.span, "merged with this one")
            .fix(format!("use `{text}`"), edits)
        };
        out.push(diagnostic);
        last = Some(index);
    }
}

fn leas(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;
    for (index, insn) in program.insns.iter().enumerate() {
        let Some(previous) = adjacent(program, index) else { continue };
        if !insn.fx.known || cx.uses_define(insn) || cx.uses_define(previous) {
            continue;
        }
        let ([target, source], "add" | "sub") =
            (insn.ins.operands.as_slice(), insn.ins.mnemonic.as_str())
        else {
            continue;
        };
        let Some(dest) = target.as_reg().filter(|reg| reg.width.is_full()) else { continue };
        // `mov dest, base` right before.
        let [copy_to, copy_from] = previous.ins.operands.as_slice() else { continue };
        let (Some(to), Some(base)) = (copy_to.as_reg(), copy_from.as_reg()) else { continue };
        if previous.ins.mnemonic != "mov"
            || to != dest
            || base.width != dest.width
            || base.id == dest.id
        {
            continue;
        }
        let base_name = base.id.name64();
        let term = match (&source.kind, insn.ins.mnemonic.as_str()) {
            (OperandKind::Reg(reg), "add") if reg.width == dest.width => {
                // `add dest, dest` after the copy adds `base` twice.
                let id = if reg.id == dest.id { base.id } else { reg.id };
                format!("+ {}", id.name64())
            }
            (OperandKind::Imm(_), mnemonic) => {
                let Some(value) = source.constant().filter(|value| i32::try_from(*value).is_ok())
                else {
                    continue;
                };
                let delta = if mnemonic == "add" { value } else { -value };
                if delta < 0 { format!("- {}", -delta) } else { format!("+ {delta}") }
            }
            _ => continue,
        };
        if !flags_needed_after(program, index).is_empty() {
            continue;
        }
        let text = format!("lea {dest}, [{base_name} {term}]");
        out.push(
            Diagnostic::new(
                &USE_LEA,
                previous.span,
                format!("`{}` and `{}` can be one `lea`", cx.code(previous), cx.code(insn)),
            )
            .label("`lea` adds without needing the copy")
            .note_at(insn.span, "merged with this one")
            .fix(
                format!("use `{text}`"),
                vec![Edit::Replace { span: previous.span, text }, Edit::DeleteLine(insn.line)],
            ),
        );
    }
}

fn multiplications(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;
    for (index, insn) in program.insns.iter().enumerate() {
        if insn.ins.mnemonic != "imul" || !insn.fx.known || cx.uses_define(insn) {
            continue;
        }
        let operands = insn.ins.operands.as_slice();
        let (dest, factor) = match operands {
            [dest, factor] => (dest.as_reg(), factor.constant()),
            [dest, source, factor]
                if dest.as_reg().is_some() && dest.as_reg() == source.as_reg() =>
            {
                (dest.as_reg(), factor.constant())
            }
            _ => continue,
        };
        let (Some(dest), Some(factor)) = (dest, factor) else { continue };
        if factor < 2 || factor.count_ones() != 1 || factor.trailing_zeros() >= dest.width.bits() {
            continue;
        }
        // `imul` sets the carry and overflow flags on overflow, `shl` does not.
        if !flags_needed_after(program, index).is_empty() {
            continue;
        }
        let shift = factor.trailing_zeros();
        let text = format!("shl {dest}, {shift}");
        out.push(
            Diagnostic::new(&MUL_BY_POWER_OF_TWO, insn.span, format!("multiplication by {factor}"))
                .label("a shift does the same and is faster")
                .replace(format!("use `{text}`"), insn.span, text),
        );
    }
}

// --- memory -----------------------------------------------------------------

/// Whether the instruction may change memory, which ends what is known
/// about the contents of any address.
fn writes_memory(insn: &Insn) -> bool {
    let mnemonic = insn.ins.mnemonic.as_str();
    let first_is_memory = insn.ins.operands.first().is_some_and(Operand::is_mem);
    !insn.fx.known
        || matches!(insn.fx.flow, Flow::Call(_) | Flow::Syscall | Flow::Int(_))
        || insn.ins.prefix.is_some()
        || matches!(mnemonic, "push" | "pushf" | "pushfq" | "enter" | "xchg" | "xadd" | "cmpxchg")
        || (mnemonic.len() == 5 && (mnemonic.starts_with("movs") || mnemonic.starts_with("stos")))
        || (first_is_memory && !matches!(mnemonic, "cmp" | "test" | "bt" | "lea" | "jmp" | "call"))
}

struct Loaded {
    reg: Reg,
    key: String,
    uses: RegSet,
    line: usize,
}

fn memory_loads(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;
    for block in &program.blocks {
        let mut loaded: Vec<Loaded> = Vec::new();
        for index in block.start..block.end {
            let insn = &program.insns[index];
            if !insn.fx.known || cx.uses_define(insn) {
                loaded.clear();
                continue;
            }
            let load = match (insn.ins.mnemonic.as_str(), insn.ins.operands.as_slice()) {
                ("mov", [dest, source]) => match (dest.as_reg(), &source.kind) {
                    (Some(dest), OperandKind::Mem(mem)) => {
                        let text: String = cx
                            .text(source.span)
                            .chars()
                            .filter(|c| !c.is_whitespace())
                            .collect::<String>()
                            .to_lowercase();
                        Some((dest, format!("{}:{text}", dest.width.bits()), mem))
                    }
                    _ => None,
                },
                _ => None,
            };

            if let Some((dest, key, _)) = &load
                && let Some(earlier) = loaded.iter().find(|earlier| earlier.key == *key)
                && let Some(earlier_span) = span_at(program, earlier.line)
            {
                let source = insn.ins.operands[1].span;
                let operand = cx.text(source);
                let mut diagnostic = if earlier.reg.id == dest.id {
                    let mut diagnostic = Diagnostic::new(
                        &REDUNDANT_MEMORY_LOAD,
                        insn.span,
                        format!("`{operand}` is already in `{dest}`"),
                    )
                    .label("nothing changed it since the last load");
                    if deletable(program, insn) {
                        diagnostic = diagnostic.delete_line("remove the second load", insn.line);
                    }
                    diagnostic
                } else {
                    let text = format!("mov {dest}, {}", earlier.reg);
                    Diagnostic::new(
                        &REDUNDANT_MEMORY_LOAD,
                        insn.span,
                        format!("`{operand}` is already in `{}`", earlier.reg),
                    )
                    .label("copy the register instead of reading memory again")
                    .replace(format!("use `{text}`"), insn.span, text)
                };
                diagnostic = diagnostic.note_at(earlier_span, "loaded here");
                out.push(diagnostic);
            }

            if writes_memory(insn) {
                loaded.clear();
            }
            loaded.retain(|earlier| {
                !insn.fx.writes.contains(earlier.reg.id)
                    && insn.fx.writes.intersection(earlier.uses).is_empty()
            });
            if let Some((dest, key, mem)) = load {
                let mut uses = RegSet::EMPTY;
                mem.regs.iter().for_each(|reg| uses.insert(reg.id));
                // `mov rax, [rax]` overwrites its own address.
                if !uses.contains(dest.id) {
                    loaded.push(Loaded { reg: dest, key, uses, line: insn.line });
                }
            }
        }
    }
}

/// `[buffer + r15 + 1]` as the part that names the address and the
/// constant offset: `("buffer + r15", 1)`.
fn split_displacement(text: &str) -> Option<(String, i64)> {
    let inner = text.get(text.find('[')? + 1..text.rfind(']')?)?.trim();
    if inner.contains(['(', ')']) {
        return None;
    }
    let (prefix, body) = match inner.strip_prefix("rel ") {
        Some(rest) => ("rel ", rest.trim()),
        None => ("", inner),
    };
    let mut terms: Vec<(bool, &str)> = Vec::new();
    let (mut start, mut negative) = (0, false);
    for (at, byte) in body.bytes().enumerate().chain(std::iter::once((body.len(), b'+'))) {
        if byte == b'+' || byte == b'-' {
            let term = body[start..at].trim();
            if !term.is_empty() {
                terms.push((negative, term));
            }
            negative = byte == b'-';
            start = at + 1;
        }
    }
    let (mut offset, mut base) = (0i64, Vec::new());
    for (negative, term) in terms {
        // Numbers start with a digit: `abh` is a label, not 0xab.
        let number = term.as_bytes()[0].is_ascii_digit().then(|| parse_number(term)).flatten();
        match number {
            Some(value) => {
                offset = offset.checked_add(if negative { value.checked_neg()? } else { value })?;
            }
            None if negative => return None,
            None => base.push(term),
        }
    }
    (!base.is_empty()).then(|| (format!("{prefix}{}", base.join(" + ")), offset))
}

/// `mov byte [address + offset], constant`.
fn byte_store(cx: &Context, index: usize) -> Option<(String, i64, u8)> {
    let insn = &cx.program.insns[index];
    let [dest, source] = insn.ins.operands.as_slice() else { return None };
    if insn.ins.mnemonic != "mov" || insn.ins.prefix.is_some() || cx.uses_define(insn) {
        return None;
    }
    if !dest.is_mem() || dest.size != Some(8) || source.size.is_some() {
        return None;
    }
    let value = source.constant()?;
    let byte = u8::try_from(value).ok().or_else(|| i8::try_from(value).ok().map(|v| v as u8))?;
    let (base, offset) = split_displacement(cx.text(dest.span))?;
    Some((base, offset, byte))
}

/// The constant for a merged store: the characters when they read as text,
/// the number otherwise. NASM puts the first character in the lowest byte,
/// which is also the lowest address.
fn literal(bytes: &[u8]) -> String {
    let text = bytes.iter().all(|byte| (0x20..0x7F).contains(byte) && *byte != b'\'');
    if text {
        format!("'{}'", String::from_utf8_lossy(bytes))
    } else {
        let value =
            bytes.iter().enumerate().fold(0u64, |acc, (i, byte)| acc | u64::from(*byte) << (8 * i));
        format!("{value:#x}")
    }
}

fn stores(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;
    let mut index = 0;
    while index < program.insns.len() {
        let Some((base, offset, byte)) = byte_store(cx, index) else {
            index += 1;
            continue;
        };
        let mut group = vec![(index, byte)];
        while let next = index + group.len()
            && next < program.insns.len()
            && program.insns[next].labels.is_empty()
            && program.insns[next].run == program.insns[index].run
            && let Some((next_base, next_offset, next_byte)) = byte_store(cx, next)
            && next_base == base
            && next_offset == offset + group.len() as i64
        {
            group.push((next, next_byte));
        }
        let consumed = group.len();
        let mergeable = consumed >= 2
            && group[1..].iter().all(|(at, _)| deletable(program, &program.insns[*at]));
        if mergeable {
            let mut edits = Vec::new();
            let (mut at, mut first_text) = (0, String::new());
            while consumed - at >= 2 {
                let take = if consumed - at >= 4 { 4 } else { 2 };
                let chunk = &group[at..at + take];
                let bytes: Vec<u8> = chunk.iter().map(|(_, byte)| *byte).collect();
                let start = offset + at as i64;
                let place = match start {
                    0 => String::new(),
                    value if value > 0 => format!(" + {value}"),
                    value => format!(" - {}", -value),
                };
                let size = if take == 4 { "dword" } else { "word" };
                let text = format!("mov {size} [{base}{place}], {}", literal(&bytes));
                if first_text.is_empty() {
                    first_text.clone_from(&text);
                }
                edits.push(Edit::Replace { span: program.insns[chunk[0].0].span, text });
                for (rest, _) in &chunk[1..] {
                    edits.push(Edit::DeleteLine(program.insns[*rest].line));
                }
                at += take;
            }
            let first = &program.insns[group[0].0];
            let last = &program.insns[group[consumed - 1].0];
            out.push(
                Diagnostic::new(
                    &MERGEABLE_STORES,
                    first.span,
                    format!("{consumed} adjacent byte stores of constants"),
                )
                .label("one wider store writes them all at once")
                .note_at(last.span, "up to here")
                .fix(format!("use `{first_text}`"), edits),
            );
        }
        index += consumed;
    }
}

fn jumps_to_ret(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;
    for (index, insn) in program.insns.iter().enumerate() {
        let Flow::Jump(Target::Label(name)) = &insn.fx.flow else { continue };
        let Some(target) = program.symbols.get(name).and_then(|symbol| symbol.insn) else {
            continue;
        };
        let landing = &program.insns[target];
        if target == index + 1 || landing.fx.flow != Flow::Ret || cx.uses_define(landing) {
            continue;
        }
        let text = cx.code(landing).to_string();
        out.push(
            Diagnostic::new(&JUMP_TO_RET, insn.span, "`jmp` to a `ret`")
                .label("return from here instead")
                .note_at(landing.span, "the jump lands on this return")
                .replace(format!("use `{text}`"), insn.span, text),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    #[test]
    fn splits_displacements() {
        let split = |text: &str| split_displacement(text);
        assert_eq!(split("byte [buffer + r15 + 1]"), Some(("buffer + r15".into(), 1)));
        assert_eq!(split("[buffer]"), Some(("buffer".into(), 0)));
        assert_eq!(split("[rel msg + 0x10 - 2]"), Some(("rel msg".into(), 14)));
        assert_eq!(split("[rdi - 8]"), Some(("rdi".into(), -8)));
        assert_eq!(split("[abh + 1]"), Some(("abh".into(), 1)));
        assert_eq!(split("[16]"), None);
        assert_eq!(split("[rax*2 + (3)]"), None);
    }

    #[test]
    fn literals() {
        assert_eq!(literal(b"H4CK"), "'H4CK'");
        assert_eq!(literal(&[0x41, 0x00]), "0x41");
        assert_eq!(literal(b"'a"), "0x6127");
    }

    #[test]
    fn flags_needed_after_follows_the_code() {
        let needed = |source: &str, index: usize| {
            let lines = parse(source);
            let program = Program::build(&lines);
            flags_needed_after(&program, index)
        };
        assert_eq!(needed("f:\n add rax, 1\n mov rbx, 2\n jz f\n ret\n", 0), Flags::ZF);
        assert_eq!(needed("f:\n add rax, 1\n cmp rax, 2\n jz f\n ret\n", 0), Flags::NONE);
        assert_eq!(needed("f:\n add rax, 1\n ret\n", 0), Flags::NONE);
        // Read on one path, overwritten on the other.
        assert_eq!(
            needed("f:\n add rax, 1\n jz g\n cmp rax, 1\ng:\n jc f\n ret\n", 0),
            Flags::ZF.union(Flags::CF)
        );
        assert_eq!(needed("f:\n add rax, 1\n call g\n jz f\ng:\n ret\n", 0), Flags::NONE);
    }

    #[test]
    fn compares_like_the_processor() {
        let cond = |a: i64, b: i64, bits: u32, test: bool| {
            let mask = if bits == 64 { u64::MAX } else { (1u64 << bits) - 1 };
            let sign = 1u64 << (bits - 1);
            let (a, b) = (a as u64 & mask, b as u64 & mask);
            let result = if test { a & b } else { a.wrapping_sub(b) & mask };
            let (cf, of) =
                if test { (false, false) } else { (a < b, (a ^ b) & (a ^ result) & sign != 0) };
            Cond { zf: result == 0, sf: result & sign != 0, cf, of, why: String::new(), line: 0 }
        };
        // -1 < 1 signed, but 0xFFFF... > 1 unsigned.
        let c = cond(-1, 1, 64, false);
        assert_eq!(
            (holds("l", &c), holds("b", &c), holds("a", &c)),
            (Some(true), Some(false), Some(true))
        );
        let c = cond(i64::MIN, 1, 64, false);
        assert_eq!(holds("l", &c), Some(true)); // overflow case
        let c = cond(5, 5, 32, false);
        assert_eq!(
            (holds("e", &c), holds("g", &c), holds("ae", &c)),
            (Some(true), Some(false), Some(true))
        );
        let c = cond(6, 3, 64, true);
        assert_eq!(holds("z", &c), Some(false));
        assert_eq!(holds("p", &c), None);
    }
}
