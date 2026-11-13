//! `asm-check` is a semantic linter for x86-64 assembly written in NASM
//! (Intel) syntax.

pub mod ast;
pub mod mnemonic;
pub mod parser;
pub mod reg;
pub mod span;
