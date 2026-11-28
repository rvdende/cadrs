//! Tape -> FeatureScript. A case becomes one function value that builds its model in a fresh
//! context with the cqrt runtime and returns Parasolid's measures; a batch is a lambda running
//! several cases, for Onshape's evalFeatureScript.

use crate::{selector, sigs};
use serde_json::Value;
use std::fmt::Write;

pub const RUNTIME: &str = include_str!("../fs/cqrt.fs");

#[derive(Debug)]
pub struct Translated {
    pub i: i64,
    /// the case function's source (`const case_<i> = function(c, base) {...};`: the case built in
    /// context `c` under the id `base`, its measures returned, `result_q` its result's bodies)
    pub fs: String,
    pub guarded: Vec<i64>,
}

fn num(x: f64) -> String {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        // shortest round-trip form; FeatureScript reads exponents
        let s = format!("{x:?}");
        s
    }
}

fn string(s: &str) -> String {
    let mut o = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => {}
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// A tape value as a FeatureScript literal. `ctx` names where it is (for messages).
pub fn lit(v: &Value, ctx: &str) -> Result<String, String> {
    Ok(match v {
        Value::Null => "undefined".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => num(n.as_f64().ok_or("number")?),
        Value::String(s) => string(s),
        Value::Array(a) => {
            let parts: Vec<String> = a.iter().map(|x| lit(x, ctx)).collect::<Result<_, _>>()?;
            format!("[{}]", parts.join(", "))
        }
        Value::Object(m) => {
            if let Some(vec) = m.get("vec") {
                let a = vec.as_array().ok_or("vec")?;
                let parts: Vec<String> = a.iter().map(|x| lit(x, ctx)).collect::<Result<_, _>>()?;
                format!("vector({})", parts.join(", "))
            } else if let Some(p) = m.get("plane") {
                format!(
                    "{{\"origin\": {}, \"xDir\": {}, \"zDir\": {}}}",
                    lit(&p["origin"], ctx)?,
                    lit(&p["xDir"], ctx)?,
                    lit(&p["zDir"], ctx)?
                )
            } else if let Some(l) = m.get("loc") {
                format!(
                    "{{\"loc\": {{\"R\": {}, \"t\": {}}}}}",
                    lit(&l["R"], ctx)?,
                    lit(&l["t"], ctx)?
                )
            } else if let Some(w) = m.get("wp") {
                format!("{{\"wp\": {}}}", w)
            } else if let Some(s) = m.get("shape") {
                format!("{{\"shape\": {}}}", s)
            } else if let Some(s) = m.get("sel") {
                sel_lit(&selector::compile_obj(s)?)
            } else if m.contains_key("dict") {
                return Err(format!("a dict as {ctx}"));
            } else {
                // an already-compiled selector map, or a nested plain map
                let mut parts = vec![];
                for (k, x) in m {
                    parts.push(format!("{}: {}", string(k), lit(x, ctx)?));
                }
                format!("{{{}}}", parts.join(", "))
            }
        }
    })
}

fn sel_lit(v: &Value) -> String {
    // compiled selectors are plain JSON maps: emit them verbatim as FeatureScript maps
    lit(v, "selector").expect("selector literal")
}

/// Selector parameters may be a string or a recorded selector object.
fn selector_arg(v: &Value) -> Result<String, String> {
    match v {
        Value::Null => Ok("undefined".into()),
        Value::String(s) => Ok(sel_lit(&selector::compile_str(s)?)),
        Value::Object(m) if m.contains_key("sel") => {
            Ok(sel_lit(&selector::compile_obj(&m["sel"])?))
        }
        _ => Err(format!("selector argument {v}")),
    }
}

fn args_map(sig: &sigs::Sig, e: &Value) -> Result<String, String> {
    let args = e["args"].as_array().cloned().unwrap_or_default();
    let kwargs = e["kwargs"].as_object().cloned().unwrap_or_default();
    let bound = sigs::bind(sig, &args, &kwargs)?;
    let mut parts = vec![];
    for (k, v) in bound {
        let s = if k == "selector" {
            selector_arg(&v)?
        } else {
            lit(&v, &format!("{}.{}", sig.op, k))?
        };
        parts.push(format!("{}: {}", string(&k), s));
    }
    Ok(format!("{{{}}}", parts.join(", ")))
}

fn ref_id(v: &Value, key: &str) -> Result<i64, String> {
    v.get(key)
        .and_then(|x| x.as_i64())
        .ok_or_else(|| format!("expected a {key} reference, got {v}"))
}

/// Translate one recorded generator.
pub fn translate(rec: &Value) -> Result<Translated, String> {
    let i = rec["i"].as_i64().ok_or("i")?;
    if rec["status"] != "ok" {
        return Err(format!(
            "tape status {}: {}",
            rec["status"],
            rec["reason"].as_str().unwrap_or("")
        ));
    }
    let mut body = String::new();
    let mut guarded = vec![];
    let entries = rec["entries"].as_array().ok_or("entries")?;
    for (n, e) in entries.iter().enumerate() {
        let op = e["op"].as_str().ok_or("op")?;
        let step = n as i64 + 1;
        if e.get("guarded").and_then(|g| g.as_bool()) == Some(true) {
            guarded.push(step);
        }
        // The recorder's marks (try bodies, a replayed failure): comments, nothing to run.
        if op.starts_with("guard_") || op == "injected_failure" {
            let _ = writeln!(
                body,
                "        // step {step}: {op} {}",
                e.to_string().replace('\n', " ")
            );
            continue;
        }
        let _ = writeln!(body, "        atStep(st, {step});");
        match op {
            "Workplane" => {
                let out = ref_id(&e["out"], "wp")?;
                let plane = lit(&serde_json::json!({"plane": e["plane"]}), "Workplane plane")?;
                let obj = lit(&e["obj"], "Workplane obj")?;
                let _ = writeln!(
                    body,
                    "        cqWorkplaneNew(c, st, {out}, {{\"plane\": {plane}, \"obj\": {obj}}});"
                );
            }
            "val" => {
                let out = ref_id(&e["out"], "shape")?;
                let me = ref_id(&e["self"], "wp")?;
                let _ = writeln!(body, "        cqVal(c, st, {out}, {me});");
            }
            _ => {
                let sig = sigs::find(op).ok_or_else(|| format!("op {op} not ported"))?;
                let a = args_map(sig, e)?;
                if op.starts_with("Solid.") || op.starts_with("Compound.") {
                    let out = ref_id(&e["out"], "shape")?;
                    let _ = writeln!(body, "        {}(c, st, {out}, {a});", sig.fs);
                } else if op.starts_with("shape.") {
                    let out = ref_id(&e["out"], "shape")?;
                    let me = ref_id(&e["self"], "shape")?;
                    let _ = writeln!(body, "        {}(c, st, {out}, {me}, {a});", sig.fs);
                } else {
                    let out = ref_id(&e["out"], "wp")?;
                    let me = ref_id(&e["self"], "wp")?;
                    let _ = writeln!(body, "        {}(c, st, {out}, {me}, {a});", sig.fs);
                    if sig.solid {
                        let _ = writeln!(body, "        stepVol(c, st, {out}, {});", string(op));
                    }
                }
            }
        }
    }
    let result = lit(&rec["result"], "result")?;
    let fs = format!(
        "const case_{i} = function(c, base) {{\n    var st = newState(base);\n    try {{\n{body}        return mergeMaps(cqMeasure(c, st, {result}), {{\"i\": {i}}});\n    }} catch (e) {{\n        return {{\"i\": {i}, \"status\": \"error\", \"at\": st[].cur, \"err\": toString(e), \"log\": st[].log, \"warn\": st[].warn}};\n    }}\n}};\n"
    );
    Ok(Translated { i, fs, guarded })
}

/// An evalFeatureScript lambda running `cases`, returning {"c<i>": result}.
pub fn batch(cases: &[&Translated]) -> String {
    let mut s = String::from("function(context is Context, queries) {\n");
    s.push_str(RUNTIME);
    s.push('\n');
    for t in cases {
        s.push_str(&t.fs);
    }
    s.push_str("    var out = {};\n");
    for t in cases {
        let _ = writeln!(
            s,
            "    {{ var r = case_{i}(newContextWithDefaults(), makeId(\"q\")); r.result_q = undefined; out[\"c{i}\"] = r; }}",
            i = t.i
        );
    }
    s.push_str("    return out;\n}\n");
    s
}

/// How far apart the generator feature puts the cases' results along X (mm): each case is
/// moved `slot * SLOT` from where it was built.
pub const SLOT: f64 = 2000.0;

/// A Feature Studio's source: the runtime and the cases at top level, and one feature,
/// "cq2fs cases", that builds the cases its `cases` parameter lists (comma separated) in the
/// Part Studio, deletes whatever else each made, moves each result `SLOT` mm further along X
/// than the one before and names its parts `c<i>.<n>`, for one STEP export of them all.
/// `version`: the FeatureScript version, as a new Feature Studio's first line gives it.
pub fn feature_studio(cases: &[&Translated], version: &str) -> String {
    let mut s = format!(
        "FeatureScript {version};\nimport(path : \"onshape/std/geometry.fs\", version : \"{version}.0\");\n\n// Generated by cq2fs (crates/cadrs_cq2fs): CADEvolve generators translated from CadQuery.\n\n"
    );
    s.push_str(RUNTIME);
    s.push('\n');
    for t in cases {
        s.push_str(&t.fs);
    }
    s.push_str("const CASES = {");
    let entries: Vec<String> = cases
        .iter()
        .map(|t| format!("\"{}\" : case_{}", t.i, t.i))
        .collect();
    s.push_str(&entries.join(", "));
    s.push_str("};\n\n");
    let _ = write!(
        s,
        r#"annotation {{ "Feature Type Name" : "cq2fs cases" }}
export const cq2fsCases = defineFeature(function(context is Context, id is Id, definition is map)
    precondition
    {{
        annotation {{ "Name" : "Cases" }}
        definition.cases is string;
    }}
    {{
        const list = splitByRegexp(definition.cases, ",");
        for (var k = 0; k < size(list); k += 1)
        {{
            const i = list[k];
            const base = id + ("c" ~ i);
            var r = {{ "status" : "error" }};
            try silent {{ r = CASES[i](context, base); }}
            const made = qCreatedBy(base, EntityType.BODY);
            var keep = qNothing();
            if (r.status == "ok") keep = r.result_q;
            const rest = qSubtraction(made, keep);
            if (!isQueryEmpty(context, rest)) opDeleteBodies(context, base + "clean", {{ "entities" : rest }});
            if (!isQueryEmpty(context, keep))
            {{
                opTransform(context, base + "slot", {{ "bodies" : keep, "transform" : transform(vector({slot}, 0, 0) * k * millimeter) }});
                var n = 0;
                for (var b in evaluateQuery(context, keep))
                {{
                    n += 1;
                    setProperty(context, {{ "entities" : b, "propertyType" : PropertyType.NAME, "value" : "c" ~ i ~ "." ~ n }});
                }}
            }}
        }}
    }});
"#,
        slot = SLOT
    );
    s
}
