//! Her action's data, mended where its meaning is plain (the SDK's best practices: validate
//! everything, never assume the data matches the schema): a field under another common name, a
//! number or a yes as a string, an enum's value in capitals, the whole object sent as a JSON
//! string. Read against the action's own registered schema, so it holds for every action as
//! registered. What can't be mended is left for the action's checks to refuse, with the reason.

use serde_json::{Map, Value};

/// Other names she might give a field, each with the fields it may stand for (the first of them
/// the action has, and hasn't been given, is taken).
const ALIASES: &[(&str, &[&str])] = &[
    ("id", &["ref"]),
    ("ref", &["target"]),
    ("target", &["ref"]),
    ("object", &["ref", "target"]),
    ("item", &["ref"]),
    ("area", &["target", "ref"]),
    ("direction", &["target"]),
    ("switch", &["ref"]),
    ("door", &["ref"]),
    ("exit", &["ref"]),
    ("monster", &["ref"]),
    ("weapon", &["ref"]),
    ("gun", &["ref"]),
    ("type", &["what"]),
    ("kind", &["what"]),
    ("list", &["what"]),
    ("topic", &["what"]),
    ("action", &["do"]),
    ("mode", &["do"]),
    ("command", &["do"]),
    ("answer", &["choice"]),
    ("option", &["choice"]),
    ("letter", &["choice"]),
    ("question", &["id"]),
    ("question_id", &["id"]),
    ("level", &["map"]),
    ("difficulty", &["skill"]),
    ("thoroughness", &["thorough"]),
    ("secs", &["seconds"]),
    ("duration", &["seconds"]),
];

/// The data mended against `schema` (an action's registered `schema`, None if it takes none),
/// and a note for each field taken under another name ("id read as ref"), to be told her with
/// the result so she gives it right next time.
pub fn repair(data: Value, schema: Option<&Value>) -> (Value, Vec<String>) {
    // (the object sent as a string of JSON)
    let data = match data {
        Value::String(s) => serde_json::from_str::<Value>(&s)
            .ok()
            .filter(Value::is_object)
            .unwrap_or(Value::String(s)),
        other => other,
    };
    let (Value::Object(given), Some(props)) = (
        &data,
        schema
            .and_then(|s| s.get("properties"))
            .and_then(Value::as_object),
    ) else {
        return (data, Vec::new());
    };
    let mut out: Map<String, Value> = Map::new();
    let mut notes = Vec::new();
    // (the fields under their own names first, so an alias never takes one that's given)
    let keyed: Vec<(String, &Value)> = given
        .iter()
        .map(|(k, v)| (k.trim().to_lowercase().replace([' ', '-'], "_"), v))
        .collect();
    for (k, v) in &keyed {
        if props.contains_key(k) {
            out.insert(k.clone(), (*v).clone());
        }
    }
    for (k, v) in &keyed {
        if props.contains_key(k) {
            continue;
        }
        let to = ALIASES
            .iter()
            .find(|(alias, _)| alias == k)
            .and_then(|(_, fields)| {
                fields
                    .iter()
                    .find(|f| props.contains_key(**f) && !out.contains_key(**f))
            });
        if let Some(f) = to {
            notes.push(format!("{k} read as {f}"));
            out.insert((*f).to_string(), (*v).clone());
        }
    }
    for (k, v) in out.iter_mut() {
        if let Some(p) = props.get(k) {
            *v = typed(v.take(), p);
        }
    }
    (Value::Object(out), notes)
}

