//! Line-oriented parser for NASM-flavoured x86-64 assembly.
//!
//! The parser never fails: anything it does not understand is kept as an
//! instruction with an unknown mnemonic, which the analyses treat
//! conservatively.

use crate::ast::{
    Directive, Expr, Instruction, Label, Line, Mem, Operand, OperandKind, Stmt, StmtKind,
};
use crate::mnemonic::{
    is_data_directive, is_directive, is_known_mnemonic, is_operand_keyword, is_prefix, size_keyword,
};
use crate::reg::{self, Reg};
use crate::span::Span;

/// Parses a whole source file into one [`Line`] per source line.
pub fn parse(source: &str) -> Vec<Line> {
    let mut parent = String::new();
    source.lines().enumerate().map(|(number, raw)| parse_line(number, raw, &mut parent)).collect()
}

/// `parent` is the most recent non-local label, used to qualify `.local` names.
fn parse_line(number: usize, raw: &str, parent: &mut String) -> Line {
    let code_end = comment_start(raw);
    let comment = raw.get(code_end + 1..).map(|text| text.trim().to_string());
    let mut line = Line { number, label: None, stmt: None, comment };

    // `[bits 64]` is the primitive form of `bits 64`: drop the brackets.
    let owned;
    let mut code = &raw[..code_end];
    if code.trim_start().starts_with('[') {
        owned = code.replace(['[', ']'], " ");
        code = &owned;
    }

    let mut pos = skip_ws(code, 0);
    if pos >= code.len() {
        return line;
    }
    if code.as_bytes()[pos] == b'%' {
        line.stmt = Some(parse_preproc(number, code, pos));
        return line;
    }

    let word_end = ident_end(code, pos);
    if word_end == pos {
        return line;
    }
    let word = &code[pos..word_end];
    let after = skip_ws(code, word_end);
    let has_colon = code.as_bytes().get(after) == Some(&b':');
    if has_colon || is_bare_label(&word.to_ascii_lowercase(), &code[after..]) {
        let special = word.starts_with("..");
        let local = word.starts_with('.') && !special;
        let name = if local { format!("{parent}{word}") } else { word.to_string() };
        if !local && !special {
            *parent = word.to_string();
        }
        line.label = Some(Label { name, span: Span::new(number, pos, word_end), local });
        pos = skip_ws(code, if has_colon { after + 1 } else { after });
    }
    if pos < code.len() {
        line.stmt = parse_statement(number, code, pos, parent);
    }
    line
}

/// Whether `word`, found at the start of a line without a colon, is a label.
/// NASM allows `msg db "hi"` and even a lone `msg`.
fn is_bare_label(word: &str, rest: &str) -> bool {
    if is_directive(word)
        || is_prefix(word)
        || is_data_directive(word)
        || matches!(word, "equ" | "times")
    {
        return false;
    }
    let next = rest[..ident_end(rest, 0)].to_ascii_lowercase();
    is_data_directive(&next)
        || matches!(next.as_str(), "equ" | "times")
        || (rest.is_empty() && !is_known_mnemonic(word))
}

fn parse_statement(number: usize, code: &str, start: usize, parent: &str) -> Option<Stmt> {
    let end = code.trim_end().len();
    let head_end = ident_end(code, start);
    if head_end == start {
        return None;
    }
    let head = code[start..head_end].to_ascii_lowercase();
    let rest_start = skip_ws(code, head_end).min(end);
    let rest = &code[rest_start..end];

    let kind = if is_data_directive(&head) {
        StmtKind::Data { symbols: symbols_in(rest, parent) }
    } else if head == "equ" {
        StmtKind::Equ { symbols: symbols_in(rest, parent) }
    } else if head == "times" {
        parse_times(number, code, rest_start, end, parent)
    } else if is_directive(&head) {
        StmtKind::Directive(parse_directive(&head, rest))
    } else if is_prefix(&head) && rest_start < end {
        parse_instruction(number, code, rest_start, end, Some(head), parent)
    } else {
        parse_instruction(number, code, start, end, None, parent)
    };
    Some(Stmt { kind, span: Span::new(number, start, end) })
}

/// `times <count> <instruction or data>`: find where the repeated item starts.
fn parse_times(number: usize, code: &str, start: usize, end: usize, parent: &str) -> StmtKind {
    for (word_start, word_end) in words(code, start, end).into_iter().skip(1) {
        let word = code[word_start..word_end].to_ascii_lowercase();
        if is_data_directive(&word) {
            break;
        }
        if is_known_mnemonic(&word) || is_prefix(&word) {
            let prefix = Some("times".to_string());
            return parse_instruction(number, code, word_start, end, prefix, parent);
        }
    }
    StmtKind::Data { symbols: symbols_in(&code[start..end], parent) }
}

