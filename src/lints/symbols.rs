//! Labels and symbols: undefined, duplicated, unused, not exported.

use super::Context;
use crate::abi;
use crate::diagnostic::{Diagnostic, Group, Lint};
use crate::program::SymbolKind;
use crate::{reg, text};

pub static UNDEFINED_SYMBOL: Lint = Lint {
    name: "undefined-symbol",
    group: Group::Correctness,
    summary: "a name that is neither a label of this file nor declared `extern`",
};

pub static DUPLICATE_LABEL: Lint = Lint {
    name: "duplicate-label",
    group: Group::Correctness,
    summary: "a label defined more than once",
};

pub static ENTRY_NOT_GLOBAL: Lint = Lint {
    name: "entry-not-global",
    group: Group::Suspicious,
    summary: "`_start` is defined but not exported with `global`",
};

pub static UNUSED_LABEL: Lint = Lint {
    name: "unused-label",
    group: Group::Style,
    summary: "a label or data item nothing refers to",
};

pub fn check(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;

    for (name, span, _) in &program.duplicates {
        let mut diagnostic = Diagnostic::new(
            &DUPLICATE_LABEL,
            *span,
            format!("`{}` is defined twice", cx.text(*span)),
        )
        .label("second definition");
        if let Some(first) = program.symbols.get(name) {
            diagnostic = diagnostic.note_at(first.span, "first defined here");
        }
        out.push(diagnostic);
    }

    // Past this point the checks need the complete symbol table, which a
    // macro or an include can extend behind our back.
    if program.opaque {
        return;
    }
    let defined = |name: &str| {
        program.symbols.contains_key(name)
            || program.externs.contains_key(name)
            || name.starts_with("__")
            || name.starts_with("..")
            || name.starts_with('$')
    };

    if let Some(start) = program.symbols.get("_start")
        && start.kind == SymbolKind::Code
        && !program.globals.contains_key("_start")
    {
        out.push(
            Diagnostic::new(&ENTRY_NOT_GLOBAL, start.span, "`_start` is not exported")
                .label("the linker will not find the entry point")
                .insert_before("declare it `global`", start.span.line, "global _start"),
        );
    }

    for (name, span) in &program.globals {
        if !defined(name) {
            out.push(
                Diagnostic::new(
                    &UNDEFINED_SYMBOL,
                    *span,
                    format!("`global {name}` is never defined"),
                )
                .label("no such label in this file"),
            );
        }
    }

    for line in program.lines {
        let Some(crate::ast::StmtKind::Instruction(ins)) = line.stmt.as_ref().map(|s| &s.kind)
        else {
            continue;
        };
        for operand in &ins.operands {
            let Some(name) = operand.symbols().iter().find(|name| !defined(name)) else {
                continue;
            };
            let mut diagnostic = Diagnostic::new(
                &UNDEFINED_SYMBOL,
                operand.span,
                format!("undefined symbol `{name}`"),
            );
            let budget = (name.chars().count() / 3).max(1);
            let labels = program.symbols.keys().chain(program.externs.keys()).map(String::as_str);
            let suggestion = text::closest(name, reg::names(), 1)
                .or_else(|| text::closest(name, labels, budget));
            // What is written in the source: a local label keeps its dot.
            let operand_text = cx.text(operand.span);
            let written = name.rfind('.').map(|dot| &name[dot..]).filter(|short| {
                operand_text.contains(short) && !operand_text.contains(name.as_str())
            });
            let written = written.unwrap_or(name.as_str());
            diagnostic = match suggestion {
                Some(similar) => {
                    // Offer the short form of a local label of the same scope.
                    let short = match (name.rfind('.'), similar.rfind('.')) {
                        (Some(a), Some(b)) if name[..a] == similar[..b] => &similar[b..],
                        _ => similar,
                    };
                    let fixed = operand_text.replacen(written, short, 1);
                    diagnostic
                        .label("not defined in this file")
                        .replace(format!("did you mean `{short}`?"), operand.span, fixed)
                        .maybe_incorrect()
                }
                None => diagnostic
                    .label("not defined in this file")
                    .help(format!("define it, or declare `extern {name}`")),
            };
            out.push(diagnostic);
        }
    }

    for (name, symbol) in &program.symbols {
        let referenced = program.references.get(name).is_some_and(|count| *count > 0);
        if referenced
            || program.globals.contains_key(name)
            || abi::is_entry_point(name)
            || name == "main"
            || name.starts_with("..")
        {
            continue;
        }
        let text = cx.text(symbol.span);
        let message = match symbol.kind {
            SymbolKind::Code => format!("unused label `{text}`"),
            _ => format!("unused `{text}`"),
        };
        let mut diagnostic = Diagnostic::new(&UNUSED_LABEL, symbol.span, message);
        // A label alone on its line can go without touching any code.
        if program.lines[symbol.span.line].stmt.is_none() {
            diagnostic =
                diagnostic.delete_line("remove the label", symbol.span.line).maybe_incorrect();
        }
        out.push(diagnostic);
    }
}
