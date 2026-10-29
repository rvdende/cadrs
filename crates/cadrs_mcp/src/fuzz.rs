//! Hostile parameters for the commands, made from their JSON schemas ([`crate::commands`]): the
//! right shape with extreme values (huge, tiny, negative, empty, odd names), wrong types, missing
//! and unknown fields. The fuzz tests (here and the app's) call every command with them.

use serde_json::{Map, Value, json};

/// A small seeded generator (xorshift), so a failing case replays from its seed.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// A number in `0..n` (0 for an empty range).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

const NUMBERS: &[f64] = &[
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.5,
    10.0,
    360.0,
    1e-12,
    -1e-12,
    1e9,
    -1e9,
    1e300,
    -1e300,
    f64::MAX,
    f64::MIN,
    f64::MIN_POSITIVE,
    9_007_199_254_740_993.0,
];

const INTEGERS: &[i64] = &[
    0,
    1,
    -1,
    2,
    3,
    100,
    65_535,
    i32::MAX as i64,
    i64::MAX,
    i64::MIN,
];

/// Names of what the fuzz tests' document has, and odd ones.
const STRINGS: &[&str] = &[
    "",
    " ",
    "Part Studio 1",
    "Assembly 1",
    "Sketch 1",
    "Extrude 1",
    "Part 1",
    "Top",
    "Front",
    "Right",
    "Origin",
    "x",
    "../../etc/passwd",
    "💥\u{0}\u{202e}",
    "NaN",
    "-1",
    "1e999",
];

/// A stable hash of a name (FNV-1a), to give each command a generator of its own.
pub fn hash(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
    })
}

/// `value` (a valid one, such as a feature's defaults) with some of its numbers, strings and
/// flags made extreme or odd, and some fields left out or of the wrong type.
pub fn mutate(value: &Value, rng: &mut Rng) -> Value {
    match value {
        Value::Object(o) => {
            let mut out = Map::new();
            for (k, v) in o {
                if rng.below(10) != 0 {
                    out.insert(k.clone(), mutate(v, rng));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => {
            let mut items: Vec<Value> = items.iter().map(|v| mutate(v, rng)).collect();
            if rng.below(4) == 0 {
                items.push(items.last().cloned().unwrap_or_else(|| wrong(rng)));
            }
            Value::Array(items)
        }
        _ if rng.below(12) == 0 => wrong(rng),
        Value::Number(_) if rng.below(2) == 0 => json!(rng.pick(NUMBERS)),
        Value::String(_) if rng.below(3) == 0 => json!(rng.pick(STRINGS)),
        Value::Bool(b) if rng.below(3) == 0 => json!(!b),
        v => v.clone(),
    }
}

/// Parameters for a command whose parameters have this JSON schema.
pub fn params(schema: &Value, rng: &mut Rng) -> Value {
    if rng.below(10) == 0 {
        return wrong(rng);
    }
    value(schema, schema, rng, 0)
}

/// A value of a random type, whatever was asked for.
fn wrong(rng: &mut Rng) -> Value {
    match rng.below(7) {
        0 => Value::Null,
        1 => json!(rng.pick(NUMBERS)),
        2 => json!(rng.pick(INTEGERS)),
        3 => json!(rng.pick(STRINGS)),
        4 => json!([]),
        5 => json!(true),
        _ => json!({ "unknown": 1 }),
    }
}

/// A value for `schema` (whose `$ref`s point into `root`), now and then the wrong one.
fn value(schema: &Value, root: &Value, rng: &mut Rng, depth: u32) -> Value {
    if depth > 6 || rng.below(15) == 0 {
        return wrong(rng);
    }
    if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
        let name = r.rsplit('/').next().unwrap_or_default();
        let def = root
            .get("$defs")
            .or_else(|| root.get("definitions"))
            .and_then(|d| d.get(name));
        return match def {
            Some(d) => value(d, root, rng, depth + 1),
            None => wrong(rng),
        };
    }
    for key in ["oneOf", "anyOf", "allOf"] {
        if let Some(Value::Array(options)) = schema.get(key)
            && !options.is_empty()
        {
            return value(rng.pick(options), root, rng, depth + 1);
        }
    }
    if let Some(Value::Array(values)) = schema.get("enum")
        && !values.is_empty()
    {
        return rng.pick(values).clone();
    }
    if let Some(c) = schema.get("const") {
        return c.clone();
    }
    let ty = match schema.get("type") {
        Some(Value::String(t)) => t.as_str(),
        Some(Value::Array(types)) if !types.is_empty() => {
            rng.pick(types).as_str().unwrap_or("null")
        }
        _ if schema.get("properties").is_some() => "object",
        _ => return wrong(rng),
    };
    match ty {
        "object" => {
            let mut o = Map::new();
            if let Some(Value::Object(props)) = schema.get("properties") {
                let required: Vec<&str> = schema
                    .get("required")
                    .and_then(Value::as_array)
                    .map(|r| r.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                for (k, s) in props {
                    // The required fields nearly always, the others half the time.
                    let give = if required.contains(&k.as_str()) {
                        rng.below(10) != 0
                    } else {
                        rng.below(2) == 0
                    };
                    if give {
                        o.insert(k.clone(), value(s, root, rng, depth + 1));
                    }
                }
            }
            if rng.below(10) == 0 {
                o.insert("unexpected".into(), wrong(rng));
            }
            Value::Object(o)
        }
        "array" => {
            let n = match rng.below(6) {
                5 if depth < 2 => 40,
                k => k.min(3),
            };
            let item = schema.get("items").unwrap_or(&Value::Null);
            Value::Array((0..n).map(|_| value(item, root, rng, depth + 1)).collect())
        }
        "string" if rng.below(30) == 0 => json!("x".repeat(10_000)),
        "string" => json!(rng.pick(STRINGS)),
        "integer" => json!(rng.pick(INTEGERS)),
        "number" => json!(rng.pick(NUMBERS)),
        "boolean" => json!(rng.below(2) == 0),
        "null" => Value::Null,
        _ => wrong(rng),
    }
}
