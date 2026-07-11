//! BSON ↔ legacy extended JSON conversion.
//!
//! The Python services serialize Mongo documents with `bson.json_util.dumps`
//! (legacy mode): `ObjectId` → `{"$oid": hex}`, `datetime` → `{"$date":
//! epoch-millis-int}`. The gateway speaks exactly that dialect to the WASM
//! components (and additionally accepts canonical `$date` forms on input).

use mongodb::bson::{Bson, DateTime as BsonDateTime, Document};
use serde_json::{json, Map, Value};

pub fn json_to_bson(value: &Value) -> Bson {
    match value {
        Value::Null => Bson::Null,
        Value::Bool(b) => Bson::Boolean(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Bson::Int64(i)
            } else {
                Bson::Double(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::String(s) => Bson::String(s.clone()),
        Value::Array(items) => Bson::Array(items.iter().map(json_to_bson).collect()),
        Value::Object(map) => {
            // {"$oid": "..."}
            if map.len() == 1 {
                if let Some(Value::String(hex)) = map.get("$oid") {
                    if let Ok(oid) = mongodb::bson::oid::ObjectId::parse_str(hex) {
                        return Bson::ObjectId(oid);
                    }
                }
                if let Some(date) = map.get("$date") {
                    if let Some(bson_date) = parse_date(date) {
                        return Bson::DateTime(bson_date);
                    }
                }
                if let Some(Value::String(n)) = map.get("$numberLong") {
                    if let Ok(i) = n.parse::<i64>() {
                        return Bson::Int64(i);
                    }
                }
            }
            let mut doc = Document::new();
            for (key, val) in map {
                doc.insert(key.clone(), json_to_bson(val));
            }
            Bson::Document(doc)
        }
    }
}

fn parse_date(value: &Value) -> Option<BsonDateTime> {
    if let Some(millis) = value.as_i64() {
        return Some(BsonDateTime::from_millis(millis));
    }
    if let Some(nl) = value.get("$numberLong").and_then(Value::as_str) {
        return nl.parse::<i64>().ok().map(BsonDateTime::from_millis);
    }
    if let Some(iso) = value.as_str() {
        return chrono::DateTime::parse_from_rfc3339(iso)
            .ok()
            .map(|dt| BsonDateTime::from_millis(dt.timestamp_millis()));
    }
    None
}

pub fn json_to_document(value: &Value) -> Document {
    match json_to_bson(value) {
        Bson::Document(doc) => doc,
        _ => Document::new(),
    }
}

pub fn bson_to_json(bson: &Bson) -> Value {
    match bson {
        Bson::Null | Bson::Undefined => Value::Null,
        Bson::Boolean(b) => json!(b),
        Bson::Int32(i) => json!(i),
        Bson::Int64(i) => json!(i),
        Bson::Double(f) => json!(f),
        Bson::String(s) => json!(s),
        Bson::ObjectId(oid) => json!({ "$oid": oid.to_hex() }),
        Bson::DateTime(dt) => json!({ "$date": dt.timestamp_millis() }),
        Bson::Array(items) => Value::Array(items.iter().map(bson_to_json).collect()),
        Bson::Document(doc) => document_to_json(doc),
        Bson::Decimal128(d) => json!({ "$numberDecimal": d.to_string() }),
        Bson::Timestamp(ts) => json!({ "$timestamp": { "t": ts.time, "i": ts.increment } }),
        Bson::RegularExpression(re) => {
            json!({ "$regex": re.pattern.clone(), "$options": re.options.clone() })
        }
        // Not used by these services; stringify rather than invent an encoding.
        other => json!(other.to_string()),
    }
}

pub fn document_to_json(doc: &Document) -> Value {
    let mut map = Map::new();
    for (key, value) in doc {
        map.insert(key.clone(), bson_to_json(value));
    }
    Value::Object(map)
}
