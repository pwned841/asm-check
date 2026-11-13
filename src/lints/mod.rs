//! The lints. Each submodule declares its [`Lint`]s and a `check` pass.

pub mod operands;
pub mod symbols;

use crate::diagnostic::{Diagnostic, Lint};
use crate::program::{Insn, Program};
use crate::span::Span;

/// What a lint pass can look at.
pub struct Context<'a> {
    pub program: &'a Program<'a>,
    /// The source, split into lines.
    pub source: Vec<&'a str>,
}

impl Context<'_> {
    /// The source text a span covers.
    pub fn text(&self, span: Span) -> &str {
        self.source.get(span.line).and_then(|line| line.get(span.start..span.end)).unwrap_or("")
    }

    /// The source text of an instruction, e.g. `mov rax, 1`.
    pub fn code(&self, insn: &Insn) -> &str {
        self.text(insn.span)
    }

    /// Whether the instruction mentions a name introduced by `%define`,
    /// in which case its operands are not what they look like.
    pub fn uses_define(&self, insn: &Insn) -> bool {
        insn.ins
            .operands
            .iter()
            .flat_map(|operand| operand.symbols())
            .any(|symbol| self.program.defines.contains(symbol))
    }
}

type Pass = fn(&Context, &mut Vec<Diagnostic>);

/// Every lint, in the order `asm-check lints` prints them.
pub static ALL: &[&Lint] = &[
    &operands::OPERAND_SIZE_MISMATCH,
    &operands::AMBIGUOUS_OPERAND_SIZE,
    &operands::INVALID_OPERANDS,
    &symbols::UNDEFINED_SYMBOL,
    &symbols::DUPLICATE_LABEL,
    &symbols::ENTRY_NOT_GLOBAL,
    &symbols::UNUSED_LABEL,
];

const PASSES: &[Pass] = &[operands::check, symbols::check];

pub fn run(context: &Context) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for pass in PASSES {
        pass(context, &mut diagnostics);
    }
    diagnostics
}

/// Looks a lint up by name.
pub fn find(name: &str) -> Option<&'static Lint> {
    ALL.iter().copied().find(|lint| lint.name == name)
}
