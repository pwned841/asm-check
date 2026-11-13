//! Instruction semantics: which registers and flags an instruction reads and
//! writes, and where control goes next.

use crate::ast::{Instruction, Operand, OperandKind};
use crate::mnemonic::condition;
use crate::reg::{Reg, RegId, RegSet};

/// A set of status flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flags(u8);

impl Flags {
    pub const NONE: Flags = Flags(0);
    pub const CF: Flags = Flags(1);
    pub const PF: Flags = Flags(2);
    pub const AF: Flags = Flags(4);
    pub const ZF: Flags = Flags(8);
    pub const SF: Flags = Flags(16);
    pub const OF: Flags = Flags(32);
    pub const ALL: Flags = Flags(63);

    pub const fn union(self, other: Flags) -> Flags {
        Flags(self.0 | other.0)
    }

    pub const fn intersection(self, other: Flags) -> Flags {
        Flags(self.0 & other.0)
    }

    pub const fn difference(self, other: Flags) -> Flags {
        Flags(self.0 & !other.0)
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn intersects(self, other: Flags) -> bool {
        self.0 & other.0 != 0
    }

    /// The names of the flags in the set, e.g. `["CF", "ZF"]`.
    pub fn names(self) -> Vec<&'static str> {
        [
            (Flags::CF, "CF"),
            (Flags::PF, "PF"),
            (Flags::AF, "AF"),
            (Flags::ZF, "ZF"),
            (Flags::SF, "SF"),
            (Flags::OF, "OF"),
        ]
        .into_iter()
        .filter(|(flag, _)| self.intersects(*flag))
        .map(|(_, name)| name)
        .collect()
    }
}

/// Where a jump or call goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A named label (qualified).
    Label(String),
    /// A register, memory operand or raw address.
    Indirect,
}

/// What happens to control after an instruction.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Flow {
    /// Execution continues with the next instruction.
    #[default]
    Next,
    Jump(Target),
    /// May jump, may fall through.
    CondJump(Target),
    Call(Target),
    Ret,
    Syscall,
    /// Software interrupt, with its vector when constant.
    Int(Option<i64>),
    /// `hlt`, `ud2`: execution does not continue.
    Halt,
}

/// The effects of one instruction.
///
/// For `call`, `syscall`, `int` and `ret`, the register sets only describe
/// the instruction's own operands: what the callee or the kernel reads and
/// writes depends on a calling convention, see [`crate::abi`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Effects {
    /// `false` when the mnemonic is not modelled. The register sets are then
    /// a best guess and analyses must be conservative.
    pub known: bool,
    pub reads: RegSet,
    /// Registers written, at any width.
    pub writes: RegSet,
    /// Subset of `writes` whose whole 64 bits are replaced.
    pub full_writes: RegSet,
    pub flags_read: Flags,
    pub flags_written: Flags,
    pub flow: Flow,
    /// The destination, when it is an explicit register operand.
    pub dest: Option<Reg>,
    /// The only effect is to write `dest` (and possibly flags): removing the
    /// instruction is unobservable if `dest` is overwritten before any read.
    pub pure: bool,
    /// `xor r, r` or `sub r, r`: sets the register to zero.
    pub zeroing: bool,
}

impl Effects {
    fn read(&mut self, operand: Option<&Operand>) {
        match operand.map(|op| &op.kind) {
            Some(OperandKind::Reg(reg)) => self.reads.insert(reg.id),
            Some(OperandKind::Mem(mem)) => {
                mem.regs.iter().for_each(|reg| self.reads.insert(reg.id))
            }
            _ => {}
        }
    }

    fn write(&mut self, operand: Option<&Operand>) {
        match operand.map(|op| &op.kind) {
            Some(OperandKind::Reg(reg)) => {
                self.writes.insert(reg.id);
                if reg.width.is_full() {
                    self.full_writes.insert(reg.id);
                }
                self.dest = Some(*reg);
            }
            // Storing to memory reads the registers that form the address.
            Some(OperandKind::Mem(mem)) => {
                mem.regs.iter().for_each(|reg| self.reads.insert(reg.id))
            }
            _ => {}
        }
    }

