//! Applying fixes to source text, and previewing them as a diff.

use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostic::{Edit, Fix};
use crate::parser::comment_start;

/// One line of a fix preview. `number` is the 0-based line of the original
/// file the row belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    Context { number: usize, text: String },
    Removed { number: usize, text: String },
    Added { number: usize, text: String },
}

/// Everything that happens to one line of the original.
#[derive(Default)]
struct LinePlan {
    before: Vec<String>,
    /// `(start, end, text)` byte ranges to replace.
    replaces: Vec<(usize, usize, String)>,
    delete: bool,
    after: Vec<String>,
}

fn plan<'a>(fixes: impl IntoIterator<Item = &'a Fix>) -> BTreeMap<usize, LinePlan> {
    let mut plans: BTreeMap<usize, LinePlan> = BTreeMap::new();
    for edit in fixes.into_iter().flat_map(|fix| &fix.edits) {
        match edit {
            Edit::Replace { span, text } => {
                plans.entry(span.line).or_default().replaces.push((
                    span.start,
                    span.end,
                    text.clone(),
                ));
            }
            Edit::DeleteLine(line) => plans.entry(*line).or_default().delete = true,
            Edit::InsertBefore { line, text } => {
                plans.entry(*line).or_default().before.push(text.clone())
            }
            Edit::InsertAfter { line, text } => {
                plans.entry(*line).or_default().after.push(text.clone())
            }
        }
    }
    plans
}

