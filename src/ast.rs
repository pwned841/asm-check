//! The syntax tree produced by [`crate::parser`].
//!
//! Assembly is line-oriented, so the tree is a flat list of [`Line`]s. Each
//! line carries an optional label, an optional statement and an optional
//! trailing comment.

use crate::reg::Reg;
use crate::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// 0-based line number.
    pub number: usize,
    pub label: Option<Label>,
    pub stmt: Option<Stmt>,
    /// Comment text without the leading `;`, trimmed.
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    /// Fully qualified name: a local label `.loop` defined after `main`
    /// is stored as `main.loop`.
    pub name: String,
    pub span: Span,
    pub local: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Instruction(Instruction),
    /// `db`, `dq`, `resb`, `incbin`, ...
    Data {
        /// Lowercased directive, e.g. `db`.
        directive: String,
        /// Names referenced by the initialisers.
        symbols: Vec<String>,
        /// Bytes occupied, when the directive and its operands say so.
        size: Option<u64>,
    },
    /// `name equ expr`.
    Equ {
        symbols: Vec<String>,
        /// The expression uses `$`, the current position.
        here: bool,
    },
    Directive(Directive),
    /// A `%`-prefixed preprocessor line. `name` is the symbol it introduces,
    /// for the directives that define one (`%define`, `%assign`, ...).
    Preproc {
        directive: String,
        name: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Directive {
    Section(String),
    Global(Vec<String>),
    Extern(Vec<String>),
    Bits(u32),
    Other(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Instruction {
    /// `rep`, `lock`, `times`, ... lowercased.
    pub prefix: Option<String>,
    /// Lowercased mnemonic.
    pub mnemonic: String,
    pub mnemonic_span: Span,
    pub operands: Vec<Operand>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Operand {
    pub kind: OperandKind,
    pub span: Span,
    /// Explicit size keyword (`byte`, `qword`, ...) in bits.
    pub size: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OperandKind {
    Reg(Reg),
    /// A register this tool does not track (`xmm0`, `fs`, ...).
    OtherReg(String),
    Imm(Expr),
    Mem(Mem),
}

/// An immediate expression: a constant, a symbol, or a mix of both.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Expr {
    /// The value, when the expression is a compile-time constant.
    pub value: Option<i64>,
    /// Qualified names of the symbols the expression refers to.
    pub symbols: Vec<String>,
}

/// A memory operand, `[base + index*scale + disp]`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mem {
    /// Registers used to compute the address.
    pub regs: Vec<Reg>,
    pub symbols: Vec<String>,
    /// Scaled registers: `rcx*8` gives `(rcx, 8)`.
    pub scales: Vec<(Reg, i64)>,
    /// The address is exactly one symbol: `[counter]`, `[rel counter]`.
    pub direct: bool,
    /// Addressed relative to the instruction pointer (`rel`, or `rip`).
    pub relative: bool,
}

impl Operand {
    pub fn as_reg(&self) -> Option<Reg> {
        match self.kind {
            OperandKind::Reg(reg) => Some(reg),
            _ => None,
        }
    }

    pub fn as_imm(&self) -> Option<&Expr> {
        match &self.kind {
            OperandKind::Imm(expr) => Some(expr),
            _ => None,
        }
    }

    /// The constant value of an immediate operand.
    pub fn constant(&self) -> Option<i64> {
        self.as_imm().and_then(|expr| expr.value)
    }

    pub fn is_mem(&self) -> bool {
        matches!(self.kind, OperandKind::Mem(_))
    }

    pub fn is_imm(&self) -> bool {
        matches!(self.kind, OperandKind::Imm(_))
    }

    /// Operand size in bits when the operand itself determines it:
    /// a register's width, or an explicit size keyword.
    pub fn bits(&self) -> Option<u32> {
        match self.kind {
            OperandKind::Reg(reg) => Some(reg.width.bits()),
            _ => self.size,
        }
    }

    /// Names of the symbols this operand refers to.
    pub fn symbols(&self) -> &[String] {
        match &self.kind {
            OperandKind::Imm(expr) => &expr.symbols,
            OperandKind::Mem(mem) => &mem.symbols,
            _ => &[],
        }
    }
}
