use std::collections::BTreeMap;

use serde_json::{Map, Value};

/// Flattens a JSON value into a flat map of dot-path -> leaf value.
///
/// `{"a":{"b":[1,2]}}` becomes `{"a.b.0": 1, "a.b.1": 2}`. Object keys and
/// array indices share the same `.` separator - there is no bracket
/// notation, so a numeric object key and an array index at the same depth
/// are indistinguishable by path alone (only `unflatten`'s own
/// array-vs-object heuristic tells them apart, see below).
///
/// An empty object (`{}`) or empty array (`[]`) has no children to recurse
/// into, so it is kept as a leaf value at its own path rather than
/// disappearing - `{"a":{}}` becomes `{"a": {}}`, one entry, not zero.
///
/// A top-level scalar (or top-level empty object/array) has no key of its
/// own, so it is stored under the empty-string path `""`. This makes a
/// bare top-level scalar indistinguishable from an object containing a
/// single empty-string key holding that same scalar - a real, documented
/// ambiguity (see the crate README's scope-limits section) rather than a
/// silently wrong result.
pub fn flatten(value: &Value) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    flatten_into(value, String::new(), &mut out);
    out
}

fn is_leaf(value: &Value) -> bool {
    match value {
        Value::Object(m) => m.is_empty(),
        Value::Array(a) => a.is_empty(),
        _ => true,
    }
}

fn flatten_into(value: &Value, prefix: String, out: &mut BTreeMap<String, Value>) {
    if is_leaf(value) {
        out.insert(prefix, value.clone());
        return;
    }
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                let key = join(&prefix, k);
                flatten_into(v, key, out);
            }
        }
        Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                let key = join(&prefix, &i.to_string());
                flatten_into(v, key, out);
            }
        }
        _ => unreachable!("scalars are always leaves"),
    }
}

fn join(prefix: &str, segment: &str) -> String {
    if prefix.is_empty() {
        segment.to_string()
    } else {
        format!("{prefix}.{segment}")
    }
}

/// Intermediate tree used while reassembling a flat map, before a final
/// pass decides which nodes became a JSON array vs a JSON object.
enum Tree {
    Leaf(Value),
    Children(BTreeMap<String, Tree>),
}

/// Reverses [`flatten`]: reassembles a flat dot-path map back into nested
/// JSON. For `unflatten(flatten(x)) == x` to hold, a node's children must
/// be recognized as an array whenever they were originally one - this is
/// decided per node by checking whether *every* child key parses as a
/// plain non-negative integer (`"0"`, `"1"`, ...); if so the node becomes
/// a `Value::Array` indexed accordingly (any gap in the indices, which
/// `flatten`'s own output never produces, is filled with `null` rather
/// than panicking), otherwise it becomes a `Value::Object`.
pub fn unflatten(flat: &BTreeMap<String, Value>) -> Value {
    // A single entry at the empty-string path is the sentinel for "the
    // whole document was a bare scalar (or empty object/array)" - see the
    // ambiguity documented on `flatten`.
    if flat.len() == 1 {
        if let Some(v) = flat.get("") {
            return v.clone();
        }
    }

    let mut root: BTreeMap<String, Tree> = BTreeMap::new();
    for (path, value) in flat {
        let segments: Vec<&str> = path.split('.').collect();
        insert(&mut root, &segments, value.clone());
    }
    tree_map_to_value(root)
}

fn insert(map: &mut BTreeMap<String, Tree>, segments: &[&str], value: Value) {
    let head = segments[0];
    if segments.len() == 1 {
        map.insert(head.to_string(), Tree::Leaf(value));
        return;
    }
    let rest = &segments[1..];
    match map.get_mut(head) {
        Some(Tree::Children(child_map)) => insert(child_map, rest, value),
        _ => {
            let mut child_map = BTreeMap::new();
            insert(&mut child_map, rest, value);
            map.insert(head.to_string(), Tree::Children(child_map));
        }
    }
}

