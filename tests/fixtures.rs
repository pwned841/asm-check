//! Fixture tests.
//!
//! `tests/fixtures/<lint>.s` exercises one lint: every line that ends with a
//! `;~` marker must be reported by that lint, and no other line may be.
//! `tests/clean/*.s` are correct programs that must produce no diagnostic.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use asm_check::{Config, check, lints, render};

fn sources(dir: &str) -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(dir);
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "s"))
        .collect();
    paths.sort();
    paths
}

#[test]
fn lints_fire_exactly_where_expected() {
    let config = Config { pedantic: true, ..Config::default() };
    let mut covered = BTreeSet::new();
    let mut failures = Vec::new();

    for path in sources("fixtures") {
        let lint = path.file_stem().unwrap().to_str().unwrap().to_string();
        assert!(lints::find(&lint).is_some(), "{}: no lint named `{lint}`", path.display());
        let source = fs::read_to_string(&path).unwrap();

        let expected: BTreeSet<usize> = source
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains(";~"))
            .map(|(index, _)| index + 1)
            .collect();
        let actual: BTreeSet<usize> = check(&source, &config)
            .iter()
            .filter(|diagnostic| diagnostic.lint.name == lint)
            .map(|diagnostic| diagnostic.span.line + 1)
            .collect();

        let missing: Vec<_> = expected.difference(&actual).collect();
        let unexpected: Vec<_> = actual.difference(&expected).collect();
        if !missing.is_empty() || !unexpected.is_empty() {
            failures.push(format!("{lint}: not reported on lines {missing:?}, wrongly reported on lines {unexpected:?}"));
        }
        covered.insert(lint);
    }

    assert!(failures.is_empty(), "\n{}\n", failures.join("\n"));
    for lint in lints::ALL {
        assert!(covered.contains(lint.name), "lint `{}` has no fixture", lint.name);
    }
}

#[test]
fn clean_programs_stay_clean() {
    for path in sources("clean") {
        let source = fs::read_to_string(&path).unwrap();
        let diagnostics = check(&source, &Config::default());
        let name = path.display().to_string();
        assert!(diagnostics.is_empty(), "\n{}", render::human(&name, &source, &diagnostics, false));
    }
}

#[test]
fn lint_names_are_unique_and_kebab_case() {
    let mut seen = BTreeSet::new();
    for lint in lints::ALL {
        assert!(seen.insert(lint.name), "duplicate lint `{}`", lint.name);
        assert!(
            lint.name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "`{}` is not kebab-case",
            lint.name
        );
    }
}

/// Whatever the input, the tool must answer rather than crash: an editor
/// runs it on every keystroke, on half-typed lines.
#[test]
fn never_panics_on_truncated_or_mangled_input() {
    let config = Config { pedantic: true, ..Config::default() };
    for path in sources("fixtures").into_iter().chain(sources("clean")) {
        let source = fs::read_to_string(&path).unwrap();
        for end in (0..source.len()).filter(|end| source.is_char_boundary(*end)) {
            check(&source[..end], &config);
        }
        // Drop each character class in turn to break the syntax in new ways.
        for removed in [' ', ',', ':', '[', ']', '\n', '.'] {
            check(&source.replace(removed, ""), &config);
        }
    }
}

#[test]
fn gnu_syntax_is_left_alone() {
    let att = ".globl _start\n_start:\n    movq $60, %rax\n    syscall\n";
    assert!(asm_check::is_gnu_syntax(att));
    assert!(check(att, &Config::default()).is_empty());
    assert!(!asm_check::is_gnu_syntax("global _start\n_start:\n    mov rax, 10 % 3\n"));
}

#[test]
fn other_modes_are_left_alone() {
    let source = "bits 32\n_start:\n    push eax\n    int 0x80\n";
    assert!(check(source, &Config::default()).is_empty());
}

#[test]
fn every_lint_is_documented() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/lints.md");
    let docs = fs::read_to_string(path).unwrap();
    let documented: BTreeSet<&str> =
        docs.lines().filter_map(|line| line.strip_prefix("### ")).collect();
    let existing: BTreeSet<&str> = lints::ALL.iter().map(|lint| lint.name).collect();
    assert_eq!(documented, existing, "docs/lints.md and the lint list disagree");
}