fn parse_instruction(
    number: usize,
    code: &str,
    start: usize,
    end: usize,
    prefix: Option<String>,
    parent: &str,
) -> StmtKind {
    let mnemonic_end = ident_end(code, start).max(start + 1).min(end);
    let operands = split_operands(code, skip_ws(code, mnemonic_end), end)
        .into_iter()
        .map(|(s, e)| parse_operand(&code[s..e], Span::new(number, s, e), parent))
        .collect();
    StmtKind::Instruction(Instruction {
        prefix,
        mnemonic: code[start..mnemonic_end].to_ascii_lowercase(),
        mnemonic_span: Span::new(number, start, mnemonic_end),
        operands,
    })
}

fn parse_directive(head: &str, rest: &str) -> Directive {
    let names = || {
        rest.split(',')
            .filter_map(|item| item.split_whitespace().next())
            .map(|name| name.split(':').next().unwrap_or(name).to_string())
            .filter(|name| !name.is_empty())
            .collect()
    };
    match head {
        "section" | "segment" => {
            Directive::Section(rest.split_whitespace().next().unwrap_or("").to_ascii_lowercase())
        }
        "global" | "globl" => Directive::Global(names()),
        "extern" | "common" => Directive::Extern(names()),
        "bits" => Directive::Bits(rest.trim().parse().unwrap_or(64)),
        "use16" => Directive::Bits(16),
        "use32" => Directive::Bits(32),
        "use64" => Directive::Bits(64),
        _ => Directive::Other(head.to_string()),
    }
}

fn parse_preproc(number: usize, code: &str, start: usize) -> Stmt {
    let end = code.trim_end().len();
    let directive_end = ident_end(code, start + 1);
    let directive = code[start + 1..directive_end].to_ascii_lowercase();
    let name_start = skip_ws(code, directive_end);
    let name_end = ident_end(code, name_start);
    let name = (name_end > name_start).then(|| code[name_start..name_end].to_string());
    Stmt { kind: StmtKind::Preproc { directive, name }, span: Span::new(number, start, end) }
}

fn parse_operand(text: &str, span: Span, parent: &str) -> Operand {
    let tokens = tokenize(text);
    let mut size = None;
    let mut skip = 0;
    for token in &tokens {
        let Tok::Ident(word) = token else { break };
        let word = word.to_ascii_lowercase();
        if let Some(bits) = size_keyword(&word) {
            size = Some(bits);
        } else if !is_operand_keyword(&word) {
            break;
        }
        skip += 1;
    }
    let rest = &tokens[skip..];

    let mut regs = Vec::new();
    let mut symbols = Vec::new();
    collect_names(rest, parent, &mut regs, &mut symbols);

    let kind = if rest.contains(&Tok::Punct('[')) {
        OperandKind::Mem(Mem { regs, symbols })
    } else if let [Tok::Ident(name)] = rest
        && let Some(reg) = Reg::parse(name)
    {
        OperandKind::Reg(reg)
    } else if let [Tok::Ident(name)] = rest
        && reg::is_other_register(name)
    {
        OperandKind::OtherReg(name.to_ascii_lowercase())
    } else {
        let value = if symbols.is_empty() { evaluate(rest) } else { None };
        OperandKind::Imm(Expr { value, symbols })
    };
    Operand { kind, span, size }
}

/// Qualified names of the symbols referenced in a data or `equ` expression.
fn symbols_in(text: &str, parent: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    collect_names(&tokenize(text), parent, &mut Vec::new(), &mut symbols);
    symbols
}

fn collect_names(tokens: &[Tok], parent: &str, regs: &mut Vec<Reg>, symbols: &mut Vec<String>) {
    let mut after_wrt = false;
    for token in tokens {
        let Tok::Ident(name) = token else { continue };
        if std::mem::take(&mut after_wrt) {
            continue; // `wrt ..plt`, `wrt ..got`
        }
        let lower = name.to_ascii_lowercase();
        if lower == "wrt" {
            after_wrt = true;
        } else if let Some(reg) = Reg::parse(&lower) {
            regs.push(reg);
        } else if !reg::is_other_register(&lower)
            && !is_operand_keyword(&lower)
            && name != "?"
            && !name.bytes().all(|b| b == b'$')
        {
            let local = name.starts_with('.') && !name.starts_with("..");
            symbols.push(if local { format!("{parent}{name}") } else { name.clone() });
        }
    }
}

// --- lexical helpers -------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Num(Option<i64>),
    Str(Vec<u8>),
    Punct(char),
}

