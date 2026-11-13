//! Program structure recovered from the parsed lines: symbols, the
//! instruction stream, basic blocks, functions and per-function summaries.
//!
//! Assembly has no syntax for functions, so they are inferred: a label is a
//! function entry when it is exported with `global`, is the target of a
//! `call`, or is a well-known entry point. Every other label is an ordinary
//! jump target inside whichever function reaches it.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::abi;
use crate::ast::{Directive, Instruction, Line, StmtKind};
use crate::insn::{Effects, Flow, Target, effects};
use crate::reg::{RegId, RegSet, Width};
use crate::span::Span;

/// An instruction in an executable section.
#[derive(Debug)]
pub struct Insn<'a> {
    pub line: usize,
    pub ins: &'a Instruction,
    /// Span of the whole statement.
    pub span: Span,
    pub fx: Effects,
    /// Labels attached to this instruction.
    pub labels: Vec<String>,
    /// Instructions in the same run are contiguous in memory: control can
    /// fall from one to the next. A section change or inline data starts a
    /// new run.
    pub run: usize,
    pub block: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Code,
    Data,
    /// Defined with `equ`.
    Constant,
}

/// The data definition a label names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDef {
    /// `db`, `dq`, `resb`, ...
    pub directive: String,
    /// Bytes occupied, when known.
    pub size: Option<u64>,
}

#[derive(Debug)]
pub struct Symbol {
    pub span: Span,
    pub kind: SymbolKind,
    /// For code labels, the instruction the label points at.
    pub insn: Option<usize>,
    /// The section the symbol is defined in, e.g. `.rodata`.
    pub section: String,
    /// For data labels, what is defined there.
    pub data: Option<DataDef>,
}

/// A straight-line sequence of instructions, entered only at the top.
#[derive(Debug, Default)]
pub struct Block {
    /// Index of the first instruction.
    pub start: usize,
    /// Index one past the last instruction.
    pub end: usize,
    pub succs: Vec<usize>,
    /// The block reached by running past the last instruction, if any.
    pub fallthrough: Option<usize>,
    /// Control runs past the last instruction and there is nothing after it.
    pub falls_off: bool,
    /// Reachable from some function entry.
    pub reachable: bool,
    /// The function this block is the entry of.
    pub function: Option<usize>,
}

#[derive(Debug, Default)]
pub struct Function {
    pub name: String,
    /// Entry block.
    pub entry: usize,
    /// Blocks reachable from the entry without entering another function.
    pub blocks: Vec<usize>,
    pub global: bool,
    /// The program entry point (`_start`), which is never returned from.
    pub entry_point: bool,
    /// `(block, function)`: the block runs off its end into another function.
    pub falls_into: Vec<(usize, usize)>,
    /// Local functions called or tail-jumped to.
    pub callees: Vec<usize>,
    /// Registers this function writes itself.
    pub writes: RegSet,
    /// Registers pushed and popped back, hence preserved for the caller.
    pub saved: RegSet,
    /// Registers written by this function or its callees and not restored.
    pub clobbers: RegSet,
    /// Calls code outside this file, directly or through a callee.
    pub calls_external: bool,
    /// Executes `syscall`, directly or through a callee.
    pub makes_syscall: bool,
}

impl Function {
    /// Registers left holding meaningless values when the function returns:
    /// destroyed as a side effect of `syscall` or of an external call, as
    /// opposed to registers the function sets on purpose, which may be
    /// results under a custom calling convention.
    pub fn garbage(&self) -> RegSet {
        let mut set = RegSet::EMPTY;
        if self.calls_external {
            set = abi::CALLER_SAVED;
            set.remove(RegId::RAX);
        }
        if self.makes_syscall {
            set = set.union(abi::SYSCALL_CLOBBERS);
        }
        set.difference(self.clobbers).difference(self.saved)
    }
}

/// What a `call` resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Callee {
    /// Index into [`Program::functions`].
    Local(usize),
    /// An `extern` symbol or an indirect call.
    External,
}

#[derive(Debug)]
pub struct Program<'a> {
    pub lines: &'a [Line],
    pub insns: Vec<Insn<'a>>,
    pub blocks: Vec<Block>,
    pub functions: Vec<Function>,
    pub symbols: HashMap<String, Symbol>,
    /// Redefinitions: `(name, span, line of the first definition)`.
    pub duplicates: Vec<(String, Span, usize)>,
    pub globals: HashMap<String, Span>,
    pub externs: HashSet<String>,
    /// Names introduced by `%define` and friends.
    pub defines: HashSet<String>,
    /// How many times each symbol is referenced.
    pub references: HashMap<String, usize>,
    /// Operating mode set by `bits`, 64 by default.
    pub bits: u32,
    /// The file uses macros, includes or structures, so the symbol table
    /// built here may be incomplete.
    pub opaque: bool,
    /// Every `section` directive: its name and where it is.
    pub sections: Vec<(String, Span)>,
    /// Instructions found in a data section: their span and the section.
    pub stray_code: Vec<(Span, String)>,
    /// Initialised data found in `.bss`.
    pub bss_data: Vec<Span>,
}

