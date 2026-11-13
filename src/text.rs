//! Small string helpers for "did you mean" suggestions.

/// Levenshtein distance.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substitution = diagonal + usize::from(ca != *cb);
            diagonal = row[j + 1];
            row[j + 1] = substitution.min(row[j] + 1).min(diagonal + 1);
        }
    }
    row[b.len()]
}

/// Like [`edit_distance`], but two neighbouring characters typed in the
/// wrong order (`mvo` for `mov`) count as one mistake, not two.
pub fn typo_distance(a: &str, b: &str) -> usize {
    let (x, y): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let swapped = x.len() == y.len()
        && (1..x.len()).any(|i| {
            x[i - 1] == y[i]
                && x[i] == y[i - 1]
                && x[..i - 1] == y[..i - 1]
                && x[i + 1..] == y[i + 1..]
        });
    if swapped && x != y { 1 } else { edit_distance(a, b) }
}

/// The candidate closest to `name`, if it is within `budget` mistakes.
pub fn closest<'a>(
    name: &str,
    candidates: impl IntoIterator<Item = &'a str>,
    budget: usize,
) -> Option<&'a str> {
    candidates
        .into_iter()
        .map(|candidate| (typo_distance(name, candidate), candidate))
        .filter(|(distance, candidate)| *distance <= budget && *candidate != name)
        .min()
        .map(|(_, candidate)| candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances() {
        assert_eq!(edit_distance("buffer", "buffer"), 0);
        assert_eq!(edit_distance("bufer", "buffer"), 1);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(typo_distance("mvo", "mov"), 1);
        assert_eq!(typo_distance(".bbs", ".bss"), 1);
        assert_eq!(typo_distance("sycall", "syscall"), 1);
        assert_eq!(typo_distance("abc", "cba"), 2);
    }

    #[test]
    fn suggests_only_near_misses() {
        let names = ["buffer", "message"];
        assert_eq!(closest("bufer", names, 1), Some("buffer"));
        assert_eq!(closest("printf", names, 2), None);
        assert_eq!(closest("buffer", names, 1), None);
    }
}
