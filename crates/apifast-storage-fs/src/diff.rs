//! JSON Patch (RFC 6902) between two YAML values, element by element.
//!
//! Applying the patch to a [`oneyaml::Document`] changes only the lines that
//! differ, so comments and formatting elsewhere survive. Sequences are aligned
//! with a longest common subsequence, so removing the first header does not
//! rewrite every header after it.

use oneyaml::{PatchOp, Value};

/// Above this many cells, sequences are compared position by position
/// instead of with a full alignment table.
const MAX_ALIGNMENT_CELLS: usize = 1_000_000;

/// Operations that turn `old` into `new`.
pub(crate) fn diff(old: &Value, new: &Value) -> Vec<PatchOp> {
    let mut ops = Vec::new();
    diff_at(old, new, &mut String::new(), &mut ops);
    ops
}

fn diff_at(old: &Value, new: &Value, path: &mut String, ops: &mut Vec<PatchOp>) {
    if old == new {
        return;
    }
    match (old, new) {
        (Value::Mapping(a), Value::Mapping(b)) => {
            for (key, _) in a {
                if !b.contains_key(key) {
                    with_key(path, key, |p| ops.push(PatchOp::remove(p.clone())));
                }
            }
            for (key, new_value) in b {
                with_key(path, key, |p| match a.get(key) {
                    Some(old_value) => diff_at(old_value, new_value, p, ops),
                    None => ops.push(PatchOp::add(p.clone(), new_value.clone())),
                });
            }
        }
        (Value::Sequence(a), Value::Sequence(b)) => {
            diff_sequences(a.as_slice(), b.as_slice(), path, ops);
        }
        _ => ops.push(PatchOp::replace(path.clone(), new.clone())),
    }
}

/// Runs `f` with `path` extended by `/key`, escaped as RFC 6901 requires.
fn with_key(path: &mut String, key: &Value, f: impl FnOnce(&mut String)) {
    let len = path.len();
    path.push('/');
    let text = match key {
        Value::String(s) => s.clone(),
        other => oneyaml::to_string(other)
            .map(|s| s.trim_end().to_owned())
            .unwrap_or_default(),
    };
    path.push_str(&text.replace('~', "~0").replace('/', "~1"));
    f(path);
    path.truncate(len);
}

fn with_index(path: &mut String, index: usize, f: impl FnOnce(&mut String)) {
    let len = path.len();
    path.push('/');
    path.push_str(&index.to_string());
    f(path);
    path.truncate(len);
}

/// What happens to each element when turning one sequence into another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// `old[i]` stays as `new[j]`, unchanged.
    Keep(usize, usize),
    /// `old[i]` becomes `new[j]`, with changes inside.
    Change(usize, usize),
    /// `old[i]` is removed.
    Remove(usize),
    /// `new[j]` is inserted.
    Insert(usize),
}

/// Aligns two sequences: equal elements are kept (longest common
/// subsequence); between them, removed and inserted elements are paired as
/// changes, in order, and the rest are plain removals or insertions.
pub(crate) fn align<T: PartialEq>(old: &[T], new: &[T]) -> Vec<Step> {
    let (n, m) = (old.len(), new.len());
    let keeps = if n.saturating_mul(m) > MAX_ALIGNMENT_CELLS {
        // Too big for a table: keep equal elements at the same position only.
        (0..n.min(m))
            .filter(|&i| old[i] == new[i])
            .map(|i| (i, i))
            .collect()
    } else {
        lcs(old, new)
    };

    let mut steps = Vec::with_capacity(n.max(m));
    let (mut i, mut j) = (0, 0);
    for (ki, kj) in keeps.into_iter().chain(std::iter::once((n, m))) {
        let removed = ki - i;
        let inserted = kj - j;
        let paired = removed.min(inserted);
        for k in 0..paired {
            steps.push(Step::Change(i + k, j + k));
        }
        steps.extend((i + paired..ki).map(Step::Remove));
        steps.extend((j + paired..kj).map(Step::Insert));
        if ki < n && kj < m {
            steps.push(Step::Keep(ki, kj));
        }
        (i, j) = (ki + 1, kj + 1);
    }
    steps
}