impl<'a> Program<'a> {
    pub fn build(lines: &'a [Line]) -> Self {
        let mut program = Program {
            lines,
            insns: Vec::new(),
            blocks: Vec::new(),
            functions: Vec::new(),
            symbols: HashMap::new(),
            duplicates: Vec::new(),
            globals: HashMap::new(),
            externs: HashSet::new(),
            defines: HashSet::new(),
            references: HashMap::new(),
            bits: 64,
            opaque: false,
            sections: Vec::new(),
            stray_code: Vec::new(),
            bss_data: Vec::new(),
        };
        program.collect();
        program.split_blocks();
        program.find_functions();
        program.summarise();
        program
    }

    /// The function a `call` target resolves to.
    pub fn callee(&self, target: &Target) -> Callee {
        let Target::Label(name) = target else { return Callee::External };
        self.symbols
            .get(name)
            .and_then(|symbol| symbol.insn)
            .and_then(|index| self.blocks[self.insns[index].block].function)
            .map_or(Callee::External, Callee::Local)
    }

    /// Whether the instruction at `index` never hands control back: an
    /// `exit` system call or a call to a function such as `exit`.
    pub fn is_noreturn(&self, index: usize) -> bool {
        match &self.insns[index].fx.flow {
            Flow::Syscall => self
                .constant_before(index, RegId::RAX)
                .is_some_and(|number| abi::EXIT_SYSCALLS.contains(&number)),
            Flow::Call(Target::Label(name)) => abi::NORETURN_FUNCTIONS.contains(&name.as_str()),
            _ => false,
        }
    }

    /// The constant held by `reg` just before the instruction at `index`,
    /// when a `mov reg, imm` or a zero idiom in the same straight-line
    /// sequence makes it obvious.
    pub fn constant_before(&self, index: usize, reg: RegId) -> Option<i64> {
        let write = &self.insns[self.last_write_before(index, reg)?];
        if write.fx.zeroing {
            Some(0)
        } else if write.ins.mnemonic == "mov" && write.fx.full_writes.contains(reg) {
            write.ins.operands.get(1)?.constant()
        } else {
            None
        }
    }

    /// The instruction that last wrote `reg` before the one at `index`,
    /// when it is in the same straight-line sequence: no label, jump, call
    /// or unknown instruction in between.
    pub fn last_write_before(&self, index: usize, reg: RegId) -> Option<usize> {
        let mut i = index;
        loop {
            if i == 0 || !self.insns[i].labels.is_empty() {
                return None;
            }
            let prev = &self.insns[i - 1];
            if prev.run != self.insns[i].run || prev.fx.flow != Flow::Next || !prev.fx.known {
                return None;
            }
            if prev.fx.writes.contains(reg) {
                return Some(i - 1);
            }
            i -= 1;
        }
    }

    /// The functions that contain `block`.
    pub fn functions_of(&self, block: usize) -> impl Iterator<Item = &Function> {
        self.functions.iter().filter(move |function| function.blocks.binary_search(&block).is_ok())
    }