    fn modify(&mut self, operand: Option<&Operand>) {
        self.read(operand);
        self.write(operand);
    }

    fn implicit_read(&mut self, ids: &[RegId]) {
        ids.iter().for_each(|id| self.reads.insert(*id));
    }

    fn implicit_write(&mut self, ids: &[RegId]) {
        for id in ids {
            self.writes.insert(*id);
            self.full_writes.insert(*id);
        }
    }

    fn flags(&mut self, read: Flags, written: Flags) {
        self.flags_read = self.flags_read.union(read);
        self.flags_written = self.flags_written.union(written);
    }
}

/// The flags tested by a condition code such as `ne` or `ge`.
pub fn condition_flags(code: &str) -> Flags {
    match code {
        "o" | "no" => Flags::OF,
        "b" | "c" | "nae" | "nb" | "nc" | "ae" => Flags::CF,
        "e" | "z" | "ne" | "nz" => Flags::ZF,
        "be" | "na" | "a" | "nbe" => Flags::CF.union(Flags::ZF),
        "s" | "ns" => Flags::SF,
        "p" | "pe" | "np" | "po" => Flags::PF,
        "l" | "nge" | "ge" | "nl" => Flags::SF.union(Flags::OF),
        _ => Flags::ZF.union(Flags::SF).union(Flags::OF),
    }
}

fn target(operand: Option<&Operand>) -> Target {
    match operand.and_then(Operand::as_imm) {
        Some(expr) if expr.symbols.len() == 1 => Target::Label(expr.symbols[0].clone()),
        _ => Target::Indirect,
    }
}

fn same_register(a: Option<&Operand>, b: Option<&Operand>) -> bool {
    matches!((a.and_then(Operand::as_reg), b.and_then(Operand::as_reg)), (Some(x), Some(y)) if x == y)
}