fn tree_map_to_value(map: BTreeMap<String, Tree>) -> Value {
    let all_numeric = !map.is_empty() && map.keys().all(|k| k.parse::<usize>().is_ok());
    if all_numeric {
        let mut indexed: Vec<(usize, Tree)> = map
            .into_iter()
            .map(|(k, v)| (k.parse::<usize>().expect("checked above"), v))
            .collect();
        indexed.sort_by_key(|(i, _)| *i);
        let max_index = indexed.last().map(|(i, _)| *i).unwrap_or(0);
        let mut arr = vec![Value::Null; max_index + 1];
        for (i, tree) in indexed {
            arr[i] = tree_to_value(tree);
        }
        Value::Array(arr)
    } else {
        let mut obj = Map::new();
        for (k, v) in map {
            obj.insert(k, tree_to_value(v));
        }
        Value::Object(obj)
    }
}

fn tree_to_value(tree: Tree) -> Value {
    match tree {
        Tree::Leaf(v) => v,
        Tree::Children(map) => tree_map_to_value(map),
    }
}

/// Parses one `key=value` line (as produced by the `flatten` CLI command)
/// into a `(path, Value)` pair. The value half is JSON - so `a.b=1` and
/// `a.b="1"` are different values (a number vs a string) - and only the
/// *first* `=` splits the line, so a value that itself contains `=`
/// (inside a JSON string) round-trips correctly.
pub fn parse_line(line: &str) -> Result<(String, Value), String> {
    let (key, raw_value) = line
        .split_once('=')
        .ok_or_else(|| format!("no '=' found in line: {line:?}"))?;
    let value: Value = serde_json::from_str(raw_value)
        .map_err(|e| format!("invalid JSON value in {line:?}: {e}"))?;
    Ok((key.to_string(), value))
}

