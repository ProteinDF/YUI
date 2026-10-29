//! メッセージの値（msgpack の `rmpv::Value`）と、アプリの中で使う JSON（`serde_json::Value`）の変換。
//!
//! メニュー項目の `args` と、コマンドに渡す `target` / `args` は、アプリの中では JSON で持っている
//! （`CommandInvocation` の引数）。プロトコルでは msgpack で運ぶので、境目で変換する。

use rmpv::Value as Mp;
use serde_json::{Map, Number, Value as Js};

/// msgpack の値を JSON にする。JSON にできないもの（バイナリ、拡張型、文字列でないキー、
/// 不正な UTF-8）は、理由つきの失敗にする。
pub fn to_json(value: &Mp) -> Result<Js, String> {
    Ok(match value {
        Mp::Nil => Js::Null,
        Mp::Boolean(b) => Js::Bool(*b),
        Mp::Integer(n) => {
            if let Some(i) = n.as_i64() {
                Js::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                Js::Number(u.into())
            } else {
                return Err("an integer is out of range".into());
            }
        }
        Mp::F32(f) => float(f64::from(*f)),
        Mp::F64(f) => float(*f),
        Mp::String(s) => Js::String(
            s.as_str()
                .ok_or_else(|| "a string is not valid UTF-8".to_string())?
                .to_string(),
        ),
        Mp::Binary(_) => return Err("binary data cannot be used here".into()),
        Mp::Array(items) => Js::Array(items.iter().map(to_json).collect::<Result<_, _>>()?),
        Mp::Map(entries) => {
            let mut map = Map::new();
            for (key, value) in entries {
                let key = key
                    .as_str()
                    .ok_or_else(|| "a map key must be a string".to_string())?;
                map.insert(key.to_string(), to_json(value)?);
            }
            Js::Object(map)
        }
        Mp::Ext(..) => return Err("an extension type cannot be used here".into()),
    })
}

/// 有限でない小数（NaN、無限大）は、JSON に書けないので null にする
fn float(f: f64) -> Js {
    Number::from_f64(f).map_or(Js::Null, Js::Number)
}

/// JSON を msgpack の値にする
pub fn from_json(value: &Js) -> Mp {
    match value {
        Js::Null => Mp::Nil,
        Js::Bool(b) => Mp::Boolean(*b),
        Js::Number(n) => {
            if let Some(i) = n.as_i64() {
                Mp::from(i)
            } else if let Some(u) = n.as_u64() {
                Mp::from(u)
            } else {
                Mp::F64(n.as_f64().unwrap_or(0.0))
            }
        }
        Js::String(s) => Mp::from(s.as_str()),
        Js::Array(items) => Mp::Array(items.iter().map(from_json).collect()),
        Js::Object(map) => Mp::Map(
            map.iter()
                .map(|(key, value)| (Mp::from(key.as_str()), from_json(value)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_goes_to_msgpack_and_back() {
        let value = json!({
            "kind": "atom",
            "atom_id": 12,
            "negative": -7,
            "big": 18_446_744_073_709_551_615u64,
            "pi": 2.5,
            "flag": true,
            "none": null,
            "list": [1, "two", {"three": 3}],
            "text": "日本語"
        });
        let back = to_json(&from_json(&value)).expect("round trip");
        assert_eq!(back, value);
    }

    #[test]
    fn msgpack_that_json_cannot_hold_is_refused_with_a_reason() {
        let binary = to_json(&Mp::Binary(vec![1, 2])).unwrap_err();
        assert!(binary.contains("binary"), "{binary}");

        let key = to_json(&Mp::Map(vec![(Mp::from(1), Mp::Nil)])).unwrap_err();
        assert!(key.contains("key"), "{key}");

        let ext = to_json(&Mp::Ext(5, vec![0])).unwrap_err();
        assert!(ext.contains("extension"), "{ext}");

        // 入れ子の中でも見つける
        let nested = to_json(&Mp::Array(vec![Mp::Binary(vec![])])).unwrap_err();
        assert!(nested.contains("binary"), "{nested}");
    }

    #[test]
    fn a_non_finite_float_becomes_null() {
        assert_eq!(to_json(&Mp::F64(f64::NAN)), Ok(Js::Null));
        assert_eq!(to_json(&Mp::F32(f32::INFINITY)), Ok(Js::Null));
        assert_eq!(to_json(&Mp::F32(1.5)), Ok(json!(1.5)));
    }
}
