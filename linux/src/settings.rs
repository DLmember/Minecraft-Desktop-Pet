//! 以 JSON 为唯一事实来源读写核心设置（键名与 native 的 serde camelCase 一致）。

use serde_json::{json, Value};

/// 读取 `a.b.3` 形式的路径；数字段表示数组下标。
pub fn get(root: &Value, path: &str) -> Value {
    let mut cur = root;
    for part in path.split('.') {
        match part.parse::<usize>() {
            Ok(index) => match cur.get(index) {
                Some(v) => cur = v,
                None => return Value::Null,
            },
            Err(_) => match cur.get(part) {
                Some(v) => cur = v,
                None => return Value::Null,
            },
        }
    }
    cur.clone()
}

pub fn get_bool(root: &Value, path: &str) -> bool {
    get(root, path).as_bool().unwrap_or(false)
}

pub fn get_f64(root: &Value, path: &str) -> f64 {
    get(root, path).as_f64().unwrap_or(0.0)
}

pub fn get_str(root: &Value, path: &str) -> String {
    get(root, path).as_str().unwrap_or_default().to_string()
}

/// 写入 `a.b.3` 形式的路径；不存在的中间对象会自动创建。
pub fn set(root: &mut Value, path: &str, value: Value) {
    let parts: Vec<String> = path.split('.').map(str::to_string).collect();
    set_parts(root, &parts, value);
}

fn set_parts(cur: &mut Value, parts: &[String], value: Value) {
    let Some((head, rest)) = parts.split_first() else {
        *cur = value;
        return;
    };
    if let Ok(index) = head.parse::<usize>() {
        if !cur.is_array() {
            *cur = json!([]);
        }
        let arr = cur.as_array_mut().expect("array");
        while arr.len() <= index {
            arr.push(Value::Null);
        }
        if rest.is_empty() {
            arr[index] = value;
        } else {
            set_parts(&mut arr[index], rest, value);
        }
    } else {
        if !cur.is_object() {
            *cur = json!({});
        }
        let obj = cur.as_object_mut().expect("object");
        let slot = obj.entry(head.clone()).or_insert(Value::Null);
        if rest.is_empty() {
            *slot = value;
        } else {
            set_parts(slot, rest, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_paths_roundtrip() {
        let mut v = json!({"modelRotation":[0,0,0],"customPose":{"head":[1,2,3]},"scale":1.0});
        set(&mut v, "modelRotation.1", json!(42));
        set(&mut v, "customPose.head.2", json!(-7));
        set(&mut v, "scale", json!(1.5));
        assert_eq!(get_f64(&v, "modelRotation.1"), 42.0);
        assert_eq!(get_f64(&v, "customPose.head.2"), -7.0);
        assert_eq!(get_f64(&v, "scale"), 1.5);
        assert_eq!(get(&v, "missing.key"), Value::Null);
    }
}
