//! `asm-check` is a semantic linter for x86-64 assembly written in NASM
//! (Intel) syntax.
//!
//! ```
//! let diagnostics = asm_check::check("mov rax, rax\n", &asm_check::Config::default());
//! assert!(diagnostics.iter().all(|d| d.span.line == 0));
//! ```

use std::collections::HashSet;

pub mod abi;
pub mod ast;
pub mod dataflow;
pub mod diagnostic;
pub mod dialect;
pub mod docs;
pub mod fix;
pub mod ide;
pub mod insn;
pub mod lints;
pub mod lsp;
pub mod mnemonic;
pub mod parser;
pub mod program;
pub mod reg;
pub mod render;
pub mod span;
pub mod suppress;
pub mod syscalls;
pub mod text;

pub use diagnostic::{Diagnostic, Group, Lint, Severity};

/// Which lints run.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Also run the `pedantic` group.
    pub pedantic: bool,
    /// Lints to silence, by name.
    pub allow: HashSet<String>,
}

/// Whether the source is written for the GNU assembler rather than NASM:
/// AT&T operands (`%rax`, `$1`) or dot-directives (`.globl`, `.text`).
pub fn is_gnu_syntax(source: &str) -> bool {
    dialect::detect(source) == dialect::Dialect::Gnu
}

/// Checks a source file and returns its diagnostics in source order.
///
/// Sources this tool does not understand (GNU, MASM or ARM assembly, 16-
/// and 32-bit code) produce no diagnostics rather than wrong ones.
pub fn check(source: &str, config: &Config) -> Vec<Diagnostic> {
    if dialect::detect(source) != dialect::Dialect::Nasm {
        return Vec::new();
    }
    let lines = parser::parse(source);
    let mut program = program::Program::build(&lines);
    program.text = source.lines().collect();
    if program.bits != 64 {
        return Vec::new();
    }
    let context = lints::Context { program: &program, source: source.lines().collect() };
    let suppressions = suppress::Suppressions::collect(&lines);

    let mut diagnostics = lints::run(&context);
    diagnostics.retain(|diagnostic| {
        let lint = diagnostic.lint;
        (config.pedantic || lint.group != Group::Pedantic)
            && !config.allow.contains(lint.name)
            && !suppressions.allows(lint.name, diagnostic.span.line)
    });
    diagnostics.sort_by_key(|d| (d.span.line, d.span.start, d.lint.name));
    diagnostics.dedup_by_key(|d| (d.span, d.lint.name));
    diagnostics
}