    fn collect(&mut self) {
        let mut in_text = true;
        let mut section = String::from(".text");
        // A label alone on its line, waiting for the statement it names.
        let mut bare: Option<String> = None;
        let mut run = 0;
        let mut break_run = false;
        let mut pending: Vec<String> = Vec::new();

        for line in self.lines {
            let stmt = line.stmt.as_ref();
            if let Some(label) = &line.label {
                let kind = match stmt.map(|stmt| &stmt.kind) {
                    Some(StmtKind::Equ { .. }) => SymbolKind::Constant,
                    Some(StmtKind::Data { .. }) => SymbolKind::Data,
                    _ if in_text => SymbolKind::Code,
                    _ => SymbolKind::Data,
                };
                if let Some(first) = self.symbols.get(&label.name) {
                    self.duplicates.push((label.name.clone(), label.span, first.span.line));
                } else {
                    let symbol = Symbol {
                        span: label.span,
                        kind,
                        insn: None,
                        section: section.clone(),
                        data: None,
                    };
                    self.symbols.insert(label.name.clone(), symbol);
                    if kind == SymbolKind::Code {
                        pending.push(label.name.clone());
                    }
                    bare = Some(label.name.clone());
                }
            }
            let Some(stmt) = stmt else { continue };
            // `msg:` on one line and `db ...` on the next still name data.
            if let (StmtKind::Data { directive, size, .. }, Some(name)) = (&stmt.kind, bare.take())
                && let Some(symbol) = self.symbols.get_mut(&name)
            {
                symbol.kind = SymbolKind::Data;
                symbol.data = Some(DataDef { directive: directive.clone(), size: *size });
                pending.retain(|pending| *pending != name);
            }
            match &stmt.kind {
                StmtKind::Instruction(ins) => {
                    for symbol in ins.operands.iter().flat_map(|operand| operand.symbols()) {
                        *self.references.entry(symbol.clone()).or_default() += 1;
                    }
                    if !in_text {
                        self.stray_code.push((stmt.span, section.clone()));
                        continue;
                    }
                    if std::mem::take(&mut break_run) {
                        run += 1;
                    }
                    let index = self.insns.len();
                    for name in &pending {
                        if let Some(symbol) = self.symbols.get_mut(name) {
                            symbol.insn = Some(index);
                        }
                    }
                    self.insns.push(Insn {
                        line: line.number,
                        ins,
                        span: stmt.span,
                        fx: effects(ins),
                        labels: std::mem::take(&mut pending),
                        run,
                        block: 0,
                    });
                }
                StmtKind::Data { symbols, directive, .. } => {
                    for symbol in symbols {
                        *self.references.entry(symbol.clone()).or_default() += 1;
                    }
                    if section.starts_with(".bss") && !directive.starts_with("res") {
                        self.bss_data.push(stmt.span);
                    }
                    if in_text {
                        break_run = true;
                        pending.clear();
                    }
                }
                StmtKind::Equ { symbols, .. } => {
                    for symbol in symbols {
                        *self.references.entry(symbol.clone()).or_default() += 1;
                    }
                }
                StmtKind::Directive(directive) => match directive {
                    Directive::Section(name) => {
                        section = name.clone();
                        self.sections.push((name.clone(), stmt.span));
                        in_text = !is_data_section(name);
                        break_run = true;
                        pending.clear();
                    }
                    Directive::Global(names) => {
                        for name in names {
                            self.globals.entry(name.clone()).or_insert(stmt.span);
                        }
                    }
                    Directive::Extern(names) => self.externs.extend(names.iter().cloned()),
                    Directive::Bits(bits) => self.bits = *bits,
                    Directive::Other(name) => {
                        if matches!(name.as_str(), "struc" | "istruc" | "absolute") {
                            self.opaque = true;
                        }
                    }
                },
                StmtKind::Preproc { name, .. } => {
                    // A macro can expand to anything, including labels.
                    self.opaque = true;
                    self.defines.extend(name.iter().cloned());
                }
            }
        }
    }

    fn ends_block(&self, index: usize) -> bool {
        matches!(
            self.insns[index].fx.flow,
            Flow::Jump(_) | Flow::CondJump(_) | Flow::Ret | Flow::Halt
        ) || self.is_noreturn(index)
    }

    fn split_blocks(&mut self) {
        let count = self.insns.len();
        let mut leaders = BTreeSet::new();
        for i in 0..count {
            let starts_run = i == 0 || self.insns[i].run != self.insns[i - 1].run;
            if starts_run || !self.insns[i].labels.is_empty() {
                leaders.insert(i);
            }
            if self.ends_block(i) && i + 1 < count {
                leaders.insert(i + 1);
            }
        }
        let starts: Vec<usize> = leaders.into_iter().collect();
        for (block, &start) in starts.iter().enumerate() {
            let end = starts.get(block + 1).copied().unwrap_or(count);
            self.insns[start..end].iter_mut().for_each(|insn| insn.block = block);
            self.blocks.push(Block { start, end, ..Block::default() });
        }

        for block in 0..self.blocks.len() {
            let last = self.blocks[block].end - 1;
            let next = (last + 1 < count && self.insns[last + 1].run == self.insns[last].run)
                .then(|| self.insns[last + 1].block);
            let flow = &self.insns[last].fx.flow;
            let jump = match flow {
                Flow::Jump(Target::Label(name)) | Flow::CondJump(Target::Label(name)) => self
                    .symbols
                    .get(name)
                    .and_then(|symbol| symbol.insn)
                    .map(|index| self.insns[index].block),
                _ => None,
            };
            let continues =
                !matches!(flow, Flow::Jump(_) | Flow::Ret | Flow::Halt) && !self.is_noreturn(last);

            let entry = &mut self.blocks[block];
            entry.succs.extend(jump);
            if continues {
                match next {
                    Some(next) => {
                        entry.fallthrough = Some(next);
                        if !entry.succs.contains(&next) {
                            entry.succs.push(next);
                        }
                    }
                    None => entry.falls_off = true,
                }
            }
        }
    }

