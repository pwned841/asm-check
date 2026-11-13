//! Turning diagnostics into text.

use std::fmt::Write;

use serde::Serialize;

use crate::diagnostic::{Diagnostic, Severity};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Multi-line, with the offending source line, like rustc.
    Human,
    /// One `path:line:col: severity[lint]: message` line per diagnostic,
    /// the format editors' problem matchers understand.
    Short,
    Json,
}

struct Palette {
    bold: &'static str,
    blue: &'static str,
    reset: &'static str,
    color: bool,
}

impl Palette {
    fn new(color: bool) -> Self {
        if color {
            Self { bold: "\x1b[1m", blue: "\x1b[1;34m", reset: "\x1b[0m", color }
        } else {
            Self { bold: "", blue: "", reset: "", color }
        }
    }

    fn severity(&self, severity: Severity) -> &'static str {
        match (self.color, severity) {
            (false, _) => "",
            (true, Severity::Error) => "\x1b[1;31m",
            (true, Severity::Warning) => "\x1b[1;33m",
            (true, Severity::Hint) => "\x1b[1;36m",
        }
    }
}

/// 1-based column of a byte offset, counted in characters.
fn column(line: &str, offset: usize) -> usize {
    line.get(..offset).map_or(offset, |prefix| prefix.chars().count()) + 1
}

pub fn short(path: &str, source: &str, diagnostics: &[Diagnostic]) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let mut out = String::new();
    for d in diagnostics {
        let col = column(lines.get(d.span.line).copied().unwrap_or(""), d.span.start);
        let _ = writeln!(
            out,
            "{path}:{}:{col}: {}[{}]: {}",
            d.span.line + 1,
            d.severity,
            d.lint.name,
            d.message
        );
    }
    out
}

pub fn human(path: &str, source: &str, diagnostics: &[Diagnostic], color: bool) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let p = Palette::new(color);
    let mut out = String::new();
    for d in diagnostics {
        let text = lines.get(d.span.line).copied().unwrap_or("");
        let number = (d.span.line + 1).to_string();
        let pad = " ".repeat(number.len());
        let tint = p.severity(d.severity);
        let (bold, blue, reset) = (p.bold, p.blue, p.reset);

        let _ = writeln!(
            out,
            "{tint}{}[{}]{reset}{bold}: {}{reset}",
            d.severity, d.lint.name, d.message
        );
        let _ =
            writeln!(out, "{pad}{blue}-->{reset} {path}:{number}:{}", column(text, d.span.start));
        let _ = writeln!(out, "{pad} {blue}|{reset}");
        let _ = writeln!(out, "{blue}{number} |{reset} {text}");

        // Keep tabs in the gutter so the carets line up whatever the tab width.
        let start = d.span.start.min(text.len());
        let end = d.span.end.clamp(start, text.len());
        let indent: String = text
            .get(..start)
            .unwrap_or("")
            .chars()
            .map(|c| if c == '\t' { '\t' } else { ' ' })
            .collect();
        let carets = "^".repeat(text.get(start..end).map_or(1, |s| s.chars().count()).max(1));
        let _ = writeln!(out, "{pad} {blue}|{reset} {indent}{tint}{carets}{reset}");

        if d.help.is_some() || !d.notes.is_empty() {
            let _ = writeln!(out, "{pad} {blue}|{reset}");
        }
        for note in &d.notes {
            let _ = writeln!(out, "{pad} {blue}={reset} {bold}note{reset}: {}", note.message);
        }
        if let Some(help) = &d.help {
            let _ = writeln!(out, "{pad} {blue}={reset} {bold}help{reset}: {help}");
        }
        out.push('\n');
    }
    out
}

#[derive(Serialize)]
struct JsonDiagnostic<'a> {
    file: &'a str,
    /// 1-based.
    line: usize,
    /// 1-based, in characters.
    column: usize,
    end_column: usize,
    severity: &'static str,
    lint: &'static str,
    group: &'static str,
    message: &'a str,
    help: Option<&'a str>,
    notes: Vec<JsonNote<'a>>,
}

#[derive(Serialize)]
struct JsonNote<'a> {
    line: Option<usize>,
    message: &'a str,
}

/// One JSON array for the file.
pub fn json(path: &str, source: &str, diagnostics: &[Diagnostic]) -> serde_json::Value {
    let lines: Vec<&str> = source.lines().collect();
    let items: Vec<JsonDiagnostic> = diagnostics
        .iter()
        .map(|d| {
            let text = lines.get(d.span.line).copied().unwrap_or("");
            JsonDiagnostic {
                file: path,
                line: d.span.line + 1,
                column: column(text, d.span.start),
                end_column: column(text, d.span.end),
                severity: d.severity.label(),
                lint: d.lint.name,
                group: d.lint.group.name(),
                message: &d.message,
                help: d.help.as_deref(),
                notes: d
                    .notes
                    .iter()
                    .map(|note| JsonNote {
                        line: note.span.map(|span| span.line + 1),
                        message: &note.message,
                    })
                    .collect(),
            }
        })
        .collect();
    serde_json::to_value(items).unwrap_or_default()
}

/// `2 errors, 1 warning`, or `None` when there is nothing to report.
pub fn summary(diagnostics: &[Diagnostic]) -> Option<String> {
    let count = |severity| diagnostics.iter().filter(|d| d.severity == severity).count();
    let parts: Vec<String> = [Severity::Error, Severity::Warning, Severity::Hint]
        .into_iter()
        .map(|severity| (count(severity), severity))
        .filter(|(n, _)| *n > 0)
        .map(|(n, severity)| format!("{n} {severity}{}", if n == 1 { "" } else { "s" }))
        .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}