fn tokenize(text: &str) -> Vec<Tok> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if matches!(c, b'\'' | b'"' | b'`') {
            let body = i + 1;
            let close =
                bytes[body..].iter().position(|&b| b == c).map_or(bytes.len(), |p| body + p);
            tokens.push(Tok::Str(bytes[body..close].to_vec()));
            i = close + 1;
        } else if c.is_ascii_digit() {
            let end = ident_end(text, i);
            tokens.push(Tok::Num(parse_number(&text[i..end])));
            i = end;
        } else if is_ident_byte(c) && !matches!(c, b'~' | b'#') {
            let end = ident_end(text, i);
            tokens.push(Tok::Ident(text[i..end].to_string()));
            i = end;
        } else {
            tokens.push(Tok::Punct(c as char));
            i += 1;
        }
    }
    tokens
}

/// Parses a NASM numeric literal: `42`, `0x2a`, `2ah`, `0b101`, `101b`, `0o17`, `17q`.
fn parse_number(literal: &str) -> Option<i64> {
    let text = literal.replace('_', "").to_ascii_lowercase();
    let radix = |digits: &str, radix: u32| {
        u64::from_str_radix(digits, radix).ok().map(|value| value as i64)
    };
    let all =
        |digits: &str, radix: u32| !digits.is_empty() && digits.chars().all(|c| c.is_digit(radix));
    if let Some(digits) = text.strip_prefix("0x") {
        return radix(digits, 16);
    }
    if let Some(digits) = text.strip_suffix('h').filter(|d| all(d, 16)) {
        return radix(digits, 16);
    }
    for (prefix, base) in [("0b", 2), ("0y", 2), ("0o", 8), ("0q", 8), ("0d", 10)] {
        if let Some(digits) = text.strip_prefix(prefix).filter(|d| all(d, base)) {
            return radix(digits, base);
        }
    }
    for (suffix, base) in [('b', 2), ('y', 2), ('o', 8), ('q', 8), ('d', 10)] {
        if let Some(digits) = text.strip_suffix(suffix).filter(|d| all(d, base)) {
            return radix(digits, base);
        }
    }
    radix(&text, 10)
}

/// Evaluates a constant expression, or returns `None` if it is not one.
fn evaluate(tokens: &[Tok]) -> Option<i64> {
    let mut eval = Evaluator { tokens, pos: 0 };
    let value = eval.binary(0)?;
    (eval.pos == tokens.len()).then_some(value)
}

struct Evaluator<'a> {
    tokens: &'a [Tok],
    pos: usize,
}

impl Evaluator<'_> {
    fn punct(&self, offset: usize) -> Option<char> {
        match self.tokens.get(self.pos + offset) {
            Some(Tok::Punct(c)) => Some(*c),
            _ => None,
        }
    }

    /// The operator at the cursor: its precedence and its length in tokens.
    fn operator(&self) -> Option<(char, u8, usize)> {
        let c = self.punct(0)?;
        let doubled = self.punct(1) == Some(c);
        match c {
            '|' => Some((c, 1, 1)),
            '^' => Some((c, 2, 1)),
            '&' => Some((c, 3, 1)),
            '<' | '>' if doubled => Some((c, 4, 2)),
            '+' | '-' => Some((c, 5, 1)),
            '*' => Some((c, 6, 1)),
            '/' | '%' => Some((c, 6, if doubled { 2 } else { 1 })),
            _ => None,
        }
    }

    fn binary(&mut self, min_precedence: u8) -> Option<i64> {
        let mut left = self.unary()?;
        while let Some((op, precedence, width)) = self.operator() {
            if precedence < min_precedence {
                break;
            }
            self.pos += width;
            let right = self.binary(precedence + 1)?;
            left = match op {
                '|' => left | right,
                '^' => left ^ right,
                '&' => left & right,
                '<' => left.checked_shl(u32::try_from(right).ok()?)?,
                '>' => ((left as u64).checked_shr(u32::try_from(right).ok()?)?) as i64,
                '+' => left.wrapping_add(right),
                '-' => left.wrapping_sub(right),
                '*' => left.wrapping_mul(right),
                '/' => left.checked_div(right)?,
                _ => left.checked_rem(right)?,
            };
        }
        Some(left)
    }

    fn unary(&mut self) -> Option<i64> {
        let token = self.tokens.get(self.pos)?;
        self.pos += 1;
        match token {
            Tok::Num(value) => *value,
            Tok::Str(bytes) if (1..=8).contains(&bytes.len()) => {
                // Character constants are little-endian: 'ab' is 0x6261.
                Some(bytes.iter().rev().fold(0i64, |acc, &b| (acc << 8) | i64::from(b)))
            }
            Tok::Punct('-') => self.unary().map(i64::wrapping_neg),
            Tok::Punct('+') => self.unary(),
            Tok::Punct('~') => self.unary().map(|value| !value),
            Tok::Punct('(') => {
                let value = self.binary(0)?;
                (self.punct(0) == Some(')')).then(|| self.pos += 1)?;
                Some(value)
            }
            _ => None,
        }
    }
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(b, b'_' | b'.' | b'$' | b'#' | b'@' | b'~' | b'?')
        || b >= 0x80
}

