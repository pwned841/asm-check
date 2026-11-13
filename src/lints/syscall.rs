//! System calls: numbers that do not exist, arguments that are not what
//! the kernel expects.

use super::Context;
use crate::ast::OperandKind;
use crate::diagnostic::{Diagnostic, Group, Lint};
use crate::insn::{Flow, Target};
use crate::program::{SymbolKind, Value};
use crate::reg::RegId;
use crate::syscalls;

pub static UNKNOWN_SYSCALL: Lint = Lint {
    name: "unknown-syscall",
    group: Group::Suspicious,
    summary: "`syscall` with a number that is not a Linux x86-64 system call",
};

pub static EXIT_CODE_TRUNCATED: Lint = Lint {
    name: "exit-code-truncated",
    group: Group::Suspicious,
    summary: "an exit status outside 0-255, of which the shell only sees the low 8 bits",
};

pub static POINTER_FROM_CONTENTS: Lint = Lint {
    name: "pointer-from-contents",
    group: Group::Suspicious,
    summary: "a pointer argument loaded with `mov reg, [buffer]`, the buffer's contents, \
              instead of its address",
};

pub static BUFFER_OVERFLOW: Lint = Lint {
    name: "buffer-overflow",
    group: Group::Suspicious,
    summary: "a system call given a length larger than the buffer it points to",
};

pub static MISSING_NUL_TERMINATOR: Lint = Lint {
    name: "missing-nul-terminator",
    group: Group::Suspicious,
    summary: "a string without a final zero byte passed where a C string is expected",
};

/// Library functions whose first argument is a C string.
const STRING_FUNCTIONS: &[&str] = &[
    "puts", "printf", "strlen", "fopen", "atoi", "atol", "system", "perror", "getenv", "strdup",
    "unlink", "chdir", "remove",
];

/// Parameters that are a byte count for the buffer just before them.
const LENGTHS: &[&str] = &["count", "len", "buflen", "bufsiz", "size"];

/// Reports `name` when it is a byte string without a terminating zero.
fn check_terminated(
    cx: &Context,
    name: &str,
    at: crate::span::Span,
    reader: &str,
    out: &mut Vec<Diagnostic>,
) {
    let Some(symbol) = cx.program.symbols.get(name) else { return };
    let Some(data) = symbol.data.as_ref().filter(|data| data.directive == "db") else { return };
    if data.nul_terminated || data.size.is_none() {
        return;
    }
    out.push(
        Diagnostic::new(&MISSING_NUL_TERMINATOR, at, format!("`{name}` is not NUL-terminated"))
            .label(format!("`{reader}` reads until the first zero byte"))
            .note_at(symbol.span, "the string ends without a 0")
            .replace("terminate the string", data.end, ", 0")
            .maybe_incorrect(),
    );
}

pub fn check(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;
    for (index, insn) in program.insns.iter().enumerate() {
        if let Flow::Call(Target::Label(function)) = &insn.fx.flow
            && STRING_FUNCTIONS.contains(&function.as_str())
            && program.externs.contains_key(function)
            && let Some(Value::Address(name)) = program.value_before(index, RegId::RDI)
        {
            check_terminated(cx, &name, insn.span, function, out);
        }
        if insn.fx.flow != Flow::Syscall {
            continue;
        }
        let Some(number) = program.constant_before(index, RegId::RAX) else { continue };
        if syscalls::is_impossible(number) {
            out.push(
                Diagnostic::new(
                    &UNKNOWN_SYSCALL,
                    insn.span,
                    format!("no system call {number} on x86-64 Linux"),
                )
                .note("`syscall` uses the 64-bit numbers: 0 `read`, 1 `write`, 60 `exit`"),
            );
            continue;
        }
        let Some(syscall) = syscalls::lookup(number) else { continue };

        if matches!(syscall.name, "exit" | "exit_group")
            && let Some(status) = program.constant_before(index, RegId::RDI)
            && !(0..=255).contains(&status)
        {
            out.push(
                Diagnostic::new(
                    &EXIT_CODE_TRUNCATED,
                    insn.span,
                    format!("exit status {status} becomes {}", status & 0xFF),
                )
                .label("only the low 8 bits reach the parent process"),
            );
        }

        for (position, reg) in syscall.registers().iter().enumerate() {
            if !syscall.is_pointer(position) {
                continue;
            }
            let param = syscall.param(position);
            if let Some(Value::Address(name)) = program.value_before(index, *reg) {
                if param.contains("path") || matches!(param, "name" | "target") {
                    check_terminated(cx, &name, insn.span, syscall.name, out);
                }
                // A buffer followed by its length: compare with the real size.
                let length = syscall.registers().get(position + 1).filter(|_| {
                    param.contains("buf") && LENGTHS.contains(&syscall.param(position + 1))
                });
                if let Some(length) = length
                    && let Some(Value::Constant(count)) = program.value_before(index, *length)
                    && let Some(symbol) = program.symbols.get(&name)
                    && let Some(size) = symbol.data.as_ref().and_then(|data| data.size)
                    && u64::try_from(count).is_ok_and(|count| count > size)
                {
                    let sends = matches!(syscall.name, "write" | "pwrite64" | "sendto");
                    let message = if sends {
                        format!(
                            "`{}` sends {count} bytes from `{name}`, which holds {size}",
                            syscall.name
                        )
                    } else {
                        format!(
                            "`{}` may store {count} bytes in `{name}`, which holds {size}",
                            syscall.name
                        )
                    };
                    out.push(
                        Diagnostic::new(&BUFFER_OVERFLOW, insn.span, message)
                            .label(if sends { "reads past the end" } else { "writes past the end" })
                            .note_at(symbol.span, format!("`{name}` is {size} bytes")),
                    );
                }
            }
            let Some(load) = program.last_write_before(index, *reg).map(|i| &program.insns[i])
            else {
                continue;
            };
            let Some(source) = load.ins.operands.get(1) else { continue };
            let OperandKind::Mem(mem) = &source.kind else { continue };
            if load.ins.mnemonic != "mov" || !mem.direct {
                continue;
            }
            // Bytes are text or a buffer; a `dq` may well hold a pointer.
            let name = &mem.symbols[0];
            let bytes = program.symbols.get(name).is_some_and(|symbol| {
                symbol.kind == SymbolKind::Data
                    && symbol
                        .data
                        .as_ref()
                        .is_some_and(|data| matches!(data.directive.as_str(), "db" | "resb"))
            });
            if !bytes {
                continue;
            }
            let mut diagnostic = Diagnostic::new(
                &POINTER_FROM_CONTENTS,
                source.span,
                format!("`{reg}` gets the contents of `{name}`, not its address"),
            )
            .label(format!("`{}` expects a pointer in `{param}`", syscall.name))
            .help(format!("use `mov {reg}, {name}` or `lea {reg}, [rel {name}]`"));
            if !name.contains('.') {
                diagnostic = diagnostic
                    .replace(format!("pass the address of `{name}`"), source.span, name)
                    .maybe_incorrect();
            }
            out.push(diagnostic);
        }
    }
}