fn indentation(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// Splits inserted text into lines, each indented like `model`.
fn indented(text: &str, model: &str) -> Vec<String> {
    text.lines().map(|line| format!("{}{line}", indentation(model))).collect()
}

/// What `original` becomes: no line when deleted, several when a
/// replacement spans lines.
fn rewritten(plan: &LinePlan, original: &str) -> Vec<String> {
    if plan.delete {
        return Vec::new();
    }
    let mut text = original.to_string();
    let mut replaces = plan.replaces.clone();
    replaces.sort_by_key(|(start, ..)| std::cmp::Reverse(*start));
    for (start, end, with) in replaces {
        let (start, end) = (start.min(text.len()), end.min(text.len()));
        if text.is_char_boundary(start) && text.is_char_boundary(end) && start <= end {
            text.replace_range(start..end, &with);
        }
    }
    let mut lines = text.lines();
    let mut out = vec![lines.next().unwrap_or("").to_string()];
    out.extend(lines.map(|line| format!("{}{line}", indentation(original))));
    out
}

/// The lines a fix removes and adds, for display.
pub fn preview(fix: &Fix, lines: &[&str]) -> Vec<DiffLine> {
    let mut diff = Vec::new();
    for (number, plan) in plan([fix]) {
        let full = lines.get(number).copied().unwrap_or("");
        // A comment is noise in a preview: show the code alone.
        let original = full[..comment_start(full).min(full.len())].trim_end();
        let added = |text: String| DiffLine::Added { number, text };
        diff.extend(plan.before.iter().flat_map(|text| indented(text, original)).map(added));
        if plan.delete || !plan.replaces.is_empty() {
            diff.push(DiffLine::Removed { number, text: original.to_string() });
            diff.extend(rewritten(&plan, original).into_iter().map(added));
        } else {
            diff.push(DiffLine::Context { number, text: original.to_string() });
        }
        diff.extend(plan.after.iter().flat_map(|text| indented(text, original)).map(added));
    }
    diff
}

/// The fix as plain text, for editors that do not render Markdown:
/// `fix: mov [x], 0  →  mov byte [x], 0`, `fix: remove mov rax, rax`, or a
/// short list of `-` and `+` lines when several lines change.
pub fn summary(fix: &Fix, lines: &[&str]) -> String {
    let rows = preview(fix, lines);
    let collect = |wanted_removed: bool| -> Vec<String> {
        rows.iter()
            .filter_map(|row| match row {
                DiffLine::Removed { text, .. } if wanted_removed => Some(text.trim().to_string()),
                DiffLine::Added { text, .. } if !wanted_removed => Some(text.trim().to_string()),
                _ => None,
            })
            .collect()
    };
    let (removed, added) = (collect(true), collect(false));
    // The arrow says "this becomes that": only true on a single line.
    let lines_touched: std::collections::BTreeSet<usize> = rows
        .iter()
        .filter_map(|row| match row {
            DiffLine::Removed { number, .. } | DiffLine::Added { number, .. } => Some(*number),
            DiffLine::Context { .. } => None,
        })
        .collect();
    match (removed.as_slice(), added.as_slice()) {
        ([], []) => String::new(),
        ([old], [new]) if lines_touched.len() == 1 => format!("fix: {old}  →  {new}"),
        ([old], []) => format!("fix: remove {old}"),
        ([], [new]) => format!("fix: insert {new}"),
        _ => {
            let mut text = String::from("fix:");
            for line in &removed {
                text.push_str(&format!("\n  - {line}"));
            }
            for line in &added {
                text.push_str(&format!("\n  + {line}"));
            }
            text
        }
    }
}

/// Applies the fixes to `source`. A fix that touches a line an earlier fix
/// already changed is skipped: run again to pick it up. Returns the new
/// text and how many fixes were applied.
pub fn apply<'a>(source: &str, fixes: impl IntoIterator<Item = &'a Fix>) -> (String, usize) {
    let mut touched = BTreeSet::new();
    let mut accepted = Vec::new();
    for fix in fixes {
        let lines: BTreeSet<usize> = fix
            .edits
            .iter()
            .map(|edit| match edit {
                Edit::Replace { span, .. } => span.line,
                Edit::DeleteLine(line)
                | Edit::InsertBefore { line, .. }
                | Edit::InsertAfter { line, .. } => *line,
            })
            .collect();
        if lines.is_disjoint(&touched) {
            touched.extend(lines);
            accepted.push(fix);
        }
    }
    let count = accepted.len();
    let plans = plan(accepted);
    let mut out = Vec::new();
    for (number, original) in source.lines().enumerate() {
        match plans.get(&number) {
            None => out.push(original.to_string()),
            Some(plan) => {
                out.extend(plan.before.iter().flat_map(|text| indented(text, original)));
                out.extend(rewritten(plan, original));
                out.extend(plan.after.iter().flat_map(|text| indented(text, original)));
            }
        }
    }
    let mut text = out.join("\n");
    if source.ends_with('\n') {
        text.push('\n');
    }
    (text, count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::Span;

    fn fix(edits: Vec<Edit>) -> Fix {
        Fix { title: String::new(), edits, safe: true }
    }

    const SOURCE: &str = "start:\n    mov rax, 0\n    div rbx\n    ret\n";

    #[test]
    fn replaces_inserts_and_deletes() {
        let replace =
            fix(vec![Edit::Replace { span: Span::new(1, 4, 14), text: "xor eax, eax".into() }]);
        let insert = fix(vec![Edit::InsertBefore { line: 2, text: "xor edx, edx".into() }]);
        let exit = fix(vec![Edit::Replace {
            span: Span::new(3, 4, 7),
            text: "mov eax, 60\nsyscall".into(),
        }]);
        let (text, applied) = apply(SOURCE, [&replace, &insert, &exit]);
        assert_eq!(applied, 3);
        assert_eq!(
            text,
            "start:\n    xor eax, eax\n    xor edx, edx\n    div rbx\n    mov eax, 60\n    syscall\n"
        );
        let (text, _) = apply(SOURCE, [&fix(vec![Edit::DeleteLine(1)])]);
        assert_eq!(text, "start:\n    div rbx\n    ret\n");
    }

    #[test]
    fn overlapping_fixes_are_applied_one_at_a_time() {
        let a = fix(vec![Edit::DeleteLine(1)]);
        let b = fix(vec![Edit::Replace { span: Span::new(1, 4, 14), text: "nop".into() }]);
        let (text, applied) = apply(SOURCE, [&a, &b]);
        assert_eq!((applied, text.lines().count()), (1, 3));
    }

    #[test]
    fn previews() {
        let lines: Vec<&str> = SOURCE.lines().collect();
        let insert = fix(vec![
            Edit::InsertBefore { line: 2, text: "xor edx, edx".into() },
            Edit::InsertAfter { line: 2, text: "nop".into() },
        ]);
        assert_eq!(
            preview(&insert, &lines),
            [
                DiffLine::Added { number: 2, text: "    xor edx, edx".into() },
                DiffLine::Context { number: 2, text: "    div rbx".into() },
                DiffLine::Added { number: 2, text: "    nop".into() },
            ]
        );
        let replace =
            fix(vec![Edit::Replace { span: Span::new(1, 4, 14), text: "xor eax, eax".into() }]);
        assert_eq!(
            preview(&replace, &lines),
            [
                DiffLine::Removed { number: 1, text: "    mov rax, 0".into() },
                DiffLine::Added { number: 1, text: "    xor eax, eax".into() },
            ]
        );
    }

    #[test]
    fn previews_and_summaries_leave_comments_out() {
        let lines = ["    mov [x], 0 ; clear it", "    mov rax, rax ; no-op", "    div rbx"];
        let replace =
            fix(vec![Edit::Replace { span: Span::new(0, 4, 14), text: "mov byte [x], 0".into() }]);
        assert_eq!(summary(&replace, &lines), "fix: mov [x], 0  →  mov byte [x], 0");
        assert_eq!(summary(&fix(vec![Edit::DeleteLine(1)]), &lines), "fix: remove mov rax, rax");
        let insert = fix(vec![Edit::InsertBefore { line: 2, text: "xor edx, edx".into() }]);
        assert_eq!(summary(&insert, &lines), "fix: insert xor edx, edx");
        let several =
            fix(vec![Edit::DeleteLine(1), Edit::InsertAfter { line: 2, text: "nop".into() }]);
        assert_eq!(summary(&several, &lines), "fix:\n  - mov rax, rax\n  + nop");
        assert_eq!(summary(&fix(vec![]), &lines), "");
    }

    #[test]
    fn out_of_range_edits_do_not_panic() {
        let wild = fix(vec![
            Edit::Replace { span: Span::new(1, 90, 99), text: "x".into() },
            Edit::DeleteLine(50),
        ]);
        apply(SOURCE, [&wild]);
        preview(&wild, &SOURCE.lines().collect::<Vec<_>>());
    }
}