    fn find_functions(&mut self) {
        // Entry block -> function name. Exported names win over call targets.
        let mut entries: BTreeMap<usize, String> = BTreeMap::new();
        let mut named: Vec<(&String, usize)> = self
            .symbols
            .iter()
            .filter(|(name, _)| {
                self.globals.contains_key(*name) || abi::is_entry_point(name) || *name == "main"
            })
            .filter_map(|(name, symbol)| Some((name, self.insns[symbol.insn?].block)))
            .collect();
        named.sort();
        for (name, block) in named {
            entries.entry(block).or_insert_with(|| name.clone());
        }
        for insn in &self.insns {
            if let Flow::Call(Target::Label(name)) = &insn.fx.flow
                && let Some(index) = self.symbols.get(name).and_then(|symbol| symbol.insn)
            {
                entries.entry(self.insns[index].block).or_insert_with(|| name.clone());
            }
        }
        for (index, block) in self.blocks.iter().enumerate() {
            let first = &self.insns[block.start];
            if block.start == 0 || self.insns[block.start - 1].run != first.run {
                entries
                    .entry(index)
                    .or_insert_with(|| first.labels.first().cloned().unwrap_or("<code>".into()));
            }
        }

        for (index, (entry, name)) in entries.into_iter().enumerate() {
            self.blocks[entry].function = Some(index);
            self.functions.push(Function {
                global: self.globals.contains_key(&name),
                entry_point: abi::is_entry_point(&name),
                name,
                entry,
                ..Function::default()
            });
        }

        for index in 0..self.functions.len() {
            let entry = self.functions[index].entry;
            let mut seen = HashSet::from([entry]);
            let mut stack = vec![entry];
            let mut members = Vec::new();
            while let Some(block) = stack.pop() {
                members.push(block);
                self.blocks[block].reachable = true;
                for &succ in &self.blocks[block].succs {
                    match self.blocks[succ].function {
                        Some(other) if succ != entry => {
                            let function = &mut self.functions[index];
                            if self.blocks[block].fallthrough == Some(succ) {
                                function.falls_into.push((block, other));
                            } else if !function.callees.contains(&other) {
                                function.callees.push(other); // tail jump
                            }
                        }
                        _ => {
                            if seen.insert(succ) {
                                stack.push(succ);
                            }
                        }
                    }
                }
            }
            members.sort_unstable();
            self.functions[index].blocks = members;
        }
    }

    fn summarise(&mut self) {
        for index in 0..self.functions.len() {
            let mut writes = RegSet::EMPTY;
            let (mut pushed, mut popped) = (RegSet::EMPTY, RegSet::EMPTY);
            let mut callees = Vec::new();
            let (mut external, mut syscall) = (false, false);
            for &block in &self.functions[index].blocks {
                for insn in &self.insns[self.blocks[block].start..self.blocks[block].end] {
                    writes = writes.union(insn.fx.writes);
                    let reg64 = insn
                        .ins
                        .operands
                        .first()
                        .and_then(|operand| operand.as_reg())
                        .filter(|reg| reg.width == Width::W64);
                    match (insn.ins.mnemonic.as_str(), reg64) {
                        ("push", Some(reg)) => pushed.insert(reg.id),
                        ("pop", Some(reg)) => popped.insert(reg.id),
                        ("leave", _) => popped.insert(RegId::RBP),
                        _ => {}
                    }
                    match &insn.fx.flow {
                        Flow::Syscall | Flow::Int(_) => syscall = true,
                        Flow::Call(target) => match self.callee(target) {
                            Callee::Local(callee) => callees.push(callee),
                            Callee::External => external = true,
                        },
                        _ => {}
                    }
                }
            }
            writes.remove(RegId::RSP);
            let function = &mut self.functions[index];
            function.writes = writes;
            function.saved = pushed.intersection(popped);
            function.clobbers = writes.difference(function.saved);
            function.calls_external = external;
            function.makes_syscall = syscall;
            for callee in callees {
                if !function.callees.contains(&callee) {
                    function.callees.push(callee);
                }
            }
        }

        // Propagate through the call graph until nothing changes.
        let mut changed = true;
        while changed {
            changed = false;
            for index in 0..self.functions.len() {
                let function = &self.functions[index];
                let mut clobbers = function.clobbers;
                let mut external = function.calls_external;
                let mut syscall = function.makes_syscall;
                for &callee in &function.callees {
                    let callee = &self.functions[callee];
                    clobbers = clobbers.union(callee.clobbers.difference(function.saved));
                    external |= callee.calls_external;
                    syscall |= callee.makes_syscall;
                }
                if (clobbers, external, syscall)
                    != (function.clobbers, function.calls_external, function.makes_syscall)
                {
                    let function = &mut self.functions[index];
                    function.clobbers = clobbers;
                    function.calls_external = external;
                    function.makes_syscall = syscall;
                    changed = true;
                }
            }
        }
    }
}

