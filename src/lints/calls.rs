//! Function calls and returns: what `rax` holds going in and coming out.

use super::Context;
use crate::diagnostic::{Diagnostic, Group, Lint};
use crate::insn::{Flow, Target};
use crate::program::{Callee, Program};
use crate::reg::RegId;

pub static VARIADIC_CALL_WITHOUT_AL: Lint = Lint {
    name: "variadic-call-without-al",
    group: Group::Suspicious,
    summary: "`printf` and other variadic functions called without setting `al`",
};

pub static MAIN_RETURN_VALUE: Lint = Lint {
    name: "main-return-value",
    group: Group::Suspicious,
    summary: "`main` returns without setting `eax`, so the exit status is an accident",
};

const VARIADIC: &[&str] = &[
    "printf", "fprintf", "sprintf", "snprintf", "dprintf", "asprintf", "scanf", "fscanf", "sscanf",
    "syslog", "execl", "execlp", "execle",
];

/// Where the value in `rax` comes from, looking back from an instruction.
enum RaxSource {
    /// An instruction wrote it.
    Set,
    /// It is the result of the call or system call at this index.
    Result(usize),
    /// Nothing wrote it since the function was entered.
    Entry,
    Unknown,
}

fn rax_source(program: &Program, index: usize) -> RaxSource {
    let mut i = index;
    loop {
        if i == 0 || !program.insns[i].labels.is_empty() {
            let block = &program.blocks[program.insns[i].block];
            let entry = block.function.is_some() && block.start == i;
            return if entry { RaxSource::Entry } else { RaxSource::Unknown };
        }
        let previous = &program.insns[i - 1];
        if previous.run != program.insns[i].run || !previous.fx.known {
            return RaxSource::Unknown;
        }
        match previous.fx.flow {
            Flow::Call(_) | Flow::Syscall | Flow::Int(_) => return RaxSource::Result(i - 1),
            Flow::Next => {}
            _ => return RaxSource::Unknown,
        }
        if previous.fx.writes.contains(RegId::RAX) {
            return RaxSource::Set;
        }
        i -= 1;
    }
}

pub fn check(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;

    for (index, insn) in program.insns.iter().enumerate() {
        let Flow::Call(target @ Target::Label(name)) = &insn.fx.flow else { continue };
        if !VARIADIC.contains(&name.as_str()) || program.callee(target) != Callee::External {
            continue;
        }
        let (label, origin) = match rax_source(program, index) {
            RaxSource::Result(previous) => {
                ("`al` still holds an earlier result", Some(program.insns[previous].span))
            }
            RaxSource::Entry => ("`al` was never set in this function", None),
            RaxSource::Set | RaxSource::Unknown => continue,
        };
        let mut diagnostic = Diagnostic::new(
            &VARIADIC_CALL_WITHOUT_AL,
            insn.span,
            format!("`{name}` called without setting `al`"),
        )
        .label(label)
        .note("a variadic function reads `al` as the number of vector registers used");
        if let Some(span) = origin {
            diagnostic = diagnostic.note_at(span, "`rax` was last written here");
        }
        out.push(
            diagnostic
                .insert_before("no vector arguments: clear `eax`", insn.line, "xor eax, eax")
                .maybe_incorrect(),
        );
    }

    for function in program.functions.iter().filter(|function| function.name == "main") {
        for &block in &function.blocks {
            let block = &program.blocks[block];
            for index in block.start..block.end {
                let insn = &program.insns[index];
                if insn.fx.flow != Flow::Ret {
                    continue;
                }
                let (message, origin) = match rax_source(program, index) {
                    RaxSource::Result(previous) => {
                        let previous = &program.insns[previous];
                        (
                            format!("`main` returns the result of `{}`", cx.code(previous)),
                            Some(previous.span),
                        )
                    }
                    RaxSource::Entry => ("`main` returns without setting `eax`".to_string(), None),
                    RaxSource::Set | RaxSource::Unknown => continue,
                };
                let mut diagnostic = Diagnostic::new(&MAIN_RETURN_VALUE, insn.span, message)
                    .label("`eax` becomes the exit status");
                if let Some(span) = origin {
                    diagnostic = diagnostic.note_at(span, "`eax` was last set here");
                }
                out.push(
                    diagnostic
                        .insert_before("return 0", insn.line, "xor eax, eax")
                        .maybe_incorrect(),
                );
            }
        }
    }
}