/// Computes the effects of an instruction.
pub fn effects(ins: &Instruction) -> Effects {
    use RegId as R;

    let mut fx = Effects { known: true, ..Effects::default() };
    let op = |index: usize| ins.operands.get(index);
    let arity = ins.operands.len();
    let mnemonic = ins.mnemonic.as_str();

    match mnemonic {
        "mov" | "movzx" | "movsx" | "movsxd" | "movabs" | "lea" => {
            fx.write(op(0));
            fx.read(op(1));
            fx.pure = fx.dest.is_some();
        }
        "add" | "sub" | "and" | "or" | "xor" => {
            if matches!(mnemonic, "xor" | "sub") && same_register(op(0), op(1)) {
                fx.write(op(0));
                fx.pure = true;
                fx.zeroing = true;
            } else {
                fx.modify(op(0));
                fx.read(op(1));
            }
            fx.flags(Flags::NONE, Flags::ALL);
        }
        "adc" | "sbb" => {
            fx.modify(op(0));
            fx.read(op(1));
            fx.flags(Flags::CF, Flags::ALL);
        }
        "cmp" | "test" => {
            fx.read(op(0));
            fx.read(op(1));
            fx.flags(Flags::NONE, Flags::ALL);
        }
        "inc" | "dec" => {
            fx.modify(op(0));
            fx.flags(Flags::NONE, Flags::ALL.difference(Flags::CF));
        }
        "neg" => {
            fx.modify(op(0));
            fx.flags(Flags::NONE, Flags::ALL);
        }
        "not" | "bswap" => fx.modify(op(0)),
        "shl" | "sal" | "shr" | "sar" | "shld" | "shrd" => {
            fx.modify(op(0));
            fx.read(op(1));
            fx.read(op(2));
            fx.flags(Flags::NONE, Flags::ALL);
        }
        "rol" | "ror" | "rcl" | "rcr" => {
            fx.modify(op(0));
            fx.read(op(1));
            let carry_in = if mnemonic.starts_with("rc") { Flags::CF } else { Flags::NONE };
            fx.flags(carry_in, Flags::CF.union(Flags::OF));
        }
        "imul" if arity >= 2 => {
            if arity == 2 {
                fx.modify(op(0));
            } else {
                fx.write(op(0));
            }
            fx.read(op(1));
            fx.flags(Flags::NONE, Flags::ALL);
        }
        "mul" | "imul" | "div" | "idiv" => {
            fx.read(op(0));
            fx.implicit_read(&[R::RAX]);
            if op(0).and_then(Operand::bits) == Some(8) {
                // 8-bit forms work on `ax` alone.
                fx.implicit_write(&[R::RAX]);
            } else {
                if mnemonic.ends_with("div") {
                    fx.implicit_read(&[R::RDX]);
                }
                fx.implicit_write(&[R::RAX, R::RDX]);
            }
            fx.flags(Flags::NONE, Flags::ALL);
        }
        "cqo" | "cdq" | "cwd" => {
            fx.implicit_read(&[R::RAX]);
            fx.implicit_write(&[R::RDX]);
        }
        "cdqe" | "cwde" | "cbw" => {
            fx.implicit_read(&[R::RAX]);
            fx.implicit_write(&[R::RAX]);
        }
        "push" => {
            fx.read(op(0));
            fx.implicit_read(&[R::RSP]);
            fx.implicit_write(&[R::RSP]);
        }
        "pop" => {
            fx.write(op(0));
            fx.implicit_read(&[R::RSP]);
            fx.implicit_write(&[R::RSP]);
        }
        "pushf" | "pushfq" | "popf" | "popfq" => {
            fx.implicit_read(&[R::RSP]);
            fx.implicit_write(&[R::RSP]);
            if mnemonic.starts_with("push") {
                fx.flags(Flags::ALL, Flags::NONE);
            } else {
                fx.flags(Flags::NONE, Flags::ALL);
            }
        }
        "xchg" => {
            fx.modify(op(0));
            fx.modify(op(1));
        }
        "xadd" => {
            fx.modify(op(0));
            fx.modify(op(1));
            fx.flags(Flags::NONE, Flags::ALL);
        }
        "cmpxchg" => {
            fx.modify(op(0));
            fx.read(op(1));
            fx.implicit_read(&[R::RAX]);
            fx.implicit_write(&[R::RAX]);
            fx.flags(Flags::NONE, Flags::ALL);
        }
        "bt" => {
            fx.read(op(0));
            fx.read(op(1));
            fx.flags(Flags::NONE, Flags::CF);
        }
        "bts" | "btr" | "btc" => {
            fx.modify(op(0));
            fx.read(op(1));
            fx.flags(Flags::NONE, Flags::CF);
        }
        "bsf" | "bsr" => {
            // The destination is left unchanged when the source is zero.
            fx.modify(op(0));
            fx.read(op(1));
            fx.flags(Flags::NONE, Flags::ALL);
        }
        "popcnt" | "lzcnt" | "tzcnt" => {
            fx.write(op(0));
            fx.read(op(1));
            fx.flags(Flags::NONE, Flags::ALL);
        }
        "jmp" => {
            fx.read(op(0));
            fx.flow = Flow::Jump(target(op(0)));
        }
        "call" => {
            fx.read(op(0));
            fx.flow = Flow::Call(target(op(0)));
        }
        "ret" | "retn" | "retf" => fx.flow = Flow::Ret,
        "syscall" => fx.flow = Flow::Syscall,
        "int" => fx.flow = Flow::Int(op(0).and_then(Operand::constant)),
        "hlt" | "ud2" => fx.flow = Flow::Halt,
        "nop" | "pause" | "endbr64" | "int3" | "cld" | "std" => {}
        "clc" | "stc" => fx.flags(Flags::NONE, Flags::CF),
        "cmc" => fx.flags(Flags::CF, Flags::CF),
        "lahf" => {
            fx.writes.insert(R::RAX);
            fx.flags(Flags::ALL.difference(Flags::OF), Flags::NONE);
        }
        "sahf" => {
            fx.implicit_read(&[R::RAX]);
            fx.flags(Flags::NONE, Flags::ALL.difference(Flags::OF));
        }
        "leave" => {
            fx.implicit_read(&[R::RBP]);
            fx.implicit_write(&[R::RSP, R::RBP]);
        }
        "enter" => {
            fx.implicit_read(&[R::RSP, R::RBP]);
            fx.implicit_write(&[R::RSP, R::RBP]);
        }
        "loop" | "loope" | "loopz" | "loopne" | "loopnz" => {
            fx.implicit_read(&[R::RCX]);
            fx.implicit_write(&[R::RCX]);
            if mnemonic != "loop" {
                fx.flags(Flags::ZF, Flags::NONE);
            }
            fx.flow = Flow::CondJump(target(op(0)));
        }
        "jrcxz" | "jecxz" => {
            fx.implicit_read(&[R::RCX]);
            fx.flow = Flow::CondJump(target(op(0)));
        }
        "cpuid" => {
            fx.implicit_read(&[R::RAX, R::RCX]);
            fx.implicit_write(&[R::RAX, R::RBX, R::RCX, R::RDX]);
        }
        "rdtsc" => fx.implicit_write(&[R::RAX, R::RDX]),
        "rdtscp" => fx.implicit_write(&[R::RAX, R::RCX, R::RDX]),
        _ => {
            if let Some((family, code)) = condition(mnemonic) {
                fx.flags(condition_flags(code), Flags::NONE);
                match family {
                    "j" => fx.flow = Flow::CondJump(target(op(0))),
                    "set" => fx.write(op(0)),
                    _ => {
                        // cmovcc keeps the old value when the condition fails.
                        fx.modify(op(0));
                        fx.read(op(1));
                    }
                }
            } else if !string_operation(ins, &mut fx) {
                unknown(ins, &mut fx);
            }
        }
    }

    if matches!(ins.prefix.as_deref(), Some("rep" | "repe" | "repz" | "repne" | "repnz")) {
        fx.implicit_read(&[R::RCX]);
        fx.implicit_write(&[R::RCX]);
    }
    fx
}

