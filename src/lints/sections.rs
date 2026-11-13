//! Sections and declarations: code or data in the wrong place.

use super::Context;
use crate::ast::StmtKind;
use crate::diagnostic::{Diagnostic, Group, Lint};
use crate::insn::{Flow, Target};
use crate::program::SymbolKind;

pub static CODE_IN_DATA_SECTION: Lint = Lint {
    name: "code-in-data-section",
    group: Group::Suspicious,
    summary: "instructions placed in `.data` or `.bss`, where they cannot run",
};

pub static DATA_IN_BSS: Lint = Lint {
    name: "data-in-bss",
    group: Group::Suspicious,
    summary: "initialised data in `.bss`, which only reserves space",
};

pub static UNKNOWN_SECTION: Lint = Lint {
    name: "unknown-section",
    group: Group::Suspicious,
    summary: "a section name that looks like a misspelt standard one, such as `text` or `.date`",
};

pub static MISPLACED_LENGTH: Lint = Lint {
    name: "misplaced-length",
    group: Group::Suspicious,
    summary: "`len equ $ - msg` placed after other data, so it measures too much",
};

pub static JUMP_TO_DATA: Lint = Lint {
    name: "jump-to-data",
    group: Group::Correctness,
    summary: "a `jmp` or `call` whose target is a data label",
};

pub static UNUSED_EXTERN: Lint = Lint {
    name: "unused-extern",
    group: Group::Style,
    summary: "an `extern` declaration nothing uses",
};

const STANDARD: [&str; 4] = [".text", ".data", ".bss", ".rodata"];

pub fn check(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;

    // One report per section, on its first instruction.
    let mut reported = None;
    for (span, section) in &program.stray_code {
        if reported.replace(*section) == Some(*section) {
            continue;
        }
        let name = program.sections.get(*section).map_or("a data section", |(name, _)| name);
        out.push(
            Diagnostic::new(
                &CODE_IN_DATA_SECTION,
                *span,
                format!("this instruction is in `{name}`, which is not executable"),
            )
            .help("put `section .text` before the code"),
        );
    }

    for span in &program.bss_data {
        out.push(
            Diagnostic::new(&DATA_IN_BSS, *span, "`.bss` cannot hold initialised data")
                .note("the section only reserves space: the assembler ignores these values")
                .help("reserve space with `resb`, `resq`, ... or move the definition to `.data`"),
        );
    }

    for (name, span) in &program.sections {
        if STANDARD.contains(&name.as_str()) || name.starts_with(".text.") {
            continue;
        }
        let dotted = format!(".{}", name.trim_start_matches('.'));
        let intended = STANDARD
            .into_iter()
            .find(|standard| *standard == dotted || distance_at_most_one(standard, name));
        if let Some(intended) = intended {
            out.push(
                Diagnostic::new(
                    &UNKNOWN_SECTION,
                    *span,
                    format!("`{name}` is not a standard section: did you mean `{intended}`?"),
                )
                .note(
                    "an unknown name creates a new section with default attributes, \
                     which is neither executable nor writable",
                ),
            );
        }
    }

    for line in program.lines {
        let Some(StmtKind::Equ { symbols, here: true }) = line.stmt.as_ref().map(|s| &s.kind)
        else {
            continue;
        };
        let [name] = symbols.as_slice() else { continue };
        let Some(symbol) = program.symbols.get(name) else { continue };
        if symbol.kind != SymbolKind::Data || symbol.span.line >= line.number {
            continue;
        }
        // Another labelled definition between the data and its length?
        let between = program.lines[symbol.span.line + 1..line.number].iter().find(|other| {
            other.label.is_some()
                && matches!(other.stmt.as_ref().map(|s| &s.kind), Some(StmtKind::Data { .. }))
        });
        if let (Some(other), Some(stmt)) = (between, &line.stmt) {
            let other = cx.text(other.label.as_ref().map(|label| label.span).unwrap_or_default());
            out.push(
                Diagnostic::new(
                    &MISPLACED_LENGTH,
                    stmt.span,
                    format!("`$ - {name}` also counts `{other}`, defined after `{name}`"),
                )
                .note(
                    "`$` is the current position: everything between it and the label is measured",
                )
                .help(format!("move this line right after the definition of `{name}`")),
            );
        }
    }

    for insn in &program.insns {
        let (Flow::Jump(Target::Label(name))
        | Flow::CondJump(Target::Label(name))
        | Flow::Call(Target::Label(name))) = &insn.fx.flow
        else {
            continue;
        };
        if program.symbols.get(name).is_some_and(|symbol| symbol.kind == SymbolKind::Data) {
            let target = insn.ins.operands.first().map_or(insn.span, |operand| operand.span);
            out.push(
                Diagnostic::new(
                    &JUMP_TO_DATA,
                    target,
                    format!("`{}` to `{name}`, which is data, not code", insn.ins.mnemonic),
                )
                .note("the processor would execute the bytes of the data as instructions"),
            );
        }
    }

    if !program.opaque {
        for (name, span) in &program.externs {
            if !program.references.contains_key(name) {
                out.push(Diagnostic::new(
                    &UNUSED_EXTERN,
                    *span,
                    format!("`{name}` is declared `extern` but never used"),
                ));
            }
        }
    }
}

/// Whether `b` is `a` with one character changed, added or removed.
fn distance_at_most_one(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a == b || a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a.iter().rev().zip(b.iter().rev()).take_while(|(x, y)| x == y).count();
    let longest = a.len().max(b.len());
    // Two neighbours swapped (`.bbs`) count as one mistake.
    let swapped = a.len() == b.len()
        && prefix + suffix + 2 == longest
        && a[prefix] == b[prefix + 1]
        && a[prefix + 1] == b[prefix];
    prefix + suffix + 1 >= longest || swapped
}

#[cfg(test)]
mod tests {
    use super::distance_at_most_one;

    #[test]
    fn near_misses() {
        assert!(distance_at_most_one(".text", ".txt"));
        assert!(distance_at_most_one(".data", ".date"));
        assert!(distance_at_most_one(".bss", ".bbs"));
        assert!(distance_at_most_one(".bss", ".bs"));
        assert!(!distance_at_most_one(".text", ".text"));
        assert!(!distance_at_most_one(".text", ".mycode"));
    }
}
