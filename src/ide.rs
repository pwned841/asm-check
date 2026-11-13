//! Editor features other than diagnostics: hover, navigation, outline,
//! inlay hints and completion.
//!
//! Everything here works on source text and plain spans, so it can be
//! tested without a language server; [`crate::lsp`] only translates.

use crate::ast::{Line, StmtKind};
use crate::insn::{Flags, Flow};
use crate::mnemonic::{all_known, is_data_directive, is_directive};
use crate::parser::{self, comment_start, is_ident_byte, parse_number};
use crate::program::{Program, SymbolKind};
use crate::reg::{Reg, RegId, RegSet, Width};
use crate::span::Span;
use crate::{abi, docs, reg, syscalls};

/// A parsed document.
pub struct Document {
    text: Vec<String>,
    lines: Vec<Line>,
}

/// What a name in the outline is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolClass {
    Function,
    Label,
    Data,
    Constant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outline {
    pub name: String,
    pub class: SymbolClass,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Instruction,
    Register,
    Label,
    Keyword,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: String,
}

impl Document {
    pub fn new(source: &str) -> Self {
        Self { text: source.lines().map(str::to_string).collect(), lines: parser::parse(source) }
    }

    /// The identifier-like word at a byte column, outside comments.
    fn word_at(&self, line: usize, column: usize) -> Option<(Span, &str)> {
        let text = self.text.get(line)?;
        let bytes = text.as_bytes();
        if column > bytes.len() || column >= comment_start(text) && column < bytes.len() {
            return None;
        }
        let mut start = column.min(bytes.len());
        while start > 0 && is_ident_byte(bytes[start - 1]) {
            start -= 1;
        }
        let mut end = column;
        while end < bytes.len() && is_ident_byte(bytes[end]) {
            end += 1;
        }
        (start < end).then(|| (Span::new(line, start, end), &text[start..end]))
    }

    /// The non-local label that `.local` names on `line` belong to.
    fn parent_at(&self, line: usize) -> &str {
        self.lines[..=line.min(self.lines.len().saturating_sub(1))]
            .iter()
            .rev()
            // Same rule as the parser: a lone word without a colon opens no scope.
            .filter(|line| line.stmt.is_some() || line.label.as_ref().is_some_and(|l| l.colon))
            .filter_map(|line| line.label.as_ref())
            .find(|label| !label.local && !label.name.starts_with(".."))
            .map_or("", |label| label.name.as_str())
    }

    /// The full name of a symbol as written on `line`.
    fn qualify(&self, line: usize, word: &str) -> String {
        if word.starts_with('.') && !word.starts_with("..") {
            format!("{}{word}", self.parent_at(line))
        } else {
            word.to_string()
        }
    }

    /// Every place a symbol is written, its definition included.
    fn occurrences(&self, name: &str) -> Vec<Span> {
        let mut found = Vec::new();
        for (number, text) in self.text.iter().enumerate() {
            let bytes = text.as_bytes();
            let end = comment_start(text);
            let (mut i, mut quote) = (0, None);
            while i < end {
                let b = bytes[i];
                if let Some(q) = quote {
                    quote = (b != q).then_some(q);
                    i += 1;
                } else if matches!(b, b'\'' | b'"' | b'`') {
                    quote = Some(b);
                    i += 1;
                } else if is_ident_byte(b) {
                    let start = i;
                    while i < end && is_ident_byte(bytes[i]) {
                        i += 1;
                    }
                    if self.qualify(number, &text[start..i]) == name {
                        found.push(Span::new(number, start, i));
                    }
                } else {
                    i += 1;
                }
            }
        }
        found
    }

    /// The symbol under the cursor, if it is one the file defines.
    fn symbol_at(&self, program: &Program, line: usize, column: usize) -> Option<String> {
        let (_, word) = self.word_at(line, column)?;
        let name = self.qualify(line, word);
        program.symbols.contains_key(&name).then_some(name)
    }

    pub fn definition(&self, line: usize, column: usize) -> Option<Span> {
        let program = Program::build(&self.lines);
        let name = self.symbol_at(&program, line, column)?;
        Some(program.symbols[&name].span)
    }

