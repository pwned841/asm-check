//! Misspelt instructions.

use super::Context;
use crate::diagnostic::{Diagnostic, Group, Lint};
use crate::mnemonic::{all_known, is_instruction};
use crate::span::Span;
use crate::text;

pub static UNKNOWN_INSTRUCTION: Lint = Lint {
    name: "unknown-instruction",
    group: Group::Correctness,
    summary: "a word that is not an instruction but is one typo away from one, such as `mvo`",
};

pub fn check(cx: &Context, out: &mut Vec<Diagnostic>) {
    let program = cx.program;
    // A macro can be named anything.
    if program.opaque {
        return;
    }
    let known = all_known();
    let mut report = |word: &str, span: Span| {
        let word = word.to_ascii_lowercase();
        if word.len() < 3 || is_instruction(&word) {
            return;
        }
        let budget = if word.len() >= 6 { 2 } else { 1 };
        // Only speak up when there is something to suggest: the list of
        // instructions this tool knows is not the whole instruction set.
        if let Some(similar) = text::closest(&word, known.iter().map(String::as_str), budget) {
            out.push(
                Diagnostic::new(
                    &UNKNOWN_INSTRUCTION,
                    span,
                    format!("`{word}` is not an instruction"),
                )
                .help(format!("did you mean `{similar}`?"))
                .replace(format!("replace with `{similar}`"), span, similar),
            );
        }
    };

    for insn in program.insns.iter().filter(|insn| !insn.fx.known) {
        report(&insn.ins.mnemonic, insn.ins.mnemonic_span);
    }
    // A lone word without a colon is parsed as a label: `sycall`.
    for line in program.lines {
        if let Some(label) = line.label.as_ref().filter(|label| !label.colon)
            && line.stmt.is_none()
            && !program.references.contains_key(&label.name)
        {
            report(cx.text(label.span), label.span);
        }
    }
}