/// End of the identifier-like word starting at `start`.
fn ident_end(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut end = start;
    while end < bytes.len() && is_ident_byte(bytes[end]) {
        end += 1;
    }
    end
}

fn skip_ws(text: &str, start: usize) -> usize {
    let bytes = text.as_bytes();
    let mut pos = start;
    while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    pos
}

/// Byte offset of the `;` that starts the comment, or the line length.
fn comment_start(line: &str) -> usize {
    let mut quote = None;
    for (i, b) in line.bytes().enumerate() {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if matches!(b, b'\'' | b'"' | b'`') => quote = Some(b),
            None if b == b';' => return i,
            None => {}
        }
    }
    line.len()
}

/// Ranges of the comma-separated operands in `code[start..end]`, trimmed.
fn split_operands(code: &str, start: usize, end: usize) -> Vec<(usize, usize)> {
    let bytes = code.as_bytes();
    let mut ranges = Vec::new();
    let mut push = |from: usize, to: usize| {
        let text = &code[from..to];
        let lead = text.len() - text.trim_start().len();
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            ranges.push((from + lead, from + lead + trimmed.len()));
        }
    };
    let (mut quote, mut depth, mut from) = (None, 0i32, start);
    for i in start..end {
        let b = bytes[i];
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None => match b {
                b'\'' | b'"' | b'`' => quote = Some(b),
                b'[' | b'(' => depth += 1,
                b']' | b')' => depth -= 1,
                b',' if depth == 0 => {
                    push(from, i);
                    from = i + 1;
                }
                _ => {}
            },
        }
    }
    push(from, end);
    ranges
}