    pub fn references(&self, line: usize, column: usize) -> Vec<Span> {
        let program = Program::build(&self.lines);
        match self.symbol_at(&program, line, column) {
            Some(name) => self.occurrences(&name),
            None => Vec::new(),
        }
    }

    /// The edits that rename the symbol under the cursor: each span and
    /// the text to put there. `None` when the new name is not usable.
    pub fn rename(
        &self,
        line: usize,
        column: usize,
        new_name: &str,
    ) -> Option<Vec<(Span, String)>> {
        let program = Program::build(&self.lines);
        let name = self.symbol_at(&program, line, column)?;
        let bare = new_name.trim_start_matches('.');
        let valid = !bare.is_empty()
            && bare.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && !bare.as_bytes()[0].is_ascii_digit()
            && Reg::parse(bare).is_none();
        if !valid {
            return None;
        }
        // A local label stays local, whatever the user typed.
        let local = program.lines[program.symbols[&name].span.line]
            .label
            .as_ref()
            .is_some_and(|label| label.local);
        let replacement = if local { format!(".{bare}") } else { bare.to_string() };
        Some(self.occurrences(&name).into_iter().map(|span| (span, replacement.clone())).collect())
    }

    pub fn outline(&self) -> Vec<Outline> {
        let program = Program::build(&self.lines);
        let functions: Vec<&str> =
            program.functions.iter().map(|function| function.name.as_str()).collect();
        let mut outline: Vec<Outline> = program
            .symbols
            .iter()
            .map(|(name, symbol)| Outline {
                name: name.clone(),
                class: match symbol.kind {
                    SymbolKind::Code if functions.contains(&name.as_str()) => SymbolClass::Function,
                    SymbolKind::Code => SymbolClass::Label,
                    SymbolKind::Data => SymbolClass::Data,
                    SymbolKind::Constant => SymbolClass::Constant,
                },
                span: symbol.span,
            })
            .collect();
        outline.sort_by_key(|entry| (entry.span.line, entry.span.start));
        outline
    }

    /// Hints that spell out what the code does, as `(line, byte column,
    /// text)`:
    /// - after `syscall`, the call and the values of its arguments;
    /// - after a conditional jump, the condition it tests;
    /// - after a byte-sized number, the character it stands for.
    pub fn inlay_hints(&self) -> Vec<(usize, usize, String)> {
        let mut program = Program::build(&self.lines);
        program.text = self.text.iter().map(String::as_str).collect();
        let mut hints = Vec::new();
        for (index, insn) in program.insns.iter().enumerate() {
            if insn.fx.flow == Flow::Syscall
                && let Some(number) = program.constant_before(index, RegId::RAX)
                && let Some(syscall) = syscalls::lookup(number)
            {
                let arguments: Vec<String> = syscall
                    .registers()
                    .iter()
                    .enumerate()
                    .map(|(position, reg)| {
                        let name = syscall.param(position);
                        match argument_text(&program, index, *reg) {
                            Some(value) => format!("{name}={value}"),
                            None => name.to_string(),
                        }
                    })
                    .collect();
                let text = format!("{}({})", syscall.name, arguments.join(", "));
                hints.push((insn.line, insn.span.end, text));
            }
            if let Some(condition) = condition_text(&program, index) {
                hints.push((insn.line, insn.span.end, format!("if {condition}")));
            }
            // `cmp al, 48` reads better with the character next to it.
            if let [dest, source] = insn.ins.operands.as_slice()
                && dest.bits() == Some(8)
                && let Some(value) = source.constant().and_then(|value| u8::try_from(value).ok())
                && (value.is_ascii_graphic() || value == b' ')
                && program.snippet(source.span).as_bytes().first().is_some_and(u8::is_ascii_digit)
            {
                hints.push((insn.line, source.span.end, format!("'{}'", value as char)));
            }
        }
        hints.sort();
        hints
    }

