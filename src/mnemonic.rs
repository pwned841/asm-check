//! Vocabulary of the assembler: mnemonics, directives and keywords.

/// Instructions whose behaviour [`crate::insn`] models.
///
/// Conditional families (`jcc`, `setcc`, `cmovcc`) are matched by
/// [`condition`] instead of being listed here.
const KNOWN: &[&str] = &[
    "adc", "add", "and", "bsf", "bsr", "bswap", "bt", "btc", "btr", "bts", "call", "cbw", "cdq",
    "cdqe", "clc", "cld", "cmc", "cmp", "cmpsb", "cmpsd", "cmpsq", "cmpsw", "cmpxchg", "cpuid",
    "cqo", "cwd", "cwde", "dec", "div", "endbr64", "enter", "hlt", "idiv", "imul", "inc", "int",
    "int3", "jecxz", "jmp", "jrcxz", "lahf", "lea", "leave", "lodsb", "lodsd", "lodsq", "lodsw",
    "loop", "loope", "loopne", "loopnz", "loopz", "lzcnt", "mov", "movabs", "movsb", "movsd",
    "movsq", "movsw", "movsx", "movsxd", "movzx", "mul", "neg", "nop", "not", "or", "pause", "pop",
    "popcnt", "popf", "popfq", "push", "pushf", "pushfq", "rcl", "rcr", "rdtsc", "rdtscp", "ret",
    "retf", "retn", "rol", "ror", "sahf", "sal", "sar", "sbb", "scasb", "scasd", "scasq", "scasw",
    "shl", "shld", "shr", "shrd", "stc", "std", "stosb", "stosd", "stosq", "stosw", "sub",
    "syscall", "test", "tzcnt", "ud2", "xadd", "xchg", "xor",
];

const DATA_DIRECTIVES: &[&str] = &[
    "db", "dw", "dd", "dq", "dt", "do", "dy", "dz", "resb", "resw", "resd", "resq", "rest", "reso",
    "resy", "resz", "incbin",
];

const DIRECTIVES: &[&str] = &[
    "section",
    "segment",
    "global",
    "globl",
    "extern",
    "common",
    "static",
    "bits",
    "use16",
    "use32",
    "use64",
    "default",
    "align",
    "alignb",
    "sectalign",
    "org",
    "cpu",
    "float",
    "struc",
    "endstruc",
    "istruc",
    "iend",
    "at",
    "absolute",
    "required",
    "warning",
    "group",
    "import",
    "export",
];

const PREFIXES: &[&str] =
    &["rep", "repe", "repz", "repne", "repnz", "lock", "bnd", "notrack", "xacquire", "xrelease"];

const SIZES: &[(&str, u32)] = &[
    ("byte", 8),
    ("word", 16),
    ("dword", 32),
    ("qword", 64),
    ("tword", 80),
    ("oword", 128),
    ("yword", 256),
    ("zword", 512),
];

/// Splits a conditional mnemonic into its family and condition code:
/// `jne` gives `("j", "ne")`, `cmovge` gives `("cmov", "ge")`.
pub fn condition(mnemonic: &str) -> Option<(&'static str, &str)> {
    const CODES: &[&str] = &[
        "o", "no", "b", "c", "nae", "nb", "nc", "ae", "e", "z", "ne", "nz", "be", "na", "a", "nbe",
        "s", "ns", "p", "pe", "np", "po", "l", "nge", "ge", "nl", "le", "ng", "g", "nle",
    ];
    ["cmov", "set", "j"].into_iter().find_map(|family| {
        let code = mnemonic.strip_prefix(family)?;
        CODES.contains(&code).then_some((family, code))
    })
}

pub fn is_known_mnemonic(word: &str) -> bool {
    KNOWN.contains(&word) || condition(word).is_some()
}

pub fn known_mnemonics() -> &'static [&'static str] {
    KNOWN
}

pub fn is_data_directive(word: &str) -> bool {
    DATA_DIRECTIVES.contains(&word)
}

pub fn is_directive(word: &str) -> bool {
    DIRECTIVES.contains(&word)
}

pub fn is_prefix(word: &str) -> bool {
    PREFIXES.contains(&word)
}

/// The size in bits named by a keyword such as `dword`.
pub fn size_keyword(word: &str) -> Option<u32> {
    SIZES.iter().find(|(name, _)| *name == word).map(|(_, bits)| *bits)
}

/// Keywords that may appear inside an operand without naming a symbol.
pub fn is_operand_keyword(word: &str) -> bool {
    size_keyword(word).is_some()
        || matches!(
            word,
            "ptr" | "short" | "near" | "far" | "strict" | "rel" | "abs" | "seg" | "nosplit" | "dup"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_conditions() {
        assert_eq!(condition("jne"), Some(("j", "ne")));
        assert_eq!(condition("setz"), Some(("set", "z")));
        assert_eq!(condition("cmovnle"), Some(("cmov", "nle")));
        assert_eq!(condition("jmp"), None);
        assert_eq!(condition("j"), None);
    }

    #[test]
    fn known_list_is_sorted_and_unique() {
        assert!(KNOWN.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
