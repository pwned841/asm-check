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