    pub fn completions(&self, line: usize, column: usize) -> Vec<Completion> {
        let Some(text) = self.text.get(line) else { return Vec::new() };
        let before = &text[..column.min(text.len())];
        if column > comment_start(text) {
            return Vec::new();
        }
        // Still typing the first word of the statement: an instruction.
        let statement = before.rsplit(':').next().unwrap_or(before).trim_start();
        if !statement.contains(char::is_whitespace) {
            return all_known()
                .into_iter()
                .filter_map(|name| {
                    let detail = docs::instruction(&name)?;
                    Some(Completion { label: name, kind: CompletionKind::Instruction, detail })
                })
                .collect();
        }

        let program = Program::build(&self.lines);
        let parent = self.parent_at(line);
        let mut items: Vec<Completion> = reg::names()
            .map(|name| Completion {
                label: name.to_string(),
                kind: CompletionKind::Register,
                detail: Reg::parse(name).map_or("", |reg| docs::register(reg.id)).to_string(),
            })
            .collect();
        for size in ["byte", "word", "dword", "qword", "rel"] {
            items.push(Completion {
                label: size.to_string(),
                kind: CompletionKind::Keyword,
                detail: String::new(),
            });
        }
        let mut labels: Vec<(&String, &crate::program::Symbol)> = program.symbols.iter().collect();
        labels.sort_by_key(|(_, symbol)| symbol.span.line);
        for (name, symbol) in labels {
            let is_local = program.lines[symbol.span.line].label.as_ref().is_some_and(|l| l.local);
            // A local label is only reachable by its short name inside its parent.
            let label = match name.strip_prefix(parent) {
                Some(short) if is_local && short.starts_with('.') => short.to_string(),
                _ if is_local => continue,
                _ => name.clone(),
            };
            let detail = match (&symbol.kind, &symbol.data) {
                (SymbolKind::Data, Some(data)) => {
                    format!("{} in {}", data.directive, symbol.section)
                }
                (SymbolKind::Constant, _) => "constant".to_string(),
                _ => format!("label in {}", symbol.section),
            };
            items.push(Completion { label, kind: CompletionKind::Label, detail });
        }
        for name in program.externs.keys() {
            items.push(Completion {
                label: name.clone(),
                kind: CompletionKind::Label,
                detail: "extern".to_string(),
            });
        }
        items
    }

    /// Markdown describing what is under the cursor, and the span it covers.
    pub fn hover(&self, line: usize, column: usize) -> Option<(Span, String)> {
        let (span, word) = self.word_at(line, column)?;
        let lower = word.to_ascii_lowercase();
        let program = Program::build(&self.lines);

        if let Some(StmtKind::Instruction(ins)) = self.lines[line].stmt.as_ref().map(|s| &s.kind)
            && ins.mnemonic_span == span
        {
            return Some((span, self.instruction(&program, line, &lower)?));
        }
        if let Some(reg) = Reg::parse(&lower) {
            return Some((span, register(reg)));
        }
        if word.as_bytes()[0].is_ascii_digit() {
            return Some((span, number(parse_number(word)?)));
        }
        if is_directive(&lower) || is_data_directive(&lower) {
            return None;
        }

        let name = self.qualify(line, word);
        if let Some(symbol) = program.symbols.get(&name) {
            let what = match symbol.kind {
                SymbolKind::Constant => "constant".to_string(),
                SymbolKind::Data => match &symbol.data {
                    Some(data) => match data.size {
                        Some(size) => format!(
                            "data in `{}`: `{}`, {size} byte{}",
                            symbol.section,
                            data.directive,
                            if size == 1 { "" } else { "s" }
                        ),
                        None => format!("data in `{}`: `{}`", symbol.section, data.directive),
                    },
                    None => format!("data in `{}`", symbol.section),
                },
                SymbolKind::Code => match program.functions.iter().find(|f| f.name == name) {
                    Some(function) => {
                        let mut facts = vec!["function".to_string()];
                        if !function.clobbers.is_empty() {
                            facts.push(format!("changes {}", registers(function.clobbers)));
                        }
                        if !function.saved.is_empty() {
                            facts.push(format!("preserves {}", registers(function.saved)));
                        }
                        facts.join(" · ")
                    }
                    None => format!("label in `{}`", symbol.section),
                },
            };
            let definition = self.text[symbol.span.line].trim();
            let uses = program.references.get(&name).copied().unwrap_or(0);
            return Some((
                span,
                format!(
                    "**{name}** — {what}\n\n```nasm\n{definition}\n```\n\nline {}, used {uses} time{}",
                    symbol.span.line + 1,
                    if uses == 1 { "" } else { "s" }
                ),
            ));
        }
        program
            .externs
            .contains_key(&name)
            .then(|| (span, format!("**{name}** — external symbol, defined in another file")))
    }