/// Index pairs of a longest common subsequence, in order.
fn lcs<T: PartialEq>(old: &[T], new: &[T]) -> Vec<(usize, usize)> {
    let (n, m) = (old.len(), new.len());
    // table[i][j]: LCS length of old[i..] and new[j..].
    let mut table = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i][j] = if old[i] == new[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if old[i] == new[j] {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    pairs
}

fn diff_sequences(old: &[Value], new: &[Value], path: &mut String, ops: &mut Vec<PatchOp>) {
    // JSON Patch applies ops one after another, so `at` and `len` track the
    // position and length of the sequence as it is after the ops so far.
    let mut at = 0;
    let mut len = old.len();
    for step in align(old, new) {
        match step {
            Step::Keep(..) => at += 1,
            Step::Change(i, j) => {
                with_index(path, at, |p| diff_at(&old[i], &new[j], p, ops));
                at += 1;
            }
            Step::Remove(_) => {
                with_index(path, at, |p| ops.push(PatchOp::remove(p.clone())));
                len -= 1;
            }
            Step::Insert(j) => {
                if at == len {
                    // Appending with `-` lets the writer add lines at the end.
                    ops.push(PatchOp::add(format!("{path}/-"), new[j].clone()));
                } else {
                    with_index(path, at, |p| {
                        ops.push(PatchOp::add(p.clone(), new[j].clone()));
                    });
                }
                at += 1;
                len += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn yaml(text: &str) -> Value {
        oneyaml::from_str(text).unwrap()
    }

    fn apply(text: &str, ops: Vec<PatchOp>) -> String {
        let mut doc = oneyaml::Document::parse(text).unwrap();
        doc.apply_patch(&oneyaml::Patch::new(ops)).unwrap();
        doc.to_string()
    }

    fn roundtrip(old: &str, new: &str) -> String {
        let out = apply(old, diff(&yaml(old), &yaml(new)));
        assert_eq!(yaml(&out), yaml(new), "patched document:\n{out}");
        out
    }

    #[test]
    fn equal_values_need_no_ops() {
        assert_eq!(
            diff(&yaml("a: [1, {b: 2}]"), &yaml("a: [1, {b: 2}]")),
            vec![]
        );
    }

    #[test]
    fn a_changed_scalar_touches_only_its_line() {
        let old = "# head\na: 1 # one\nb: 2 # two\n";
        assert_eq!(
            roundtrip(old, "a: 1\nb: 3\n"),
            "# head\na: 1 # one\nb: 3 # two\n"
        );
    }

    #[test]
    fn removing_the_first_item_keeps_the_others_untouched() {
        let old = "h:\n  - name: a # first\n  - name: b # second\n  - name: c # third\n";
        let ops = diff(&yaml(old), &yaml("h:\n  - name: b\n  - name: c\n"));
        assert_eq!(ops, vec![PatchOp::remove("/h/0")]);
        assert_eq!(
            apply(old, ops),
            "h:\n  - name: b # second\n  - name: c # third\n"
        );
    }

    #[test]
    fn a_changed_item_in_the_middle_is_patched_in_place() {
        let old = "h:\n  - {name: a, value: 1}\n  - {name: b, value: 2}\n  - {name: c, value: 3}\n";
        let new = "h:\n  - {name: a, value: 1}\n  - {name: b, value: 9}\n  - {name: c, value: 3}\n";
        assert_eq!(
            diff(&yaml(old), &yaml(new)),
            vec![PatchOp::replace("/h/1/value", Value::from(9u32))]
        );
    }

    #[test]
    fn appended_items_use_the_dash_index() {
        let ops = diff(&yaml("l: [1, 2]"), &yaml("l: [1, 2, 3]"));
        assert_eq!(ops, vec![PatchOp::add("/l/-", Value::from(3u32))]);
    }

    #[test]
    fn keys_with_slashes_and_tildes_are_escaped() {
        let ops = diff(&yaml("'a/b~c': 1"), &yaml("'a/b~c': 2"));
        assert_eq!(ops, vec![PatchOp::replace("/a~1b~0c", Value::from(2u32))]);
    }

    #[test]
    fn alignment_pairs_changes_between_kept_items() {
        use Step::{Change, Insert, Keep, Remove};
        assert_eq!(
            align(&[1, 2, 3], &[1, 9, 3]),
            [Keep(0, 0), Change(1, 1), Keep(2, 2)]
        );
        assert_eq!(
            align(&[1, 2, 3], &[2, 3]),
            [Remove(0), Keep(1, 0), Keep(2, 1)]
        );
        assert_eq!(
            align(&[1, 2], &[0, 1, 2, 3]),
            [Insert(0), Keep(0, 1), Keep(1, 2), Insert(3)]
        );
        assert_eq!(align::<u8>(&[], &[]), []);
    }

    fn arb_value() -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            Just(Value::Null),
            any::<bool>().prop_map(Value::Bool),
            (0u32..5).prop_map(Value::from),
            "[a-c]{0,2}".prop_map(Value::from),
        ];
        leaf.prop_recursive(3, 24, 4, |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..4).prop_map(Value::from),
                prop::collection::vec(("[a-d]", inner), 0..4).prop_map(|pairs| {
                    let mut map = oneyaml::Mapping::new();
                    for (k, v) in pairs {
                        map.insert(Value::from(k), v);
                    }
                    Value::Mapping(map)
                }),
            ]
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        /// Whatever the two values, patching a document of the first gives
        /// exactly the second.
        #[test]
        fn the_patch_turns_old_into_new(old in arb_value(), new in arb_value()) {
            let text = oneyaml::to_string(&old).unwrap();
            let mut doc = oneyaml::Document::parse(text.as_str()).unwrap();
            doc.apply_patch(&oneyaml::Patch::new(diff(&old, &new))).unwrap();
            let out: Value = oneyaml::from_str(&doc.to_string()).unwrap();
            prop_assert_eq!(out, new);
        }

        #[test]
        fn alignment_covers_both_sequences(old in prop::collection::vec(0u8..4, 0..8), new in prop::collection::vec(0u8..4, 0..8)) {
            let steps = align(&old, &new);
            let olds: Vec<_> = steps.iter().filter_map(|s| match s {
                Step::Keep(i, _) | Step::Change(i, _) | Step::Remove(i) => Some(*i),
                Step::Insert(_) => None,
            }).collect();
            let news: Vec<_> = steps.iter().filter_map(|s| match s {
                Step::Keep(_, j) | Step::Change(_, j) | Step::Insert(j) => Some(*j),
                Step::Remove(_) => None,
            }).collect();
            prop_assert_eq!(olds, (0..old.len()).collect::<Vec<_>>());
            prop_assert_eq!(news, (0..new.len()).collect::<Vec<_>>());
            for step in steps {
                if let Step::Keep(i, j) = step { prop_assert_eq!(old[i], new[j]); }
            }
        }
    }
}