/// `movsb`, `stosq`, ... in their operand-less form. With operands, `movsd`
/// and `cmpsd` are SSE instructions and are left to [`unknown`].
fn string_operation(ins: &Instruction, fx: &mut Effects) -> bool {
    use RegId as R;

    if !ins.operands.is_empty() || ins.mnemonic.len() != 5 {
        return false;
    }
    let (family, width) = ins.mnemonic.split_at(4);
    if !matches!(width, "b" | "w" | "d" | "q") {
        return false;
    }
    match family {
        "movs" => {
            fx.implicit_read(&[R::RSI, R::RDI]);
            fx.implicit_write(&[R::RSI, R::RDI]);
        }
        "stos" => {
            fx.implicit_read(&[R::RAX, R::RDI]);
            fx.implicit_write(&[R::RDI]);
        }
        "lods" => {
            fx.implicit_read(&[R::RSI]);
            fx.implicit_write(&[R::RSI]);
            fx.writes.insert(R::RAX);
            if matches!(width, "d" | "q") {
                fx.full_writes.insert(R::RAX);
            }
        }
        "scas" => {
            fx.implicit_read(&[R::RAX, R::RDI]);
            fx.implicit_write(&[R::RDI]);
            fx.flags(Flags::NONE, Flags::ALL);
        }
        "cmps" => {
            fx.implicit_read(&[R::RSI, R::RDI]);
            fx.implicit_write(&[R::RSI, R::RDI]);
            fx.flags(Flags::NONE, Flags::ALL);
        }
        _ => return false,
    }
    true
}

