//! Diagnostics and the lints that produce them.

use std::fmt;

use crate::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Hint,
    Warning,
    Error,
}

impl Severity {
    pub const fn label(self) -> &'static str {
        match self {
            Severity::Hint => "hint",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Lint groups, mirroring clippy's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// Code the assembler rejects, or that is wrong on every execution.
    Correctness,
    /// Code that is most likely a bug.
    Suspicious,
    /// Code that works but can be written in a shorter or clearer way.
    Style,
    /// Code that works but can be made faster or smaller.
    Performance,
    /// Stricter suggestions, off unless requested.
    Pedantic,
}

impl Group {
    pub const fn name(self) -> &'static str {
        match self {
            Group::Correctness => "correctness",
            Group::Suspicious => "suspicious",
            Group::Style => "style",
            Group::Performance => "performance",
            Group::Pedantic => "pedantic",
        }
    }

    pub const fn severity(self) -> Severity {
        match self {
            Group::Correctness => Severity::Error,
            Group::Suspicious => Severity::Warning,
            Group::Style | Group::Performance | Group::Pedantic => Severity::Hint,
        }
    }
}

/// A check the tool performs. Lints are declared as statics next to the
/// code that implements them.
#[derive(Debug)]
pub struct Lint {
    /// Kebab-case identifier, used in output and in `allow(...)` comments.
    pub name: &'static str,
    pub group: Group,
    /// One-line description for `asm-check lints`.
    pub summary: &'static str,
}

/// Extra information attached to a diagnostic, optionally pointing at
/// another place in the file.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub span: Option<Span>,
    pub message: String,
}

/// A text edit. Inserted and replacement text may span several lines;
/// every line after the first takes the indentation of the line it lands on.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    Replace {
        span: Span,
        text: String,
    },
    DeleteLine(usize),
    /// New line(s) above the given line.
    InsertBefore {
        line: usize,
        text: String,
    },
    /// New line(s) below the given line.
    InsertAfter {
        line: usize,
        text: String,
    },
}

/// A change that resolves a diagnostic.
#[derive(Debug, Clone, PartialEq)]
pub struct Fix {
    /// What the change does, in the imperative: "use `xor edi, edi`".
    pub title: String,
    pub edits: Vec<Edit>,
    /// The change cannot alter what the program does. Fixes that guess the
    /// intent (which register to test, which size was meant) are not safe
    /// and are never applied without the user asking.
    pub safe: bool,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub lint: &'static Lint,
    pub severity: Severity,
    pub span: Span,
    /// One short sentence. The detail goes in `label`, `notes` and `help`.
    pub message: String,
    /// A few words shown right under the highlighted code.
    pub label: Option<String>,
    pub help: Option<String>,
    pub notes: Vec<Note>,
    pub fix: Option<Fix>,
}

impl Diagnostic {
    pub fn new(lint: &'static Lint, span: Span, message: impl Into<String>) -> Self {
        Self {
            lint,
            severity: lint.group.severity(),
            span,
            message: message.into(),
            label: None,
            help: None,
            notes: Vec::new(),
            fix: None,
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn note(mut self, message: impl Into<String>) -> Self {
        self.notes.push(Note { span: None, message: message.into() });
        self
    }

    /// A note that points at another location.
    pub fn note_at(mut self, span: Span, message: impl Into<String>) -> Self {
        self.notes.push(Note { span: Some(span), message: message.into() });
        self
    }

    /// Attaches a safe fix made of the given edits.
    pub fn fix(mut self, title: impl Into<String>, edits: Vec<Edit>) -> Self {
        self.fix = Some(Fix { title: title.into(), edits, safe: true });
        self
    }

    pub fn replace(self, title: impl Into<String>, span: Span, text: impl Into<String>) -> Self {
        self.fix(title, vec![Edit::Replace { span, text: text.into() }])
    }

    pub fn delete_line(self, title: impl Into<String>, line: usize) -> Self {
        self.fix(title, vec![Edit::DeleteLine(line)])
    }

    pub fn insert_before(
        self,
        title: impl Into<String>,
        line: usize,
        text: impl Into<String>,
    ) -> Self {
        self.fix(title, vec![Edit::InsertBefore { line, text: text.into() }])
    }

    /// Marks the attached fix as a guess at what was meant.
    pub fn maybe_incorrect(mut self) -> Self {
        if let Some(fix) = &mut self.fix {
            fix.safe = false;
        }
        self
    }
}
