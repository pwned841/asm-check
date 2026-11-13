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

/// Whether the source is written for the GNU assembler rather than NASM:
/// AT&T operands (`%rax`, `$1`) or dot-directives (`.globl`, `.text`).
///
/// Editors hand every `.s` file to the same language server, so this is how
/// the tool knows to stay quiet about a dialect it does not understand.
pub fn is_gnu_syntax(source: &str) -> bool {
    const DIRECTIVES: &[&str] = &[
        ".globl",
        ".global",
        ".text",
        ".data",
        ".bss",
        ".section",
        ".intel_syntax",
        ".att_syntax",
        ".file",
        ".type",
        ".size",
        ".align",
        ".p2align",
        ".balign",
        ".string",
        ".asciz",
        ".ascii",
        ".quad",
        ".long",
        ".byte",
        ".code64",
        ".macro",
        ".include",
        ".equ",
        ".set",
        ".cfi_startproc",
    ];
    source.lines().any(|line| {
        let line = line.trim_start();
        let directive = line.split_whitespace().next().unwrap_or("");
        DIRECTIVES.contains(&directive)
            || line.match_indices('%').any(|(at, _)| {
                let name: String =
                    line[at + 1..].chars().take_while(char::is_ascii_alphanumeric).collect();
                at > 0 && reg::Reg::parse(&name).is_some()
            })
    })
}

/// Checks a source file and returns its diagnostics in source order.
///
/// Sources this tool does not understand (GNU assembler syntax, 16- and
/// 32-bit code) produce no diagnostics rather than wrong ones.
pub fn check(source: &str, config: &Config) -> Vec<Diagnostic> {
    if is_gnu_syntax(source) {
        return Vec::new();
    }
    let lines = parser::parse(source);
    let program = program::Program::build(&lines);
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