/// Best guess for an instruction that is not modelled: every register that
/// appears is read, and a register in destination position is also written.
fn unknown(ins: &Instruction, fx: &mut Effects) {
    fx.known = false;
    for operand in &ins.operands {
        fx.read(Some(operand));
    }
    if let Some(reg) = ins.operands.first().and_then(Operand::as_reg) {
        fx.writes.insert(reg.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::StmtKind;
    use crate::mnemonic::known_mnemonics;
    use crate::parser::parse;

    fn fx(source: &str) -> Effects {
        match parse(source).pop().unwrap().stmt.unwrap().kind {
            StmtKind::Instruction(ins) => effects(&ins),
            other => panic!("expected an instruction, got {other:?}"),
        }
    }

    fn set(ids: &[RegId]) -> RegSet {
        RegSet::of(ids)
    }

    #[test]
    fn mov_reads_the_address_registers() {
        let e = fx("mov rax, [rbx + rcx*8]");
        assert_eq!(e.reads, set(&[RegId::RBX, RegId::RCX]));
        assert_eq!(e.writes, set(&[RegId::RAX]));
        assert!(e.pure);

        let store = fx("mov [rdi], al");
        assert_eq!(store.reads, set(&[RegId::RDI, RegId::RAX]));
        assert!(store.writes.is_empty() && !store.pure);
    }

    #[test]
    fn partial_writes_are_not_full() {
        assert!(fx("mov al, 1").full_writes.is_empty());
        assert!(fx("mov ax, 1").full_writes.is_empty());
        assert_eq!(fx("mov eax, 1").full_writes, set(&[RegId::RAX]));
    }

    #[test]
    fn zero_idiom_does_not_read() {
        let e = fx("xor edi, edi");
        assert!(e.reads.is_empty() && e.zeroing && e.pure);
        assert_eq!(fx("xor rax, rbx").reads, set(&[RegId::RAX, RegId::RBX]));
        assert!(!fx("xor rax, rbx").zeroing);
    }

    #[test]
    fn division_uses_rdx_rax() {
        let e = fx("div rbx");
        assert_eq!(e.reads, set(&[RegId::RAX, RegId::RDX, RegId::RBX]));
        assert_eq!(e.writes, set(&[RegId::RAX, RegId::RDX]));
        let byte = fx("div bl");
        assert_eq!(byte.reads, set(&[RegId::RAX, RegId::RBX]));
        assert_eq!(byte.writes, set(&[RegId::RAX]));
        assert_eq!(fx("mul rcx").reads, set(&[RegId::RAX, RegId::RCX]));
    }

    #[test]
    fn imul_forms() {
        assert_eq!(fx("imul rbx").writes, set(&[RegId::RAX, RegId::RDX]));
        assert_eq!(fx("imul rax, rbx").reads, set(&[RegId::RAX, RegId::RBX]));
        assert_eq!(fx("imul rax, rbx, 10").reads, set(&[RegId::RBX]));
    }

    #[test]
    fn flags() {
        assert_eq!(fx("cmp rax, 1").flags_written, Flags::ALL);
        assert!(!fx("inc rax").flags_written.intersects(Flags::CF));
        assert_eq!(fx("jb done").flags_read, Flags::CF);
        assert_eq!(fx("jne done").flags_read, Flags::ZF);
        assert_eq!(fx("cmovg rax, rbx").flags_read, Flags::ZF.union(Flags::SF).union(Flags::OF));
        assert!(fx("mov rax, 1").flags_written.is_empty());
    }

    #[test]
    fn control_flow() {
        assert_eq!(fx("jmp done").flow, Flow::Jump(Target::Label("done".into())));
        assert_eq!(fx("jmp rax").flow, Flow::Jump(Target::Indirect));
        assert_eq!(fx("jne done").flow, Flow::CondJump(Target::Label("done".into())));
        assert_eq!(fx("call printf wrt ..plt").flow, Flow::Call(Target::Label("printf".into())));
        assert_eq!(fx("ret").flow, Flow::Ret);
        assert_eq!(fx("int 0x80").flow, Flow::Int(Some(0x80)));
    }

    #[test]
    fn string_operations_and_rep() {
        let e = fx("rep movsb");
        assert_eq!(e.reads, set(&[RegId::RSI, RegId::RDI, RegId::RCX]));
        assert!(!fx("movsd xmm0, [rax]").known);
    }

    #[test]
    fn unknown_instructions_are_flagged() {
        let e = fx("vaddps ymm0, ymm1, [rax]");
        assert!(!e.known);
        assert_eq!(e.reads, set(&[RegId::RAX]));
    }

    #[test]
    fn every_listed_mnemonic_is_modelled() {
        for mnemonic in known_mnemonics() {
            assert!(fx(mnemonic).known, "`{mnemonic}` is listed but not modelled");
        }
    }
}
