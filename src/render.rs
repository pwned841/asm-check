//! Turning diagnostics into text.

use std::fmt::Write;

use serde::Serialize;

use crate::diagnostic::{Diagnostic, Severity};
use crate::fix::{self, DiffLine};
use crate::span::Span;

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

/// rustc-style output: the offending line with its labels, the related
/// lines, and the fix as a red and green diff.
pub fn human(path: &str, source: &str, diagnostics: &[Diagnostic], color: bool) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let p = Palette::new(color);
    let (bold, blue, reset) = (p.bold, p.blue, p.reset);
    let (red, green) = if color { ("\x1b[31m", "\x1b[32m") } else { ("", "") };
    let mut out = String::new();

    for d in diagnostics {
        let tint = p.severity(d.severity);
        // Every line shown, and what is underlined on it.
        let mut marks: Vec<(Span, bool, &str)> =
            vec![(d.span, true, d.label.as_deref().unwrap_or(""))];
        marks.extend(
            d.notes.iter().filter_map(|note| Some((note.span?, false, note.message.as_str()))),
        );
        marks.sort_by_key(|(span, primary, _)| (span.line, span.start, !*primary));
        let diff = d.fix.as_ref().map(|fix| fix::preview(fix, &lines)).unwrap_or_default();
        let last = marks
            .iter()
            .map(|(span, ..)| span.line)
            .chain(diff.iter().map(|row| match row {
                DiffLine::Context { number, .. }
                | DiffLine::Removed { number, .. }
                | DiffLine::Added { number, .. } => *number,
            }))
            .max()
            .unwrap_or(0);
        let width = (last + 1).to_string().len();
        let pad = " ".repeat(width);
        let gutter = format!("{pad} {blue}|{reset}");

        let text = lines.get(d.span.line).copied().unwrap_or("");
        let _ = writeln!(
            out,
            "{tint}{}[{}]{reset}{bold}: {}{reset}",
            d.severity, d.lint.name, d.message
        );
        let _ = writeln!(
            out,
            "{pad}{blue}-->{reset} {path}:{}:{}",
            d.span.line + 1,
            column(text, d.span.start)
        );
        let _ = writeln!(out, "{gutter}");

        let mut previous: Option<usize> = None;
        for (span, primary, label) in &marks {
            let text = lines.get(span.line).copied().unwrap_or("");
            if previous != Some(span.line) {
                if previous.is_some_and(|line| span.line > line + 1) {
                    let _ = writeln!(out, "{blue}...{reset}");
                }
                let _ = writeln!(out, "{blue}{:>width$} |{reset} {text}", span.line + 1);
            }
            previous = Some(span.line);
            // Keep tabs in the indentation so the underline lines up.
            let start = span.start.min(text.len());
            let end = span.end.clamp(start, text.len());
            let indent: String = text
                .get(..start)
                .unwrap_or("")
                .chars()
                .map(|c| if c == '\t' { '\t' } else { ' ' })
                .collect();
            let length = text.get(start..end).map_or(1, |s| s.chars().count()).max(1);
            let (stroke, tone) = if *primary { ("^", tint) } else { ("-", blue) };
            let space = if label.is_empty() { "" } else { " " };
            let _ = writeln!(
                out,
                "{gutter} {indent}{tone}{}{space}{label}{reset}",
                stroke.repeat(length)
            );
        }

        let notes: Vec<&str> =
            d.notes.iter().filter(|note| note.span.is_none()).map(|n| n.message.as_str()).collect();
        let help = d.help.as_deref().filter(|_| d.fix.is_none());
        if !notes.is_empty() || help.is_some() {
            let _ = writeln!(out, "{gutter}");
        }
        for note in notes {
            let _ = writeln!(out, "{pad} {blue}={reset} {bold}note{reset}: {note}");
        }
        if let Some(help) = help {
            let _ = writeln!(out, "{pad} {blue}={reset} {bold}help{reset}: {help}");
        }
        if let Some(fix) = &d.fix {
            let _ = writeln!(out, "{gutter}");
            let _ = writeln!(out, "{bold}help{reset}: {}", fix.title);
            let _ = writeln!(out, "{gutter}");
            for row in &diff {
                let _ = match row {
                    DiffLine::Context { number, text } => {
                        writeln!(out, "{blue}{:>width$} |{reset} {text}", number + 1)
                    }
                    DiffLine::Removed { number, text } => {
                        writeln!(out, "{blue}{:>width$}{reset} {red}- {text}{reset}", number + 1)
                    }
                    DiffLine::Added { text, .. } => {
                        writeln!(out, "{pad} {green}+ {text}{reset}")
                    }
                };
            }
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
    label: Option<&'a str>,
    help: Option<&'a str>,
    notes: Vec<JsonNote<'a>>,
    fix: Option<JsonFix<'a>>,
}

#[derive(Serialize)]
struct JsonFix<'a> {
    title: &'a str,
    /// Whether `--fix` applies it without `--unsafe-fixes`.
    safe: bool,
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
                label: d.label.as_deref(),
                help: d.help.as_deref(),
                fix: d.fix.as_ref().map(|fix| JsonFix { title: &fix.title, safe: fix.safe }),
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
