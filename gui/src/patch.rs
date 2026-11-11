//! Applies hwlensd deltas to a snapshot. Mirrors `patch()` in src/hwlens_json.cpp.

use anyhow::{Context, Result, bail};
use serde_json::Value;

const OP_DEL: &str = "\u{0}del";
const OP_TAIL: &str = "\u{0}tail";
const OP_SIZE: &str = "\u{0}size";
const OP_AT: &str = "\u{0}at";

pub fn patch(target: &mut Value, delta: &Value) -> Result<()> {
    if let Value::Object(d) = delta {
        if let Some(tail) = d.get(OP_TAIL) {
            let Value::Array(arr) = target else { bail!("delta tail for a non array value") };
            arr.extend(tail.as_array().context("delta tail is not an array")?.iter().cloned());
            let n = d.get(OP_SIZE).and_then(Value::as_u64).context("delta tail without size")? as usize;
            if arr.len() > n {
                arr.drain(..arr.len() - n);
            }
            return Ok(());
        }
        if let Some(at) = d.get(OP_AT) {
            let Value::Array(arr) = target else { bail!("delta index for a non array value") };
            for (index, sub) in at.as_object().context("delta index is not an object")? {
                let i: usize = index.parse().context("delta index is not a number")?;
                let elem = arr.get_mut(i).with_context(|| format!("delta index {i} out of range"))?;
                patch(elem, sub)?;
            }
            return Ok(());
        }
        if let Value::Object(obj) = target {
            for (key, value) in d {
                if value.as_object().is_some_and(|o| o.contains_key(OP_DEL)) {
                    obj.remove(key);
                } else if let Some(existing) = obj.get_mut(key) {
                    patch(existing, value)?;
                } else {
                    obj.insert(key.clone(), value.clone());
                }
            }
            return Ok(());
        }
    }
    *target = delta.clone();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tail_appends_and_trims() {
        let mut v = json!({"h": [1, 2, 3]});
        patch(&mut v, &json!({"h": {"\u{0}tail": [4, 5], "\u{0}size": 3}})).unwrap();
        assert_eq!(v, json!({"h": [3, 4, 5]}));
    }

    #[test]
    fn tail_grows_until_size() {
        let mut v = json!([1]);
        patch(&mut v, &json!({"\u{0}tail": [2], "\u{0}size": 600})).unwrap();
        assert_eq!(v, json!([1, 2]));
    }

    #[test]
    fn at_patches_elements() {
        let mut v = json!([[1, 2], [3, 4]]);
        patch(&mut v, &json!({"\u{0}at": {"1": {"\u{0}tail": [5], "\u{0}size": 2}}})).unwrap();
        assert_eq!(v, json!([[1, 2], [4, 5]]));
    }

    #[test]
    fn objects_merge_delete_insert() {
        let mut v = json!({"list": {"1": {"cpu": 0.0, "name": "a"}, "2": {"cpu": 1.0}}});
        patch(
            &mut v,
            &json!({"list": {"1": {"cpu": 3.5}, "2": {"\u{0}del": 1}, "3": {"cpu": 9.0}}}),
        )
        .unwrap();
        assert_eq!(v, json!({"list": {"1": {"cpu": 3.5, "name": "a"}, "3": {"cpu": 9.0}}}));
    }

    #[test]
    fn plain_values_and_type_changes_replace() {
        let mut v = json!({"a": 1, "b": [1], "c": "x"});
        patch(&mut v, &json!({"a": 2, "b": [7, 8], "c": {"k": 1}})).unwrap();
        assert_eq!(v, json!({"a": 2, "b": [7, 8], "c": {"k": 1}}));
    }

    #[test]
    fn tail_on_scalar_is_an_error() {
        let mut v = json!(5);
        assert!(patch(&mut v, &json!({"\u{0}tail": [1], "\u{0}size": 1})).is_err());
    }
}
