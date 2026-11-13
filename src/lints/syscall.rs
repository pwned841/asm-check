//! System calls: numbers that do not exist, arguments that are not what
//! the kernel expects.

use super::Context;
use crate::ast::OperandKind;
use crate::diagnostic::{Diagnostic, Group, Lint};
use crate::insn::Flow;
use crate::program::SymbolKind;
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

pub fn check(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;
    for (index, insn) in program.insns.iter().enumerate() {
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
            let param = syscall.param(position);
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
