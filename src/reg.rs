//! x86-64 general-purpose registers.
//!
//! Every sub-register (`al`, `ax`, `eax`, `rax`, ...) is modelled as a
//! [`RegId`] naming the 64-bit register it belongs to, plus a [`Width`].

use std::fmt;

/// Which part of a 64-bit register an operand names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Width {
    /// Bits 0-7 (`al`, `sil`, `r8b`).
    Low8,
    /// Bits 8-15 (`ah`, `bh`, `ch`, `dh`).
    High8,
    W16,
    W32,
    W64,
}

impl Width {
    pub const fn bits(self) -> u32 {
        match self {
            Width::Low8 | Width::High8 => 8,
            Width::W16 => 16,
            Width::W32 => 32,
            Width::W64 => 64,
        }
    }

    /// Whether a write of this width replaces the whole 64-bit register.
    ///
    /// Writing a 32-bit register zero-extends into the upper half, so it
    /// counts as a full write; 8- and 16-bit writes keep the other bits.
    pub const fn is_full(self) -> bool {
        matches!(self, Width::W32 | Width::W64)
    }
}

/// One of the sixteen 64-bit general-purpose registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RegId(u8);

const NAMES64: [&str; 16] = [
    "rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11", "r12", "r13",
    "r14", "r15",
];
const NAMES32: [&str; 16] = [
    "eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi", "r8d", "r9d", "r10d", "r11d", "r12d",
    "r13d", "r14d", "r15d",
];
const NAMES16: [&str; 16] = [
    "ax", "cx", "dx", "bx", "sp", "bp", "si", "di", "r8w", "r9w", "r10w", "r11w", "r12w", "r13w",
    "r14w", "r15w",
];
const NAMES8: [&str; 16] = [
    "al", "cl", "dl", "bl", "spl", "bpl", "sil", "dil", "r8b", "r9b", "r10b", "r11b", "r12b",
    "r13b", "r14b", "r15b",
];
const NAMES8_ALT: [&str; 8] = ["r8l", "r9l", "r10l", "r11l", "r12l", "r13l", "r14l", "r15l"];
const NAMES8_HIGH: [&str; 4] = ["ah", "ch", "dh", "bh"];

impl RegId {
    pub const RAX: RegId = RegId(0);
    pub const RCX: RegId = RegId(1);
    pub const RDX: RegId = RegId(2);
    pub const RBX: RegId = RegId(3);
    pub const RSP: RegId = RegId(4);
    pub const RBP: RegId = RegId(5);
    pub const RSI: RegId = RegId(6);
    pub const RDI: RegId = RegId(7);
    pub const R8: RegId = RegId(8);
    pub const R9: RegId = RegId(9);
    pub const R10: RegId = RegId(10);
    pub const R11: RegId = RegId(11);
    pub const R12: RegId = RegId(12);
    pub const R13: RegId = RegId(13);
    pub const R14: RegId = RegId(14);
    pub const R15: RegId = RegId(15);

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// The register name at the given width, if that sub-register exists
    /// (only `rax`..`rbx` have a high-byte form).
    pub fn name(self, width: Width) -> Option<&'static str> {
        let i = self.index();
        match width {
            Width::W64 => Some(NAMES64[i]),
            Width::W32 => Some(NAMES32[i]),
            Width::W16 => Some(NAMES16[i]),
            Width::Low8 => Some(NAMES8[i]),
            Width::High8 => NAMES8_HIGH.get(i).copied(),
        }
    }

    /// The 64-bit name, e.g. `rax`.
    pub fn name64(self) -> &'static str {
        NAMES64[self.index()]
    }

    /// The 32-bit name, e.g. `eax`.
    pub fn name32(self) -> &'static str {
        NAMES32[self.index()]
    }
}

impl fmt::Display for RegId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name64())
    }
}

/// A register operand: which register, and which part of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Reg {
    pub id: RegId,
    pub width: Width,
}

impl Reg {
    pub const fn new(id: RegId, width: Width) -> Self {
        Self { id, width }
    }

    /// Parses a general-purpose register name, case-insensitively.
    pub fn parse(name: &str) -> Option<Reg> {
        let lower = name.to_ascii_lowercase();
        let find = |table: &[&str]| table.iter().position(|n| *n == lower);
        let reg = |i: usize, width| Reg::new(RegId(i as u8), width);
        if let Some(i) = find(&NAMES64) {
            Some(reg(i, Width::W64))
        } else if let Some(i) = find(&NAMES32) {
            Some(reg(i, Width::W32))
        } else if let Some(i) = find(&NAMES16) {
            Some(reg(i, Width::W16))
        } else if let Some(i) = find(&NAMES8) {
            Some(reg(i, Width::Low8))
        } else if let Some(i) = find(&NAMES8_ALT) {
            Some(reg(i + 8, Width::Low8))
        } else {
            find(&NAMES8_HIGH).map(|i| reg(i, Width::High8))
        }
    }

