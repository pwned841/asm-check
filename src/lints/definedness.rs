//! Which registers hold a meaningful value, at every point of the program.
//!
//! One dataflow analysis serves three lints: a register read before
//! anything wrote it, a register read after a call or a system call
//! destroyed it, and a full register read when only its low bits were set.

use std::cell::RefCell;

use super::Context;
use crate::abi;
use crate::ast::OperandKind;
use crate::dataflow::{self, Analysis};
use crate::diagnostic::{Diagnostic, Group, Lint};
use crate::insn::Flow;
use crate::program::{Callee, Function, Program};
use crate::reg::{RegId, RegSet, Width};
use crate::span::Span;
use crate::syscalls::{self, Syscall};

pub static UNINITIALIZED_REGISTER: Lint = Lint {
    name: "uninitialized-register",
    group: Group::Suspicious,
    summary: "a register read before anything wrote it, including a system call argument \
              that was never set",
};

pub static CLOBBERED_REGISTER: Lint = Lint {
    name: "clobbered-register",
    group: Group::Suspicious,
    summary: "a register read after a `syscall` or a call destroyed its value",
};

pub static PARTIAL_REGISTER_READ: Lint = Lint {
    name: "partial-register-read",
    group: Group::Suspicious,
    summary: "a whole register read when only its low bits were written, such as \
              `mov al, [rsi]` followed by `cmp rax, 10`",
};

/// How a register lost its value. Lines are 0-based.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Cause {
    /// Never written since the program started.
    Entry,
    Syscall(usize),
    /// A call, to a local function when its index is known.
    Call(usize, Option<usize>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Def {
    Defined,
    /// Whatever the caller left there: a parameter, as far as we know.
    Input,
    /// Only the low bits were written since the register became undefined.
    /// The last field is the line of that write.
    Partial(Width, Cause, usize),
    Undefined(Cause),
}

impl Def {
    /// Position in the lattice: a join keeps the worst state.
    fn rank(self) -> u8 {
        match self {
            Def::Defined => 0,
            Def::Input => 1,
            Def::Partial(..) => 2,
            Def::Undefined(_) => 3,
        }
    }
}

type State = [Def; 16];

pub fn check(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;
    // First find what each function reads from its caller. A function's
    // inputs include those of the functions it calls, so iterate.
    let mut inputs = vec![RegSet::EMPTY; program.functions.len()];
    for _ in 0..program.functions.len() + 1 {
        let analysis = Definedness { inputs: &inputs, found: RefCell::new(inputs.clone()) };
        dataflow::run(program, &analysis, &mut Vec::new());
        let found = analysis.found.into_inner();
        if found == inputs {
            break;
        }
        inputs = found;
    }
    let analysis = Definedness { inputs: &inputs, found: RefCell::new(inputs.clone()) };
    dataflow::run(program, &analysis, out);
}

struct Definedness<'a> {
    /// Per function, the registers it reads before writing them.
    inputs: &'a [RegSet],
    found: RefCell<Vec<RegSet>>,
}

/// Why an instruction reads a register.
#[derive(Debug, Clone, Copy)]
enum Reason {
    Operand,
    Implicit,
    SyscallNumber,
    SyscallArgument(Syscall, usize),
    Callee(usize),
}

struct Read {
    id: RegId,
    /// Bits read, counted from bit 0. Zero when the width is not known.
    bits: u32,
    span: Span,
    reason: Reason,
}

