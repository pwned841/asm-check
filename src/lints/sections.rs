//! Sections and declarations: code or data in the wrong place.

use super::Context;
use crate::ast::StmtKind;
use crate::diagnostic::{Diagnostic, Group, Lint};
use crate::insn::{Flow, Target};
use crate::program::SymbolKind;
use crate::text;

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
            Diagnostic::new(&CODE_IN_DATA_SECTION, *span, format!("code in `{name}`"))
                .label("this section is not executable")
                .insert_before("start a code section", span.line, "section .text")
                .maybe_incorrect(),
        );
    }

    for span in &program.bss_data {
        out.push(
            Diagnostic::new(&DATA_IN_BSS, *span, "initialised data in `.bss`")
                .label("the value is ignored: `.bss` only reserves space")
                .help("use `resb`/`resq` here, or move the definition to `.data`"),
        );
    }

    for (name, span) in &program.sections {
        if STANDARD.contains(&name.as_str()) || name.starts_with(".text.") {
            continue;
        }
        let dotted = format!(".{}", name.trim_start_matches('.'));
        let intended = STANDARD
            .into_iter()
            .find(|standard| *standard == dotted || text::typo_distance(standard, name) == 1);
        if let Some(intended) = intended {
            let fixed = cx.text(*span).replacen(name.as_str(), intended, 1);
            out.push(
                Diagnostic::new(&UNKNOWN_SECTION, *span, format!("unknown section `{name}`"))
                    .label("a new section, neither executable nor writable")
                    .replace(format!("use `{intended}`"), *span, fixed),
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
                    format!("`$ - {name}` also counts `{other}`"),
                )
                .label("measures everything since the label")
                .note_at(symbol.span, format!("`{name}` starts here"))
                .help(format!("move this line right after `{name}`")),
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
            let mut diagnostic = Diagnostic::new(
                &JUMP_TO_DATA,
                target,
                format!("`{}` to data `{name}`", insn.ins.mnemonic),
            )
            .label("would execute these bytes as instructions");
            if let Some(symbol) = program.symbols.get(name) {
                diagnostic = diagnostic.note_at(symbol.span, "defined as data here");
            }
            out.push(diagnostic);
        }
    }

    if !program.opaque {
        for (name, span) in &program.externs {
            if program.references.contains_key(name) {
                continue;
            }
            let mut diagnostic =
                Diagnostic::new(&UNUSED_EXTERN, *span, format!("unused `extern {name}`"));
            // Safe to drop the line when it declares nothing else.
            if !cx.text(*span).contains(',') && program.lines[span.line].label.is_none() {
                diagnostic = diagnostic.delete_line("remove the declaration", span.line);
            }
            out.push(diagnostic);
        }
    }
}
