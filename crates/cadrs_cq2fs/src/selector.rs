//! CadQuery's string selector grammar (selectors.py `_makeGrammar`, `_makeExpressionGrammar`) and its
//! selector classes, compiled to the form cqrt.fs `selFilter` evaluates:
//!
//! - `{"k": "dir"|"par"|"perp", "v": [x,y,z]}`          Direction / ParallelDir / PerpendicularDir
//! - `{"k": "type", "ty": "PLANE"}`                     TypeSelector
//! - `{"k": "nth", "key": "center"|"radius"|"length"|"area", "v", "n", "max", "parallel"}`
//!   _NthSelector family; DirectionMinMax is CenterNth with n = -1, DirectionNth adds the
//!   ParallelDir filter first
//! - `{"k": "and"|"or"|"exc", "a", "b"}`, `{"k": "not", "a"}`
//!
//! Precedence is pyparsing's `infix_notation` order: `and` binds tightest, then `or`, then
//! `exc`/`except`, then `not` (the loosest, unary, right-associative).

use serde_json::{Value, json};

const TYPES: &[&str] = &[
    "LINE",
    "CIRCLE",
    "ELLIPSE",
    "HYPERBOLA",
    "PARABOLA",
    "BEZIER",
    "BSPLINE",
    "OFFSET",
    "OTHER",
    "PLANE",
    "CYLINDER",
    "CONE",
    "SPHERE",
    "TORUS",
    "REVOLUTION",
    "EXTRUSION",
];

