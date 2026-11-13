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
    /// Stricter suggestions, off unless requested.
    Pedantic,
}

impl Group {
    pub const fn name(self) -> &'static str {
        match self {
            Group::Correctness => "correctness",
            Group::Suspicious => "suspicious",
            Group::Style => "style",
            Group::Pedantic => "pedantic",
        }
    }

    pub const fn severity(self) -> Severity {
        match self {
            Group::Correctness => Severity::Error,
            Group::Suspicious => Severity::Warning,
            Group::Style | Group::Pedantic => Severity::Hint,
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

/// A text edit that resolves a diagnostic.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    Replace { span: Span, text: String },
    DeleteLine(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fix {
    pub title: String,
    pub edit: Edit,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub lint: &'static Lint,
    pub severity: Severity,
    pub span: Span,
    pub message: String,
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
            help: None,
            notes: Vec::new(),
            fix: None,
        }
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

    pub fn replace(
        mut self,
        title: impl Into<String>,
        span: Span,
        text: impl Into<String>,
    ) -> Self {
        self.fix =
            Some(Fix { title: title.into(), edit: Edit::Replace { span, text: text.into() } });
        self
    }

    pub fn delete_line(mut self, title: impl Into<String>, line: usize) -> Self {
        self.fix = Some(Fix { title: title.into(), edit: Edit::DeleteLine(line) });
        self
    }
}