/// Renders one flat entry as a `key=value` line, the inverse of
/// [`parse_line`]. The value is JSON-encoded compactly, so a string value
/// keeps its surrounding quotes (`a.b="hi"`) and is unambiguous to parse
/// back.
pub fn render_line(key: &str, value: &Value) -> String {
    format!("{key}={value}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn flat_pairs(value: &Value) -> Vec<(String, Value)> {
        flatten(value).into_iter().collect()
    }

    #[test]
    fn flattens_simple_nested_object() {
        let v = json!({"a": {"b": 1}});
        assert_eq!(flat_pairs(&v), vec![("a.b".to_string(), json!(1))]);
    }

    #[test]
    fn flattens_array_of_scalars_with_numeric_segments() {
        let v = json!({"a": {"b": [1, 2]}});
        assert_eq!(
            flat_pairs(&v),
            vec![
                ("a.b.0".to_string(), json!(1)),
                ("a.b.1".to_string(), json!(2)),
            ]
        );
    }

    #[test]
    fn flattens_array_of_objects() {
        let v = json!({"users": [{"name": "a"}, {"name": "b"}]});
        assert_eq!(
            flat_pairs(&v),
            vec![
                ("users.0.name".to_string(), json!("a")),
                ("users.1.name".to_string(), json!("b")),
            ]
        );
    }

    #[test]
    fn flattens_deeply_nested_mixed_structure() {
        let v = json!({
            "a": {
                "b": [1, {"c": true}, [9, 10]]
            }
        });
        assert_eq!(
            flat_pairs(&v),
            vec![
                ("a.b.0".to_string(), json!(1)),
                ("a.b.1.c".to_string(), json!(true)),
                ("a.b.2.0".to_string(), json!(9)),
                ("a.b.2.1".to_string(), json!(10)),
            ]
        );
    }

    #[test]
    fn empty_object_and_array_kept_as_leaves_not_dropped() {
        let v = json!({"a": {}, "b": [], "c": 1});
        assert_eq!(
            flat_pairs(&v),
            vec![
                ("a".to_string(), json!({})),
                ("b".to_string(), json!([])),
                ("c".to_string(), json!(1)),
            ]
        );
    }

    #[test]
    fn top_level_scalar_uses_empty_string_path() {
        let v = json!(42);
        assert_eq!(flat_pairs(&v), vec![("".to_string(), json!(42))]);
    }

    #[test]
    fn null_and_bool_and_string_values_survive() {
        let v = json!({"a": null, "b": false, "c": "hi"});
        assert_eq!(
            flat_pairs(&v),
            vec![
                ("a".to_string(), json!(null)),
                ("b".to_string(), json!(false)),
                ("c".to_string(), json!("hi")),
            ]
        );
    }

    #[test]
    fn round_trip_nested_object() {
        let v = json!({"a": {"b": {"c": 1, "d": "x"}}});
        assert_eq!(unflatten(&flatten(&v)), v);
    }

    #[test]
    fn round_trip_array_of_scalars() {
        let v = json!({"tags": ["x", "y", "z"]});
        assert_eq!(unflatten(&flatten(&v)), v);
    }

    #[test]
    fn round_trip_array_of_objects() {
        let v = json!({
            "users": [
                {"name": "alice", "age": 30, "active": true},
                {"name": "bob", "age": 25, "active": false}
            ]
        });
        assert_eq!(unflatten(&flatten(&v)), v);
    }

    #[test]
    fn round_trip_deeply_nested_mixed_structure() {
        let v = json!({
            "meta": {"version": 2, "tags": ["a", "b"]},
            "records": [
                {"id": 1, "labels": {"x": 1, "y": [1, 2, 3]}},
                {"id": 2, "labels": {"x": 2, "y": []}}
            ],
            "note": null
        });
        assert_eq!(unflatten(&flatten(&v)), v);
    }

    #[test]
    fn round_trip_empty_object_and_array_values() {
        let v = json!({"a": {}, "b": [], "c": {"d": {}}});
        assert_eq!(unflatten(&flatten(&v)), v);
    }

    #[test]
    fn round_trip_top_level_scalar() {
        let v = json!(true);
        assert_eq!(unflatten(&flatten(&v)), v);
    }

    #[test]
    fn round_trip_top_level_array() {
        let v = json!([1, 2, 3]);
        assert_eq!(unflatten(&flatten(&v)), v);
    }

    #[test]
    fn round_trip_floats_and_negative_numbers() {
        let v = json!({"a": -3.5, "b": [-1, 2]});
        assert_eq!(unflatten(&flatten(&v)), v);
    }

    #[test]
    fn unflatten_fills_sparse_array_gaps_with_null() {
        let mut flat = BTreeMap::new();
        flat.insert("a.0".to_string(), json!("x"));
        flat.insert("a.2".to_string(), json!("z"));
        let result = unflatten(&flat);
        assert_eq!(result, json!({"a": ["x", null, "z"]}));
    }

    #[test]
    fn line_round_trip_preserves_string_with_equals_sign() {
        let line = render_line("a.b", &json!("x=y"));
        assert_eq!(line, "a.b=\"x=y\"");
        let (key, value) = parse_line(&line).unwrap();
        assert_eq!(key, "a.b");
        assert_eq!(value, json!("x=y"));
    }

    #[test]
    fn parse_line_rejects_missing_equals() {
        assert!(parse_line("no-equals-here").is_err());
    }

    #[test]
    fn parse_line_rejects_invalid_json_value() {
        assert!(parse_line("a.b=not json").is_err());
    }

    #[test]
    fn full_text_round_trip_via_lines() {
        let v = json!({"a": {"b": [1, "two", null, {"c": false}]}});
        let flat = flatten(&v);
        let lines: Vec<String> = flat.iter().map(|(k, val)| render_line(k, val)).collect();

        let mut reparsed = BTreeMap::new();
        for line in &lines {
            let (k, val) = parse_line(line).unwrap();
            reparsed.insert(k, val);
        }
        assert_eq!(unflatten(&reparsed), v);
    }
}
