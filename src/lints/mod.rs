//! The lints. Each submodule declares its [`Lint`]s and a `check` pass.

pub mod calls;
pub mod control;
pub mod definedness;
pub mod division;
pub mod flags;
pub mod memory;
pub mod operands;
pub mod registers;
pub mod sections;
pub mod stack;
pub mod style;
pub mod symbols;
pub mod syscall;
pub mod typos;

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
    &memory::INVALID_ADDRESS,
    &memory::IMMEDIATE_OUT_OF_RANGE,
    &sections::JUMP_TO_DATA,
    &typos::UNKNOWN_INSTRUCTION,
    &symbols::UNDEFINED_SYMBOL,
    &symbols::DUPLICATE_LABEL,
    &stack::UNBALANCED_STACK,
    &stack::MISALIGNED_CALL,
    &stack::MISMATCHED_POP,
    &stack::ODD_STACK_ADJUSTMENT,
    &calls::VARIADIC_CALL_WITHOUT_AL,
    &calls::MAIN_RETURN_VALUE,
    &control::RET_IN_START,
    &control::FALLTHROUGH,
    &control::UNREACHABLE_CODE,
    &control::INT80_IN_64BIT,
    &symbols::ENTRY_NOT_GLOBAL,
    &division::DIV_WITHOUT_RDX,
    &division::IDIV_ZERO_EXTENDED,
    &definedness::UNINITIALIZED_REGISTER,
    &definedness::CLOBBERED_REGISTER,
    &definedness::PARTIAL_REGISTER_READ,
    &syscall::UNKNOWN_SYSCALL,
    &syscall::EXIT_CODE_TRUNCATED,
    &syscall::POINTER_FROM_CONTENTS,
    &memory::TRUNCATED_ADDRESS,
    &memory::DATA_SIZE_MISMATCH,
    &memory::WRITE_TO_READONLY,
    &memory::SHIFT_OUT_OF_RANGE,
    &sections::CODE_IN_DATA_SECTION,
    &sections::DATA_IN_BSS,
    &sections::UNKNOWN_SECTION,
    &sections::MISPLACED_LENGTH,
    &registers::CALLEE_SAVED_CLOBBERED,
    &registers::DEAD_STORE,
    &flags::UNDEFINED_FLAGS,
    &flags::CONSTANT_COMPARISON,
    &flags::FLAGS_CLOBBERED,
    &flags::UNUSED_COMPARISON,
    &symbols::UNUSED_LABEL,
    &sections::UNUSED_EXTERN,
    &control::REDUNDANT_JUMP,
    &control::JUMP_OVER_JUMP,
    &control::JUMP_TO_JUMP,
    &style::MOV_ZERO,
    &style::CMP_ZERO,
    &style::SELF_MOVE,
    &style::NOOP_ARITHMETIC,
    &style::LEA_AS_MOV,
    &style::REDUNDANT_TEST,
    &style::REDUNDANT_PUSH_POP,
    &style::REDUNDANT_MOVE,
    &style::NEEDLESS_REX,
    &style::SLOW_LOOP,
    &style::ABSOLUTE_ADDRESS,
];

const PASSES: &[Pass] = &[
    operands::check,
    typos::check,
    calls::check,
    memory::check,
    sections::check,
    symbols::check,
    stack::check,
    control::check,
    division::check,
    definedness::check,
    registers::check,
    flags::check,
    syscall::check,
    style::check,
];

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