/// Ranges of the identifier-like words in `code[start..end]`, skipping strings.
fn words(code: &str, start: usize, end: usize) -> Vec<(usize, usize)> {
    let bytes = code.as_bytes();
    let mut ranges = Vec::new();
    let mut i = start;
    while i < end {
        let b = bytes[i];
        if matches!(b, b'\'' | b'"' | b'`') {
            i = bytes[i + 1..end].iter().position(|&c| c == b).map_or(end, |p| i + p + 2);
        } else if is_ident_byte(b) {
            let word_end = ident_end(code, i).min(end);
            ranges.push((i, word_end));
            i = word_end;
        } else {
            i += 1;
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reg::{RegId, Width};

    fn line(source: &str) -> Line {
        parse(source).pop().expect("one line")
    }

    fn instruction(source: &str) -> Instruction {
        match line(source).stmt.expect("a statement").kind {
            StmtKind::Instruction(ins) => ins,
            other => panic!("expected an instruction, got {other:?}"),
        }
    }

    #[test]
    fn instruction_with_operands_and_comment() {
        let parsed = line("    mov rax, 60 ; exit");
        assert_eq!(parsed.comment.as_deref(), Some("exit"));
        let ins = instruction("    mov rax, 60 ; exit");
        assert_eq!(ins.mnemonic, "mov");
        assert_eq!(ins.mnemonic_span, Span::new(0, 4, 7));
        assert_eq!(ins.operands[0].as_reg(), Some(Reg::new(RegId::RAX, Width::W64)));
        assert_eq!(ins.operands[0].span, Span::new(0, 8, 11));
        assert_eq!(ins.operands[1].constant(), Some(60));
    }

    #[test]
    fn label_named_like_a_mnemonic() {
        let parsed = line("    loop:");
        assert_eq!(parsed.label.unwrap().name, "loop");
        assert!(parsed.stmt.is_none());
        assert_eq!(instruction("loop .again").mnemonic, "loop");
    }

    #[test]
    fn label_and_instruction_on_one_line() {
        let parsed = line("done: ret");
        assert_eq!(parsed.label.unwrap().name, "done");
        assert!(matches!(parsed.stmt.unwrap().kind, StmtKind::Instruction(_)));
    }

    #[test]
    fn bare_labels() {
        let data = line("    text db \"1337\", 10");
        assert_eq!(data.label.unwrap().name, "text");
        assert!(matches!(data.stmt.unwrap().kind, StmtKind::Data { .. }));
        assert_eq!(line("buffer resb 64").label.unwrap().name, "buffer");
        assert_eq!(line("LEN equ $ - msg").label.unwrap().name, "LEN");
        assert_eq!(line("orphan").label.unwrap().name, "orphan");
        assert!(line("ret").label.is_none());
        assert!(line("syscall").label.is_none());
    }

    #[test]
    fn local_labels_are_qualified() {
        let lines = parse("main:\n.loop:\n    jmp .loop\nother:\n    jmp .loop\n");
        assert_eq!(lines[1].label.as_ref().unwrap().name, "main.loop");
        let target = |i: usize| match &lines[i].stmt.as_ref().unwrap().kind {
            StmtKind::Instruction(ins) => ins.operands[0].symbols()[0].clone(),
            _ => unreachable!(),
        };
        assert_eq!(target(2), "main.loop");
        assert_eq!(target(4), "other.loop");
    }

    #[test]
    fn memory_operands() {
        let ins = instruction("cmp dword [buffer + r15*4 + 8], '1337'");
        let OperandKind::Mem(mem) = &ins.operands[0].kind else { panic!("expected memory") };
        assert_eq!(ins.operands[0].size, Some(32));
        assert_eq!(mem.regs, vec![Reg::new(RegId::R15, Width::W64)]);
        assert_eq!(mem.symbols, vec!["buffer".to_string()]);
        assert_eq!(ins.operands[1].constant(), Some(0x3733_3331));

        let ins = instruction("mov rax, qword ptr [rel msg wrt ..got]");
        assert_eq!(ins.operands[1].symbols(), ["msg".to_string()]);
        assert_eq!(ins.operands[1].size, Some(64));
    }

    #[test]
    fn immediates() {
        let value = |text: &str| instruction(&format!("mov rax, {text}")).operands[1].constant();
        assert_eq!(value("0x2A"), Some(42));
        assert_eq!(value("2ah"), Some(42));
        assert_eq!(value("101010b"), Some(42));
        assert_eq!(value("0b10_1010"), Some(42));
        assert_eq!(value("52o"), Some(42));
        assert_eq!(value("-(2 + 4) * 7"), Some(-42));
        assert_eq!(value("1 << 4 | 3"), Some(19));
        assert_eq!(value("'A'"), Some(65));
        assert_eq!(value("','"), Some(44));
        assert_eq!(value("number + 31"), None);
        assert_eq!(value("$ - 4"), None);
        assert_eq!(value("1.5"), None);
    }

    #[test]
    fn symbols_and_registers() {
        let ins = instruction("call printf wrt ..plt");
        assert_eq!(ins.operands[0].symbols(), ["printf".to_string()]);
        let ins = instruction("movaps xmm0, [rsp]");
        assert_eq!(ins.operands[0].kind, OperandKind::OtherReg("xmm0".to_string()));
    }

    #[test]
    fn prefixes_and_times() {
        let ins = instruction("rep movsb");
        assert_eq!((ins.prefix.as_deref(), ins.mnemonic.as_str()), (Some("rep"), "movsb"));
        let ins = instruction("times 3 nop");
        assert_eq!((ins.prefix.as_deref(), ins.mnemonic.as_str()), (Some("times"), "nop"));
        let padding = line("times 510-($-$$) db 0");
        assert!(matches!(padding.stmt.unwrap().kind, StmtKind::Data { .. }));
    }

    #[test]
    fn directives() {
        let kind = |source: &str| line(source).stmt.unwrap().kind;
        assert_eq!(
            kind("section .TEXT exec"),
            StmtKind::Directive(Directive::Section(".text".into()))
        );
        assert_eq!(
            kind("global _start, main:function"),
            StmtKind::Directive(Directive::Global(vec!["_start".into(), "main".into()]))
        );
        assert_eq!(kind("[BITS 32]"), StmtKind::Directive(Directive::Bits(32)));
        assert_eq!(
            kind("%define SYS_exit 60"),
            StmtKind::Preproc { directive: "define".into(), name: Some("SYS_exit".into()) }
        );
    }

    #[test]
    fn semicolon_inside_a_string_is_not_a_comment() {
        let parsed = line("msg db 'a;b', 0 ; real comment");
        assert_eq!(parsed.comment.as_deref(), Some("real comment"));
    }

    #[test]
    fn never_panics_on_garbage() {
        for source in
            ["", ";", ":", "[", "%", "mov [", "'", "db '", ", ,", "é: mov rax, 'é", "times"]
        {
            parse(source);
        }
    }
}