    fn instruction(&self, program: &Program, line: usize, mnemonic: &str) -> Option<String> {
        let mut text = format!("**{mnemonic}** — {}", docs::instruction(mnemonic)?);
        let Some(index) = program.insns.iter().position(|insn| insn.line == line) else {
            return Some(text);
        };
        let fx = &program.insns[index].fx;

        let mut facts = Vec::new();
        let mut reads = fx.reads;
        reads.remove(RegId::RSP);
        let mut writes = fx.writes;
        writes.remove(RegId::RSP);
        if !reads.is_empty() {
            facts.push(format!("reads {}", registers(reads)));
        }
        if !writes.is_empty() {
            facts.push(format!("writes {}", registers(writes)));
        }
        if !fx.flags_read.is_empty() {
            facts.push(format!("tests {}", fx.flags_read.names().join(", ")));
        }
        facts.push(match fx.flags_written {
            Flags::NONE => "flags unchanged".to_string(),
            Flags::ALL => "sets all flags".to_string(),
            some => format!("sets {}", some.names().join(", ")),
        });
        text.push_str(&format!("\n\n{}", facts.join(" · ")));

        if fx.flow == Flow::Syscall {
            text.push_str(&format!("\n\ndestroys {}", registers(abi::SYSCALL_CLOBBERS)));
            if let Some(number) = program.constant_before(index, RegId::RAX) {
                match syscalls::lookup(number) {
                    Some(syscall) => {
                        text.push_str(&format!(
                            "\n\nsystem call {number}: `{}`",
                            syscall.signature()
                        ));
                        for (position, reg) in syscall.registers().iter().enumerate() {
                            text.push_str(&format!("\n- `{reg}` = {}", syscall.param(position)));
                        }
                    }
                    None => text.push_str(&format!("\n\nsystem call {number}")),
                }
            }
        }
        Some(text)
    }
}

/// How the source sets a register before the instruction at `index`:
/// the constant, the symbol or the expression it is loaded from.
fn argument_text(program: &Program, index: usize, reg: RegId) -> Option<String> {
    let write = &program.insns[program.last_write_before(index, reg)?];
    if write.fx.zeroing {
        return Some("0".to_string());
    }
    let source = write.ins.operands.get(1)?;
    let text = program.snippet(source.span).trim();
    match write.ins.mnemonic.as_str() {
        "mov" | "movzx" | "movsx" | "movsxd" => Some(text.to_string()),
        // `lea rsi, [rel msg]` passes `msg`.
        "lea" => {
            let inner = text.trim_start_matches('[').trim_end_matches(']').trim();
            Some(inner.strip_prefix("rel ").unwrap_or(inner).trim().to_string())
        }
        _ => None,
    }
}

/// The condition a conditional jump tests, written out: `rax < 5`.
fn condition_text(program: &Program, index: usize) -> Option<String> {
    let insn = &program.insns[index];
    let ("j", code) = crate::mnemonic::condition(&insn.ins.mnemonic)? else { return None };
    // The instruction that set the flags, in the same straight line.
    let block = &program.blocks[insn.block];
    let setter = program.insns[block.start..index]
        .iter()
        .rev()
        .take_while(|earlier| earlier.fx.known && earlier.fx.flow == Flow::Next)
        .find(|earlier| !earlier.fx.flags_written.is_empty())?;
    let operand = |i: usize| setter.ins.operands.get(i).map(|op| program.snippet(op.span).trim());
    let (a, b) = (operand(0)?, operand(1));

    let unsigned =
        matches!(code, "b" | "c" | "nae" | "be" | "na" | "a" | "nbe" | "ae" | "nb" | "nc");
    let relation = match code {
        "e" | "z" => "==",
        "ne" | "nz" => "!=",
        "l" | "nge" | "b" | "c" | "nae" => "<",
        "le" | "ng" | "be" | "na" => "<=",
        "g" | "nle" | "a" | "nbe" => ">",
        "ge" | "nl" | "ae" | "nb" | "nc" => ">=",
        "s" => "< 0",
        "ns" => ">= 0",
        _ => return None,
    };
    let sign_test = matches!(code, "s" | "ns");
    let zero_test = matches!(code, "e" | "z" | "ne" | "nz");
    let arithmetic = matches!(
        setter.ins.mnemonic.as_str(),
        "add" | "sub" | "and" | "or" | "xor" | "inc" | "dec" | "neg" | "shl" | "shr" | "sar"
    );
    let text = match (setter.ins.mnemonic.as_str(), b) {
        ("cmp", Some(b)) if !sign_test => format!("{a} {relation} {b}"),
        ("test", Some(b)) if a == b && sign_test => format!("{a} {relation}"),
        ("test", Some(b)) if a == b && zero_test => format!("{a} {relation} 0"),
        ("test", Some(b)) if zero_test => format!("({a} & {b}) {relation} 0"),
        // After arithmetic, the flags describe the result.
        _ if arithmetic && sign_test => format!("{a} {relation}"),
        _ if arithmetic && zero_test => format!("{a} {relation} 0"),
        _ => return None,
    };
    Some(if unsigned { format!("{text} (unsigned)") } else { text })
}