fn is_data_section(name: &str) -> bool {
    [".data", ".bss", ".rodata", ".rdata", ".tdata", ".tbss", ".note", ".comment", ".debug"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    const HELLO: &str = "\
global _start
section .text
_start:
    mov rcx, 3
again:
    call print
    dec rcx
    jnz again
    mov rax, 60
    xor edi, edi
    syscall

print:
    push rbx
    mov rbx, 1
    mov rax, 1
    mov rdi, 1
    mov rsi, msg
    mov rdx, 3
    syscall
    pop rbx
    ret

section .data
msg db 'hi', 10
";

    #[test]
    fn finds_functions_and_blocks() {
        let lines = parse(HELLO);
        let program = Program::build(&lines);
        let names: Vec<&str> = program.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["_start", "print"]);
        assert!(program.functions[0].entry_point && program.functions[0].global);
        // _start: [mov] [call, dec, jnz] [mov, xor, syscall]; print: one block.
        assert_eq!(program.blocks.len(), 4);
        assert_eq!(program.functions[0].blocks, [0, 1, 2]);
        assert_eq!(program.blocks[1].succs, [1, 2]);
        assert!(program.blocks[2].succs.is_empty() && !program.blocks[2].falls_off);
        assert!(program.blocks.iter().all(|block| block.reachable));
    }

    #[test]
    fn exit_syscall_does_not_fall_through() {
        let lines = parse(HELLO);
        let program = Program::build(&lines);
        let exit = program.blocks[2].end - 1;
        assert_eq!(program.constant_before(exit, RegId::RAX), Some(60));
        assert!(program.is_noreturn(exit));
        let write = program.insns.iter().rposition(|insn| insn.fx.flow == Flow::Syscall).unwrap();
        assert!(!program.is_noreturn(write));
    }

    #[test]
    fn summarises_functions() {
        let lines = parse(HELLO);
        let program = Program::build(&lines);
        let print = &program.functions[1];
        assert_eq!(print.saved, RegSet::of(&[RegId::RBX]));
        assert!(print.makes_syscall && !print.calls_external);
        assert!(!print.clobbers.contains(RegId::RBX) && print.clobbers.contains(RegId::RSI));
        // rcx and r11 are destroyed by the syscall, not set on purpose.
        assert_eq!(print.garbage(), abi::SYSCALL_CLOBBERS);
        let start = &program.functions[0];
        assert_eq!(start.callees, [1]);
        assert!(start.makes_syscall && start.clobbers.contains(RegId::RSI));
    }

    #[test]
    fn classifies_symbols() {
        let lines = parse(HELLO);
        let program = Program::build(&lines);
        assert_eq!(program.symbols["msg"].kind, SymbolKind::Data);
        assert_eq!(program.symbols["msg"].section, ".data");
        assert_eq!(
            program.symbols["msg"].data,
            Some(DataDef { directive: "db".into(), size: Some(3) })
        );
        assert_eq!(program.symbols["again"].kind, SymbolKind::Code);
        assert_eq!(program.references["again"], 1);
        assert_eq!(program.callee(&Target::Label("print".into())), Callee::Local(1));
        assert_eq!(program.callee(&Target::Label("printf".into())), Callee::External);
    }

    #[test]
    fn detects_falling_off_and_into() {
        let lines =
            parse("global main\nmain:\n    mov rax, 1\nhelper:\n    ret\n    call helper\n");
        let program = Program::build(&lines);
        assert_eq!(program.functions[0].falls_into, [(0, 1)]);
        // `call helper` after the `ret` is unreachable and runs off the end.
        let last = program.blocks.last().unwrap();
        assert!(!last.reachable && last.falls_off);
    }

    #[test]
    fn empty_and_data_only_sources() {
        for source in ["", "section .data\nmsg db 1\n", "; nothing\n"] {
            let lines = parse(source);
            let program = Program::build(&lines);
            assert!(program.insns.is_empty() && program.functions.is_empty());
        }
    }
}