/// A value made the type its property says, where that's plain; left as it is otherwise.
fn typed(v: Value, prop: &Value) -> Value {
    let v = match (prop.get("type").and_then(Value::as_str), v) {
        (Some("boolean"), Value::String(s)) => match s.trim().to_lowercase().as_str() {
            "true" | "yes" | "on" | "1" => Value::Bool(true),
            "false" | "no" | "off" | "0" => Value::Bool(false),
            _ => Value::String(s),
        },
        (Some("boolean"), Value::Number(n)) if n.as_f64() == Some(1.0) => Value::Bool(true),
        (Some("boolean"), Value::Number(n)) if n.as_f64() == Some(0.0) => Value::Bool(false),
        (Some("integer"), Value::String(s)) => match s.trim().parse::<f64>() {
            Ok(f) if f.is_finite() => Value::from(f.round() as i64),
            _ => Value::String(s),
        },
        (Some("integer"), Value::Number(n)) if !n.is_i64() && !n.is_u64() => n
            .as_f64()
            .map_or(Value::Number(n), |f| Value::from(f.round() as i64)),
        (Some("string"), Value::Number(n)) => Value::String(n.to_string()),
        (Some("string"), Value::Bool(b)) => Value::String(b.to_string()),
        (_, v) => v,
    };
    // (an enum's value in another case, or with spaces round it)
    match (prop.get("enum").and_then(Value::as_array), &v) {
        (Some(options), Value::String(s)) => {
            let want = s.trim().to_lowercase().replace([' ', '-'], "_");
            options
                .iter()
                .find(|o| o.as_str() == Some(want.as_str()))
                .cloned()
                .unwrap_or(v)
        }
        _ => v,
    }
}

#[cfg(test)]
mod tests {
    use super::repair;
    use crate::neuro::session::actions;
    use serde_json::{json, Value};

    fn schema(name: &str) -> Option<Value> {
        actions()
            .into_iter()
            .find(|a| a["name"] == name)
            .and_then(|a| a.get("schema").cloned())
    }

    #[test]
    fn mends_what_is_plain() {
        // (a field under another name, a yes as a string)
        let (v, notes) = repair(
            json!({ "id": "door-3", "Rush": "yes" }),
            schema("doom_go").as_ref(),
        );
        assert_eq!(v, json!({ "ref": "door-3", "rush": true }));
        assert_eq!(notes, vec!["id read as ref"]);
        // (doom_answer's own id stays its id; its choice under another name)
        let (v, _) = repair(
            json!({ "id": "q3", "answer": "b" }),
            schema("doom_answer").as_ref(),
        );
        assert_eq!(v, json!({ "id": "q3", "choice": "b" }));
        // (doom_name's ref and name both its own)
        let (v, notes) = repair(
            json!({ "ref": "area-2", "name": "pit" }),
            schema("doom_name").as_ref(),
        );
        assert_eq!(v, json!({ "ref": "area-2", "name": "pit" }));
        assert!(notes.is_empty());
        // (numbers as strings; an enum in capitals; the whole object as a string)
        let (v, _) = repair(
            Value::String(r#"{"target": "N", "thorough": "3"}"#.into()),
            schema("doom_search").as_ref(),
        );
        assert_eq!(v, json!({ "target": "N", "thorough": 3 }));
        let (v, _) = repair(
            json!({ "do": "Hold Fire", "seconds": 4.6 }),
            schema("doom_fight").as_ref(),
        );
        assert_eq!(v, json!({ "do": "hold_fire", "seconds": 5 }));
        let (v, _) = repair(json!({ "what": "Doors" }), schema("doom_list").as_ref());
        assert_eq!(v, json!({ "what": "doors" }));
        // (an id given as a number)
        let (v, _) = repair(json!({ "ref": 12 }), schema("doom_inspect").as_ref());
        assert_eq!(v, json!({ "ref": "12" }));
    }

    #[test]
    fn leaves_what_is_not() {
        // (a field given under its own name isn't overwritten by an alias)
        let (v, notes) = repair(json!({ "ref": "a", "id": "b" }), schema("doom_go").as_ref());
        assert_eq!(v, json!({ "ref": "a" }));
        assert!(notes.is_empty());
        // (a value that isn't one of the enum's, or a word that isn't a number, left for the
        // action to refuse)
        let (v, _) = repair(
            json!({ "what": "barrels", "all": "maybe" }),
            schema("doom_list").as_ref(),
        );
        assert_eq!(v, json!({ "what": "barrels", "all": "maybe" }));
        // (an action with no schema: as given)
        assert_eq!(repair(json!({ "x": 1 }), None).0, json!({ "x": 1 }));
    }
}