/// `rax`, `rdx`, in backticks.
fn registers(set: RegSet) -> String {
    set.iter().map(|id| format!("`{id}`")).collect::<Vec<_>>().join(", ")
}

fn register(reg: Reg) -> String {
    let id = reg.id;
    let parts: Vec<String> = [Width::W64, Width::W32, Width::W16, Width::Low8, Width::High8]
        .into_iter()
        .filter_map(|width| Some(format!("`{}` ({})", id.name(width)?, width.bits())))
        .collect();
    let mut text = format!("**{reg}** — {}\n\n{}", docs::register(id), parts.join(" · "));
    match reg.width {
        Width::W32 => {
            text.push_str("\n\nwriting it clears the upper 32 bits of the 64-bit register")
        }
        Width::W16 | Width::Low8 | Width::High8 => {
            text.push_str("\n\nwriting it leaves the other bits of the 64-bit register unchanged");
        }
        Width::W64 => {}
    }
    text
}

/// The same value in every base an assembly programmer reads.
fn number(value: i64) -> String {
    let mut text = format!("`{value}` = `{value:#x}` = `{value:#b}`");
    if let Ok(byte) = u8::try_from(value)
        && (byte.is_ascii_graphic() || byte == b' ')
    {
        text.push_str(&format!(" = `'{}'`", byte as char));
    }
    if value < 0 {
        text.push_str(&format!("\n\nas unsigned 64-bit: `{}`", value as u64));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "\
global _start
section .text
_start:
    mov eax, 1
    mov edi, 1
    mov rsi, message
    mov edx, length
    syscall
.loop:
    call helper
    dec rbx
    jnz .loop
    mov eax, 60
    syscall
helper:
.loop:
    jmp .loop

section .data
message db 'hi', 10 ; message for the user
length equ $ - message
";

    fn doc() -> Document {
        Document::new(SOURCE)
    }

    #[test]
    fn hovers() {
        let doc = doc();
        let (span, text) = doc.hover(3, 5).unwrap();
        assert_eq!(span, Span::new(3, 4, 7));
        assert!(text.starts_with("**mov** — copy") && text.contains("writes `rax`"));
        assert!(text.contains("flags unchanged"));

        let (_, text) = doc.hover(7, 6).unwrap();
        assert!(
            text.contains("system call 1: `write(fd, buf, count)`") && text.contains("`rsi` = buf")
        );

        let (_, text) = doc.hover(3, 9).unwrap();
        assert!(text.starts_with("**eax**") && text.contains("clears the upper 32 bits"));

        let (_, text) = doc.hover(12, 14).unwrap();
        assert_eq!(text, "`60` = `0x3c` = `0b111100` = `'<'`");

        let (_, text) = doc.hover(5, 15).unwrap();
        assert!(text.contains("data in `.data`: `db`, 3 bytes") && text.contains("used 2 times"));

        let (_, text) = doc.hover(9, 10).unwrap();
        assert!(text.starts_with("**helper** — function"));

        assert_eq!(doc.hover(19, 30), None); // inside a comment
        assert_eq!(doc.hover(1, 2), None); // a directive
    }

    #[test]
    fn local_labels_resolve_in_their_scope() {
        let doc = doc();
        assert_eq!(doc.definition(11, 9), Some(Span::new(8, 0, 5)));
        assert_eq!(doc.definition(16, 9), Some(Span::new(15, 0, 5)));
        assert_eq!(doc.references(11, 9), [Span::new(8, 0, 5), Span::new(11, 8, 13)]);
        assert_eq!(doc.definition(3, 9), None); // a register
    }

    #[test]
    fn renames() {
        let doc = doc();
        let edits = doc.rename(5, 15, "greeting").unwrap();
        let lines: Vec<usize> = edits.iter().map(|(span, _)| span.line).collect();
        assert_eq!(lines, [5, 19, 20]); // not the one in the comment
        assert!(edits.iter().all(|(_, text)| text == "greeting"));
        assert_eq!(doc.rename(11, 9, "again").unwrap()[0].1, ".again");
        assert_eq!(doc.rename(5, 15, "rax"), None);
        assert_eq!(doc.rename(5, 15, "two words"), None);
    }

    #[test]
    fn outlines() {
        let outline = doc().outline();
        let names: Vec<(&str, SymbolClass)> =
            outline.iter().map(|entry| (entry.name.as_str(), entry.class)).collect();
        assert_eq!(
            names,
            [
                ("_start", SymbolClass::Function),
                ("_start.loop", SymbolClass::Label),
                ("helper", SymbolClass::Function),
                ("helper.loop", SymbolClass::Label),
                ("message", SymbolClass::Data),
                ("length", SymbolClass::Constant),
            ]
        );
    }

    #[test]
    fn hints_name_the_system_calls() {
        assert_eq!(
            doc().inlay_hints(),
            [
                (7, 11, "write(fd=1, buf=message, count=length)".to_string()),
                (11, 13, "if rbx != 0".to_string()),
                (13, 11, "exit(status)".to_string()),
            ]
        );
    }

    #[test]
    fn hints_spell_out_conditions_and_characters() {
        let hints = |source: &str| -> Vec<String> {
            Document::new(source).inlay_hints().into_iter().map(|(_, _, text)| text).collect()
        };
        assert_eq!(hints("f:\n cmp rax, 5\n jl f\n"), ["if rax < 5"]);
        assert_eq!(hints("f:\n cmp rdi, rsi\n mov rax, 1\n jae f\n"), ["if rdi >= rsi (unsigned)"]);
        assert_eq!(hints("f:\n test rax, rax\n js f\n"), ["if rax < 0"]);
        assert_eq!(hints("f:\n test al, 1\n jz f\n"), ["if (al & 1) == 0"]);
        assert_eq!(hints("f:\n cmp byte [rsi], 48\n je f\n"), ["'0'", "if byte [rsi] == 48"]);
        assert_eq!(
            hints("f:\n lea rsi, [rel msg]\n xor edi, edi\n mov eax, 1\n syscall\n"),
            ["write(fd=0, buf=msg, count)"]
        );
        // The flags come from before a label: no claim is made.
        assert!(hints("f:\n cmp rax, 5\ng:\n jl f\n").is_empty());
        assert!(hints("f:\n cmp al, 'a'\n").is_empty());
    }

    #[test]
    fn completes() {
        let doc = Document::new("main:\n.again:\n    mo\n    jmp \nother:\n.inner:\n");
        let labels = |line, column| -> Vec<String> {
            doc.completions(line, column).into_iter().map(|item| item.label).collect()
        };
        let instructions = labels(2, 6);
        assert!(
            instructions.contains(&"mov".to_string()) && !instructions.contains(&"rax".to_string())
        );
        let operands = labels(3, 8);
        assert!(operands.contains(&".again".to_string()) && operands.contains(&"rax".to_string()));
        assert!(
            operands.contains(&"other".to_string()) && !operands.contains(&".inner".to_string())
        );
        assert!(!operands.contains(&"mov".to_string()));
    }

    #[test]
    fn out_of_range_positions_are_harmless() {
        let doc = doc();
        assert_eq!(doc.hover(999, 0), None);
        assert_eq!(doc.hover(3, 999), None);
        assert!(doc.references(999, 0).is_empty());
        assert!(doc.completions(999, 0).is_empty());
        assert!(Document::new("").outline().is_empty());
    }
}
