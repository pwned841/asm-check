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
        let what = match rax_source(program, index) {
            RaxSource::Result(previous) => format!(
                "holds the result of the `{}` on line {}",
                program.insns[previous].ins.mnemonic,
                program.insns[previous].line + 1
            ),
            RaxSource::Entry => "has not been set in this function".to_string(),
            RaxSource::Set | RaxSource::Unknown => continue,
        };
        out.push(
            Diagnostic::new(
                &VARIADIC_CALL_WITHOUT_AL,
                insn.span,
                format!("`{name}` is variadic and reads `al`, which {what}"),
            )
            .note("`al` must hold the number of vector registers used for the arguments")
            .help("put `xor eax, eax` right before the call when no floating-point argument is passed"),
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
                let message = match rax_source(program, index) {
                    RaxSource::Result(previous) => {
                        let previous = &program.insns[previous];
                        format!(
                            "`main` returns the result of `{}` (line {}) as the exit status",
                            cx.code(previous),
                            previous.line + 1
                        )
                    }
                    RaxSource::Entry => "`main` returns without setting `eax`".to_string(),
                    RaxSource::Set | RaxSource::Unknown => continue,
                };
                out.push(
                    Diagnostic::new(&MAIN_RETURN_VALUE, insn.span, message)
                        .note("the value `main` returns in `eax` becomes the exit status of the program")
                        .help("set it before `ret`: `xor eax, eax` for success"),
                );
            }
        }
    }
}
