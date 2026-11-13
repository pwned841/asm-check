//! Inline suppression comments.
//!
//! ```nasm
//!     mov rax, 0          ; asm-check: allow(mov-zero)
//!     ; asm-check: allow(dead-store, unused-label)
//!     mov rbx, 1
//! ; asm-check: allow-file(needless-rex)
//! ```
//!
//! A comment on its own line applies to the next statement. `all` matches
//! every lint.

use std::collections::{HashMap, HashSet};

use crate::ast::Line;

#[derive(Debug, Default)]
pub struct Suppressions {
    file: HashSet<String>,
    lines: HashMap<usize, HashSet<String>>,
}

impl Suppressions {
    pub fn collect(lines: &[Line]) -> Self {
        let mut found = Suppressions::default();
        // Names from comment-only lines, waiting for the next statement.
        let mut pending: HashSet<String> = HashSet::new();
        for line in lines {
            let has_code = line.stmt.is_some() || line.label.is_some();
            if let Some((file_wide, names)) = line.comment.as_deref().and_then(parse_comment) {
                if file_wide {
                    found.file.extend(names);
                } else if has_code {
                    found.lines.entry(line.number).or_default().extend(names);
                } else {
                    pending.extend(names);
                }
            }
            if has_code && !pending.is_empty() {
                found.lines.entry(line.number).or_default().extend(pending.drain());
            }
        }
        found
    }

    pub fn allows(&self, lint: &str, line: usize) -> bool {
        let matches = |set: &HashSet<String>| set.contains(lint) || set.contains("all");
        matches(&self.file) || self.lines.get(&line).is_some_and(matches)
    }
}

/// Parses `asm-check: allow(a, b)`; returns whether it is file-wide.
fn parse_comment(comment: &str) -> Option<(bool, Vec<String>)> {
    let rest = comment[comment.find("asm-check:")? + "asm-check:".len()..].trim_start();
    let (file_wide, rest) = match rest.strip_prefix("allow-file(") {
        Some(rest) => (true, rest),
        None => (false, rest.strip_prefix("allow(")?),
    };
    let names = rest[..rest.find(')')?]
        .split(',')
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect();
    Some((file_wide, names))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    #[test]
    fn scopes() {
        let lines = parse(
            "mov rax, 0 ; asm-check: allow(mov-zero)\n\
             ; asm-check: allow(dead-store, self-move)\n\
             \n\
             mov rbx, rbx\n\
             ; note: asm-check: allow-file(needless-rex)\n\
             ret ; asm-check: allow(all)\n",
        );
        let found = Suppressions::collect(&lines);
        assert!(found.allows("mov-zero", 0) && !found.allows("mov-zero", 3));
        assert!(found.allows("dead-store", 3) && found.allows("self-move", 3));
        assert!(!found.allows("dead-store", 0));
        assert!(found.allows("needless-rex", 42));
        assert!(found.allows("anything", 5));
    }

    #[test]
    fn ignores_malformed_comments() {
        for comment in ["asm-check:", "asm-check: allow", "asm-check: allow(", "asm-check allow(x)"]
        {
            assert_eq!(parse_comment(comment), None);
        }
    }
}