    pub fn name(self) -> &'static str {
        self.id.name(self.width).unwrap_or("?")
    }
}

impl fmt::Display for Reg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Every general-purpose register name, at every width.
pub fn names() -> impl Iterator<Item = &'static str> {
    NAMES64.into_iter().chain(NAMES32).chain(NAMES16).chain(NAMES8).chain(NAMES8_HIGH)
}

/// Whether `name` is a register outside the general-purpose set
/// (vector, x87, segment, control, mask, instruction pointer).
pub fn is_other_register(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "cs" | "ds" | "es" | "fs" | "gs" | "ss" | "rip" | "eip" | "ip" | "st" | "rflags" | "eflags"
    ) {
        return true;
    }
    ["xmm", "ymm", "zmm", "tmm", "mm", "st", "cr", "dr", "tr", "k"].iter().any(|prefix| {
        lower.strip_prefix(prefix).is_some_and(|rest| {
            !rest.is_empty() && rest.len() <= 2 && rest.bytes().all(|b| b.is_ascii_digit())
        })
    })
}

/// A set of 64-bit registers, stored as a bitmask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RegSet(u16);

impl RegSet {
    pub const EMPTY: RegSet = RegSet(0);

    pub const fn of(ids: &[RegId]) -> RegSet {
        let mut bits = 0;
        let mut i = 0;
        while i < ids.len() {
            bits |= 1 << ids[i].0;
            i += 1;
        }
        RegSet(bits)
    }

    pub fn insert(&mut self, id: RegId) {
        self.0 |= 1 << id.0;
    }

    pub fn remove(&mut self, id: RegId) {
        self.0 &= !(1 << id.0);
    }

    pub const fn contains(self, id: RegId) -> bool {
        self.0 & (1 << id.0) != 0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn union(self, other: RegSet) -> RegSet {
        RegSet(self.0 | other.0)
    }

    pub const fn intersection(self, other: RegSet) -> RegSet {
        RegSet(self.0 & other.0)
    }

    pub const fn difference(self, other: RegSet) -> RegSet {
        RegSet(self.0 & !other.0)
    }

    pub fn iter(self) -> impl Iterator<Item = RegId> {
        (0..16u8).map(RegId).filter(move |id| self.contains(*id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_width() {
        assert_eq!(Reg::parse("RAX"), Some(Reg::new(RegId::RAX, Width::W64)));
        assert_eq!(Reg::parse("r10d"), Some(Reg::new(RegId::R10, Width::W32)));
        assert_eq!(Reg::parse("si"), Some(Reg::new(RegId::RSI, Width::W16)));
        assert_eq!(Reg::parse("dil"), Some(Reg::new(RegId::RDI, Width::Low8)));
        assert_eq!(Reg::parse("r9l"), Some(Reg::new(RegId::R9, Width::Low8)));
        assert_eq!(Reg::parse("bh"), Some(Reg::new(RegId::RBX, Width::High8)));
        assert_eq!(Reg::parse("xmm0"), None);
        assert_eq!(Reg::parse("buffer"), None);
    }

    #[test]
    fn names_round_trip() {
        for name in NAMES64.iter().chain(&NAMES32).chain(&NAMES16).chain(&NAMES8) {
            assert_eq!(Reg::parse(name).unwrap().name(), *name);
        }
    }

    #[test]
    fn recognises_other_registers() {
        assert!(is_other_register("xmm15"));
        assert!(is_other_register("FS"));
        assert!(is_other_register("rip"));
        assert!(!is_other_register("k"));
        assert!(!is_other_register("stack"));
    }

    #[test]
    fn regset_operations() {
        let mut set = RegSet::of(&[RegId::RAX, RegId::R11]);
        assert!(set.contains(RegId::R11));
        set.remove(RegId::R11);
        set.insert(RegId::RDX);
        assert_eq!(set.iter().collect::<Vec<_>>(), vec![RegId::RAX, RegId::RDX]);
        assert!(set.difference(RegSet::of(&[RegId::RAX, RegId::RDX])).is_empty());
    }
}