impl Analysis for Definedness<'_> {
    type State = State;

    fn entry(&self, _: &Program, function: &Function) -> State {
        if !function.entry_point {
            return [Def::Input; 16];
        }
        let mut state = [Def::Undefined(Cause::Entry); 16];
        state[RegId::RSP.index()] = Def::Defined;
        state
    }

    fn unknown(&self) -> State {
        [Def::Defined; 16]
    }

    fn join(&self, a: &State, b: &State) -> State {
        std::array::from_fn(|i| if b[i].rank() > a[i].rank() { b[i] } else { a[i] })
    }

    fn transfer(
        &self,
        program: &Program,
        index: usize,
        state: &mut State,
        mut out: Option<&mut Vec<Diagnostic>>,
    ) {
        let insn = &program.insns[index];
        let fx = &insn.fx;
        if !fx.known {
            // A macro or an instruction we do not model: assume the best.
            for id in fx.writes.iter() {
                state[id.index()] = Def::Defined;
            }
            return;
        }

        for read in reads(program, index, self.inputs) {
            let slot = &mut state[read.id.index()];
            match *slot {
                Def::Defined => {}
                Def::Input => {
                    if out.is_some() {
                        let mut found = self.found.borrow_mut();
                        for (function, _) in program
                            .functions
                            .iter()
                            .enumerate()
                            .filter(|(_, f)| f.blocks.binary_search(&insn.block).is_ok())
                        {
                            found[function].insert(read.id);
                        }
                    }
                }
                Def::Undefined(cause) => {
                    if let Some(out) = out.as_deref_mut() {
                        out.push(undefined(program, index, &read, cause));
                    }
                    *slot = Def::Defined; // report once
                }
                Def::Partial(written, cause, line) => {
                    if read.bits > written.bits() {
                        if let Some(out) = out.as_deref_mut() {
                            out.push(partial(program, index, &read, written, cause, line));
                        }
                        *slot = Def::Defined;
                    }
                }
            }
        }

        for id in fx.writes.iter() {
            let slot = &mut state[id.index()];
            if fx.full_writes.contains(id) {
                *slot = Def::Defined;
                continue;
            }
            let width = fx.dest.filter(|dest| dest.id == id).map_or(Width::Low8, |dest| dest.width);
            *slot = match *slot {
                Def::Undefined(cause) => Def::Partial(width, cause, insn.line),
                Def::Partial(old, cause, _) if width.bits() > old.bits() => {
                    Def::Partial(width, cause, insn.line)
                }
                other => other,
            };
        }

        match &fx.flow {
            Flow::Syscall => {
                state[RegId::RAX.index()] = Def::Defined;
                for id in abi::SYSCALL_CLOBBERS.iter() {
                    state[id.index()] = Def::Undefined(Cause::Syscall(insn.line));
                }
            }
            Flow::Int(_) => state[RegId::RAX.index()] = Def::Defined,
            Flow::Call(target) => match program.callee(target) {
                Callee::External => {
                    for id in abi::CALLER_SAVED.iter() {
                        state[id.index()] = Def::Undefined(Cause::Call(insn.line, None));
                    }
                    state[RegId::RAX.index()] = Def::Defined;
                }
                Callee::Local(callee) => {
                    let function = &program.functions[callee];
                    for id in function.clobbers.iter() {
                        state[id.index()] = Def::Defined; // possibly a result
                    }
                    for id in function.garbage().iter() {
                        state[id.index()] = Def::Undefined(Cause::Call(insn.line, Some(callee)));
                    }
                }
            },
            _ => {}
        }
    }
}

/// The registers the instruction at `index` needs a value in.
fn reads(program: &Program, index: usize, inputs: &[RegSet]) -> Vec<Read> {
    let insn = &program.insns[index];
    let fx = &insn.fx;
    let mnemonic = insn.ins.mnemonic.as_str();
    let mut reads = Vec::new();
    let mut explicit = RegSet::EMPTY;

    // `push reg` usually saves a register for its owner: not a real use.
    if mnemonic != "push" {
        for operand in &insn.ins.operands {
            let regs = match &operand.kind {
                OperandKind::Reg(reg) => std::slice::from_ref(reg),
                OperandKind::Mem(mem) => mem.regs.as_slice(),
                _ => &[],
            };
            for reg in regs.iter().filter(|reg| fx.reads.contains(reg.id)) {
                // `ah` needs bits 8-15: more than a write to `al` gives.
                let bits = if reg.width == Width::High8 { 16 } else { reg.width.bits() };
                reads.push(Read { id: reg.id, bits, span: operand.span, reason: Reason::Operand });
                explicit.insert(reg.id);
            }
        }
    }

    let mut implicit = fx.reads.difference(explicit);
    implicit.remove(RegId::RSP);
    if matches!(mnemonic, "div" | "idiv") {
        implicit.remove(RegId::RDX); // `div-without-rdx` has more to say
    }
    if mnemonic == "push" {
        implicit = RegSet::EMPTY;
    }
    for id in implicit.iter() {
        reads.push(Read { id, bits: 0, span: insn.span, reason: Reason::Implicit });
    }

    match &fx.flow {
        Flow::Syscall => {
            let number = program.constant_before(index, RegId::RAX);
            reads.push(Read {
                id: RegId::RAX,
                bits: 32,
                span: insn.span,
                reason: Reason::SyscallNumber,
            });
            if let Some(syscall) = number.and_then(syscalls::lookup) {
                for (position, id) in syscall.registers().iter().enumerate() {
                    reads.push(Read {
                        id: *id,
                        bits: 0,
                        span: insn.span,
                        reason: Reason::SyscallArgument(syscall, position),
                    });
                }
            }
        }
        Flow::Call(target) => {
            if let Callee::Local(callee) = program.callee(target) {
                let mut needed = inputs[callee];
                needed.remove(RegId::RSP);
                for id in needed.iter() {
                    reads.push(Read {
                        id,
                        bits: 0,
                        span: insn.span,
                        reason: Reason::Callee(callee),
                    });
                }
            }
        }
        _ => {}
    }
    reads
}

