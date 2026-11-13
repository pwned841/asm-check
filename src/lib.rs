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
pub mod insn;
pub mod lints;
pub mod mnemonic;
pub mod parser;
pub mod program;
pub mod reg;
pub mod render;
pub mod span;
pub mod suppress;

pub use diagnostic::{Diagnostic, Group, Lint, Severity};

/// Which lints run.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Also run the `pedantic` group.
    pub pedantic: bool,
    /// Lints to silence, by name.
    pub allow: HashSet<String>,
}

/// Checks a source file and returns its diagnostics in source order.
pub fn check(source: &str, config: &Config) -> Vec<Diagnostic> {
    let lines = parser::parse(source);
    let program = program::Program::build(&lines);
    if program.bits != 64 {
        // Every rule here assumes the 64-bit register set and ABI.
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