fn axis(name: &str) -> Option<[f64; 3]> {
    Some(match name {
        "X" => [1.0, 0.0, 0.0],
        "Y" => [0.0, 1.0, 0.0],
        "Z" => [0.0, 0.0, 1.0],
        "XY" => [1.0, 1.0, 0.0],
        "YZ" => [0.0, 1.0, 1.0],
        "XZ" => [1.0, 0.0, 1.0],
        _ => return None,
    })
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> P<'a> {
    fn ws(&mut self) {
        while self.i < self.s.len() && (self.s[self.i] as char).is_whitespace() {
            self.i += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }
    fn eat(&mut self, lit: &str) -> bool {
        self.ws();
        if self.s[self.i..].starts_with(lit.as_bytes()) {
            self.i += lit.len();
            true
        } else {
            false
        }
    }
    /// a keyword: the literal not followed by an identifier character
    fn keyword(&mut self, kw: &str) -> bool {
        self.ws();
        let save = self.i;
        if self.eat(kw) {
            let next = self.s.get(self.i).copied();
            if next.is_none_or(|c| !(c as char).is_ascii_alphanumeric() && c != b'_') {
                return true;
            }
        }
        self.i = save;
        false
    }
    fn number(&mut self) -> Result<f64, String> {
        self.ws();
        let st = self.i;
        if matches!(self.s.get(self.i), Some(b'+') | Some(b'-')) {
            self.i += 1;
        }
        while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.') {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[st..self.i])
            .unwrap()
            .parse::<f64>()
            .map_err(|e| format!("number: {e}"))
    }
    fn direction(&mut self) -> Result<[f64; 3], String> {
        self.ws();
        if self.eat("(") {
            let x = self.number()?;
            if !self.eat(",") {
                return Err("expected ,".into());
            }
            let y = self.number()?;
            if !self.eat(",") {
                return Err("expected ,".into());
            }
            let z = self.number()?;
            if !self.eat(")") {
                return Err("expected )".into());
            }
            return Ok([x, y, z]);
        }
        // one_of(['X','Y','Z','XY','XZ','YZ']): longest match first
        for name in ["XY", "XZ", "YZ", "X", "Y", "Z"] {
            let save = self.i;
            if self.eat(name) {
                return Ok(axis(name).unwrap());
            }
            self.i = save;
        }
        Err(format!("expected a direction at {}", self.i))
    }
    fn index(&mut self) -> Result<Option<i64>, String> {
        if !self.eat("[") {
            return Ok(None);
        }
        let neg = self.eat("-");
        self.ws();
        let st = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
            self.i += 1;
        }
        let n: i64 = std::str::from_utf8(&self.s[st..self.i])
            .unwrap()
            .parse()
            .map_err(|e| format!("index: {e}"))?;
        if !self.eat("]") {
            return Err("expected ]".into());
        }
        Ok(Some(if neg { -n } else { n }))
    }

    fn atom(&mut self) -> Result<Value, String> {
        // `not` may stand wherever an operand may, and takes all the rest of the expression
        // (checked against pyparsing: "|X and not |Y or |Z" is (|X and (not (|Y or |Z))))
        if self.keyword("not") {
            let a = self.expr()?;
            return Ok(json!({"k": "not", "a": a}));
        }
        // infix_notation tries the base grammar (where "(x,y,z)" is a direction) before "( expr )"
        if self.peek() == Some(b'(') {
            let save = self.i;
            if let Ok(v) = self.direction() {
                return Ok(json!({"k": "dir", "v": v}));
            }
            self.i = save;
        }
        if self.eat("(") {
            let e = self.expr()?;
            if !self.eat(")") {
                return Err("expected )".into());
            }
            return Ok(e);
        }
        for (view, v, max) in [
            ("front", [0.0, 0.0, 1.0], true),
            ("back", [0.0, 0.0, 1.0], false),
            ("left", [1.0, 0.0, 0.0], false),
            ("right", [1.0, 0.0, 0.0], true),
            ("top", [0.0, 1.0, 0.0], true),
            ("bottom", [0.0, 1.0, 0.0], false),
        ] {
            if self.keyword(view) {
                return Ok(
                    json!({"k": "nth", "key": "center", "v": v, "n": -1, "max": max, "parallel": false}),
                );
            }
        }
        if self.eat("%") {
            self.ws();
            let st = self.i;
            while self.i < self.s.len() && (self.s[self.i] as char).is_ascii_alphabetic() {
                self.i += 1;
            }
            let t = std::str::from_utf8(&self.s[st..self.i])
                .unwrap()
                .to_uppercase();
            if !TYPES.contains(&t.as_str()) {
                return Err(format!("unknown type %{t}"));
            }
            return Ok(json!({"k": "type", "ty": t}));
        }
        if self.eat(">>") || self.eat("<<") {
            let max = self.s[self.i - 2] == b'>';
            let v = self.direction()?;
            let n = self.index()?.unwrap_or(-1);
            return Ok(
                json!({"k": "nth", "key": "center", "v": v, "n": n, "max": max, "parallel": false}),
            );
        }
        if self.eat(">") || self.eat("<") {
            let max = self.s[self.i - 1] == b'>';
            let v = self.direction()?;
            return Ok(match self.index()? {
                Some(n) => {
                    json!({"k": "nth", "key": "center", "v": v, "n": n, "max": max, "parallel": true})
                }
                None => {
                    json!({"k": "nth", "key": "center", "v": v, "n": -1, "max": max, "parallel": false})
                }
            });
        }
        for (op, k, sign) in [
            ("|", "par", 1.0),
            ("#", "perp", 1.0),
            ("+", "dir", 1.0),
            ("-", "dir", -1.0),
        ] {
            if self.eat(op) {
                let v = self.direction()?;
                return Ok(json!({"k": k, "v": [v[0] * sign, v[1] * sign, v[2] * sign]}));
            }
        }
        let v = self.direction()?;
        Ok(json!({"k": "dir", "v": v}))
    }
    fn and(&mut self) -> Result<Value, String> {
        let mut a = self.atom()?;
        while self.keyword("and") {
            let b = self.atom()?;
            a = json!({"k": "and", "a": a, "b": b});
        }
        Ok(a)
    }
    fn or(&mut self) -> Result<Value, String> {
        let mut a = self.and()?;
        while self.keyword("or") {
            let b = self.and()?;
            a = json!({"k": "or", "a": a, "b": b});
        }
        Ok(a)
    }
    fn exc(&mut self) -> Result<Value, String> {
        let mut a = self.or()?;
        while self.keyword("except") || self.keyword("exc") {
            let b = self.or()?;
            a = json!({"k": "exc", "a": a, "b": b});
        }
        Ok(a)
    }
    fn expr(&mut self) -> Result<Value, String> {
        if self.keyword("not") {
            let a = self.expr()?;
            return Ok(json!({"k": "not", "a": a}));
        }
        self.exc()
    }
}

/// Compile a CadQuery selector string.
pub fn compile_str(s: &str) -> Result<Value, String> {
    let mut p = P {
        s: s.as_bytes(),
        i: 0,
    };
    let v = p.expr()?;
    p.ws();
    if p.i != s.len() {
        return Err(format!("selector {s:?}: unparsed input at {}", p.i));
    }
    Ok(v)
}

fn vec3(v: &Value) -> Result<Value, String> {
    let a = v.get("vec").cloned().unwrap_or_else(|| v.clone());
    let arr = a.as_array().ok_or("selector vector")?;
    let mut out: Vec<f64> = arr.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect();
    if out.len() == 2 {
        out.push(0.0);
    }
    if out.len() != 3 || out.iter().any(|x| x.is_nan()) {
        return Err(format!("selector vector {a}"));
    }
    Ok(json!(out))
}