/// What the instruction does with the register, for the label under it.
fn describe(program: &Program, index: usize, read: &Read) -> String {
    let insn = &program.insns[index];
    match read.reason {
        Reason::Operand => "read here".to_string(),
        Reason::Implicit => format!("read by `{}`", insn.ins.mnemonic),
        Reason::SyscallNumber => "read as the system call number".to_string(),
        Reason::SyscallArgument(syscall, position) => {
            format!("`{}` reads it as `{}`", syscall.name, syscall.param(position))
        }
        Reason::Callee(callee) => format!("`{}` reads it", program.functions[callee].name),
    }
}

fn undefined(program: &Program, index: usize, read: &Read, cause: Cause) -> Diagnostic {
    let name = read.id.name64();
    let label = describe(program, index, read);
    let span_of = |line: usize| program.at_line(line).map(|at| program.insns[at].span);
    match cause {
        Cause::Entry => {
            let diagnostic = Diagnostic::new(
                &UNINITIALIZED_REGISTER,
                read.span,
                format!("`{name}` is used uninitialized"),
            )
            .label(label);
            match read.reason {
                Reason::SyscallArgument(syscall, _) => {
                    diagnostic.help(format!("set `{name}` first: {}", syscall.signature()))
                }
                _ => diagnostic
                    .help(format!("give it a value first, e.g. `xor {0}, {0}`", read.id.name32())),
            }
        }
        Cause::Syscall(line) => {
            let mut diagnostic = Diagnostic::new(
                &CLOBBERED_REGISTER,
                read.span,
                format!("`{name}` was destroyed by `syscall`"),
            )
            .label(label)
            .help("keep the value in a register `syscall` preserves, such as `r12`");
            if let Some(span) = span_of(line) {
                diagnostic = diagnostic.note_at(span, "`syscall` overwrites `rcx` and `r11`");
            }
            diagnostic
        }
        Cause::Call(line, callee) => {
            let (who, why) = match callee {
                None => {
                    ("a call".to_string(), format!("may overwrite `{name}`: it is caller-saved"))
                }
                Some(callee) => {
                    let callee = &program.functions[callee];
                    let why = if callee.makes_syscall && abi::SYSCALL_CLOBBERS.contains(read.id) {
                        "runs `syscall`, which overwrites `rcx` and `r11`"
                    } else {
                        "calls external code, which may overwrite it"
                    };
                    (format!("`{}`", callee.name), format!("`{}` {why}", callee.name))
                }
            };
            let mut diagnostic = Diagnostic::new(
                &CLOBBERED_REGISTER,
                read.span,
                format!("`{name}` was destroyed by {who}"),
            )
            .label(label)
            .help("keep the value in `rbx` or `r12`-`r15`, or save it on the stack");
            if let Some(span) = span_of(line) {
                diagnostic = diagnostic.note_at(span, why);
            }
            diagnostic
        }
    }
}

fn partial(
    program: &Program,
    index: usize,
    read: &Read,
    written: Width,
    cause: Cause,
    line: usize,
) -> Diagnostic {
    let low = read.id.name(written).unwrap_or("the low bits");
    let rest = match cause {
        Cause::Entry => "were never set".to_string(),
        Cause::Syscall(at) => format!("are left over from the `syscall` on line {}", at + 1),
        Cause::Call(at, _) => format!("are left over from the call on line {}", at + 1),
    };
    let mut diagnostic = Diagnostic::new(
        &PARTIAL_REGISTER_READ,
        read.span,
        format!("only `{low}` of `{}` holds a value", read.id.name64()),
    )
    .label(format!("{}: the other bits {rest}", describe(program, index, read)))
    .help(format!("load with `movzx`, or clear `{}` first", read.id.name32()));

    // The write that set the low bits: widen it when it is a plain load.
    let Some(writer) = program.at_line(line).map(|at| &program.insns[at]) else {
        return diagnostic;
    };
    diagnostic = diagnostic.note_at(writer.span, format!("only `{low}` is written here"));
    if let ("mov", [dest, source]) = (writer.ins.mnemonic.as_str(), writer.ins.operands.as_slice())
        && dest.as_reg().is_some_and(|reg| reg.id == read.id)
    {
        let wide = read.id.name32();
        let (start, end) = (source.span.start, source.span.end);
        let text = program.snippet(crate::span::Span::new(writer.line, start, end)).to_string();
        let size = if written.bits() == 16 { "word" } else { "byte" };
        let replacement = if source.constant().is_some() {
            format!("mov {wide}, {text}")
        } else if source.is_mem() && source.size.is_none() {
            format!("movzx {wide}, {size} {text}")
        } else {
            format!("movzx {wide}, {text}")
        };
        diagnostic = diagnostic
            .replace(format!("load the whole register: `{replacement}`"), writer.span, replacement)
            .maybe_incorrect();
    }
    diagnostic
}
