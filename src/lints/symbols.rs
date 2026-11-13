//! Labels and symbols: undefined, duplicated, unused, not exported.

use super::Context;
use crate::abi;
use crate::diagnostic::{Diagnostic, Group, Lint};
use crate::program::SymbolKind;

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

    for (_, span, first) in &program.duplicates {
        let message =
            format!("label `{}` is already defined on line {}", cx.text(*span), first + 1);
        out.push(Diagnostic::new(&DUPLICATE_LABEL, *span, message));
    }

    // Past this point the checks need the complete symbol table, which a
    // macro or an include can extend behind our back.
    if program.opaque {
        return;
    }
    let defined = |name: &str| {
        program.symbols.contains_key(name)
            || program.externs.contains(name)
            || name.starts_with("__")
            || name.starts_with("..")
            || name.starts_with('$')
    };

    if let Some(start) = program.symbols.get("_start")
        && start.kind == SymbolKind::Code
        && !program.globals.contains_key("_start")
    {
        out.push(
            Diagnostic::new(&ENTRY_NOT_GLOBAL, start.span, "`_start` is not declared `global`")
                .note("the linker only sees exported symbols and will not find the entry point")
                .help("add `global _start` at the top of the file"),
        );
    }

    for (name, span) in &program.globals {
        if !defined(name) {
            let message = format!("`{name}` is declared `global` but never defined");
            out.push(Diagnostic::new(&UNDEFINED_SYMBOL, *span, message));
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
            diagnostic = match closest(name, program.symbols.keys().chain(&program.externs)) {
                Some(similar) => diagnostic.help(format!("did you mean `{similar}`?")),
                None => diagnostic.help(format!(
                    "define the label, or declare `extern {name}` if it lives in another file"
                )),
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
            SymbolKind::Code => format!("label `{text}` is never jumped to or called"),
            _ => format!("`{text}` is never used"),
        };
        out.push(Diagnostic::new(&UNUSED_LABEL, symbol.span, message));
    }
}

/// The candidate closest to `name`, if it is close enough to be a typo.
fn closest<'a>(name: &str, candidates: impl Iterator<Item = &'a String>) -> Option<&'a str> {
    let budget = (name.chars().count() / 3).max(1);
    candidates
        .map(|candidate| (edit_distance(name, candidate), candidate.as_str()))
        .filter(|(distance, _)| *distance <= budget)
        .min()
        .map(|(_, candidate)| candidate)
}

/// Levenshtein distance.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substitution = diagonal + usize::from(ca != *cb);
            diagonal = row[j + 1];
            row[j + 1] = substitution.min(row[j] + 1).min(diagonal + 1);
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances() {
        assert_eq!(edit_distance("buffer", "buffer"), 0);
        assert_eq!(edit_distance("bufer", "buffer"), 1);
        assert_eq!(edit_distance("mesage", "message"), 1);
        assert_eq!(edit_distance("", "abc"), 3);
    }

    #[test]
    fn suggests_only_near_misses() {
        let names = ["buffer".to_string(), "message".to_string()];
        assert_eq!(closest("bufer", names.iter()), Some("buffer"));
        assert_eq!(closest("printf", names.iter()), None);
    }
}