/// Compile a recorded selector object (`cq.selectors.X(...)`).
pub fn compile_obj(o: &Value) -> Result<Value, String> {
    let kind = o["kind"].as_str().unwrap_or("");
    if kind == "combo" {
        let parts: Vec<Value> = o["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(compile_obj)
            .collect::<Result<_, _>>()?;
        let op = o["op"].as_str().unwrap();
        return Ok(if op == "not" {
            json!({"k": "not", "a": parts[0]})
        } else {
            json!({"k": op, "a": parts[0], "b": parts[1]})
        });
    }
    let args = o["args"].as_array().cloned().unwrap_or_default();
    let kw = o["kwargs"].as_object().cloned().unwrap_or_default();
    let arg = |i: usize, name: &str| -> Option<Value> {
        kw.get(name).cloned().or_else(|| args.get(i).cloned())
    };
    if let Some(t) = arg(99, "tolerance")
        && t.as_f64() != Some(0.0001)
    {
        return Err("selector with a non-default tolerance".into());
    }
    let flag =
        |i: usize, name: &str, def: bool| arg(i, name).and_then(|v| v.as_bool()).unwrap_or(def);
    Ok(match kind {
        "StringSyntaxSelector" => compile_str(
            arg(0, "selectorString")
                .and_then(|v| v.as_str().map(String::from))
                .ok_or("selector string")?
                .as_str(),
        )?,
        "DirectionSelector" => json!({"k": "dir", "v": vec3(&arg(0, "vector").ok_or("vector")?)?}),
        "ParallelDirSelector" => {
            json!({"k": "par", "v": vec3(&arg(0, "vector").ok_or("vector")?)?})
        }
        "PerpendicularDirSelector" => {
            json!({"k": "perp", "v": vec3(&arg(0, "vector").ok_or("vector")?)?})
        }
        "TypeSelector" => {
            json!({"k": "type", "ty": arg(0, "typeString").and_then(|v| v.as_str().map(|s| s.to_uppercase())).ok_or("type")?})
        }
        "DirectionMinMaxSelector" => {
            json!({"k": "nth", "key": "center", "v": vec3(&arg(0, "vector").ok_or("vector")?)?, "n": -1, "max": flag(1, "directionMax", true), "parallel": false})
        }
        "DirectionNthSelector" => {
            json!({"k": "nth", "key": "center", "v": vec3(&arg(0, "vector").ok_or("vector")?)?,
            "n": arg(1, "n").and_then(|v| v.as_i64()).ok_or("n")?, "max": flag(2, "directionMax", true), "parallel": true})
        }
        "CenterNthSelector" => {
            json!({"k": "nth", "key": "center", "v": vec3(&arg(0, "vector").ok_or("vector")?)?,
            "n": arg(1, "n").and_then(|v| v.as_i64()).ok_or("n")?, "max": flag(2, "directionMax", true), "parallel": false})
        }
        "RadiusNthSelector" | "LengthNthSelector" | "AreaNthSelector" => {
            let key = match kind {
                "RadiusNthSelector" => "radius",
                "LengthNthSelector" => "length",
                _ => "area",
            };
            json!({"k": "nth", "key": key, "v": [0, 0, 1], "n": arg(0, "n").and_then(|v| v.as_i64()).ok_or("n")?, "max": flag(1, "directionMax", true), "parallel": false})
        }
        other => return Err(format!("selector {other} not ported")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammar() {
        assert_eq!(
            compile_str(">Z").unwrap(),
            json!({"k":"nth","key":"center","v":[0.0,0.0,1.0],"n":-1,"max":true,"parallel":false})
        );
        assert_eq!(compile_str("<Z[1]").unwrap()["parallel"], json!(true));
        assert_eq!(compile_str(">>Y[-2]").unwrap()["n"], json!(-2));
        assert_eq!(
            compile_str("|Z").unwrap(),
            json!({"k":"par","v":[0.0,0.0,1.0]})
        );
        assert_eq!(
            compile_str("-X").unwrap(),
            json!({"k":"dir","v":[-1.0,-0.0,-0.0]})
        );
        assert_eq!(
            compile_str("%circle").unwrap(),
            json!({"k":"type","ty":"CIRCLE"})
        );
        assert_eq!(compile_str(">XY").unwrap()["v"], json!([1.0, 1.0, 0.0]));
        assert_eq!(
            compile_str("(1,0,-1)").unwrap(),
            json!({"k":"dir","v":[1.0,0.0,-1.0]})
        );
        // and binds tighter than or
        let e = compile_str(">Z or <Z and |X").unwrap();
        assert_eq!(e["k"], "or");
        assert_eq!(e["b"]["k"], "and");
        // not is the loosest
        let n = compile_str("not >Z or <Z").unwrap();
        assert_eq!(n["k"], "not");
        assert_eq!(n["a"]["k"], "or");
        assert_eq!(compile_str("|Z exc >X").unwrap()["k"], "exc");
        assert!(compile_str(">Q").is_err());
    }
}
