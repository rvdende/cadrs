//! Onshape expressions (`25 mm`, `#width * 2`, `sqrt(#a^2 + #b^2)`, `#h / tan(#angle)`).
//!
//! Values carry their dimension: a power of length (mm) and whether they are an angle
//! (radians). Units are values to multiply by (`9.6*mm`, `2 in`); a number right before a unit
//! word is multiplied by it. Functions: sqrt, abs, sin, cos, tan, asin, acos, atan, atan2, min,
//! max, floor, ceil, round, exp, log, log10; constants PI, pi. Variables (`#name`) are
//! looked up (and evaluated, recursively) in a table of expressions.

use std::collections::HashMap;

/// A value: `v` in mm^len (angles in radians).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Q {
    pub v: f64,
    pub len: i32,
    pub angle: bool,
}

impl Q {
    fn num(v: f64) -> Self {
        Self { v, len: 0, angle: false }
    }
}

/// Looks up a variable's expression.
pub trait Vars {
    fn expr(&self, name: &str) -> Option<String>;
}

impl Vars for HashMap<String, String> {
    fn expr(&self, name: &str) -> Option<String> {
        self.get(name).cloned()
    }
}

/// Evaluates an expression.
pub fn eval(text: &str, vars: &dyn Vars) -> Result<Q, String> {
    eval_depth(text, vars, 0)
}

fn eval_depth(text: &str, vars: &dyn Vars, depth: usize) -> Result<Q, String> {
    if depth > 32 {
        return Err("variables refer to each other in a loop".into());
    }
    let toks = tokenize(text)?;
    let mut p = P { t: &toks, i: 0, vars, depth };
    let v = p.sum()?;
    if p.i != toks.len() {
        return Err(format!("unexpected {:?}", toks[p.i]));
    }
    Ok(v)
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Word(String),
    Var(String),
    Op(char),
}

fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch.is_ascii_digit() || (ch == '.' && c.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.') {
                i += 1;
            }
            // Exponent: 1e-3, 2.5E+4.
            if i < c.len() && (c[i] == 'e' || c[i] == 'E') {
                let mut j = i + 1;
                if j < c.len() && (c[j] == '+' || c[j] == '-') {
                    j += 1;
                }
                if j < c.len() && c[j].is_ascii_digit() {
                    i = j;
                    while i < c.len() && c[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let t: String = c[start..i].iter().collect();
            out.push(Tok::Num(t.parse().map_err(|_| format!("bad number {t}"))?));
        } else if ch == '#' {
            i += 1;
            let start = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            out.push(Tok::Var(c[start..i].iter().collect()));
        } else if ch.is_alphabetic() || ch == '_' {
            let start = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            out.push(Tok::Word(c[start..i].iter().collect()));
        } else if ch == '"' {
            out.push(Tok::Word("in".into()));
            i += 1;
        } else if ch == '°' {
            out.push(Tok::Word("deg".into()));
            i += 1;
        } else if "+-*/^(),".contains(ch) {
            out.push(Tok::Op(ch));
            i += 1;
        } else {
            return Err(format!("unexpected {ch:?}"));
        }
    }
    Ok(out)
}

/// A unit word's value.
fn unit(w: &str) -> Option<Q> {
    let len = |mm: f64| Q { v: mm, len: 1, angle: false };
    Some(match w {
        "mm" | "millimeter" | "millimeters" => len(1.0),
        "cm" | "centimeter" | "centimeters" => len(10.0),
        "m" | "meter" | "meters" => len(1000.0),
        "in" | "inch" | "inches" => len(25.4),
        "ft" | "foot" | "feet" => len(304.8),
        "yd" | "yard" | "yards" => len(914.4),
        "deg" | "degree" | "degrees" => Q { v: std::f64::consts::PI / 180.0, len: 0, angle: true },
        "rad" | "radian" | "radians" => Q { v: 1.0, len: 0, angle: true },
        _ => return None,
    })
}

struct P<'a> {
    t: &'a [Tok],
    i: usize,
    vars: &'a dyn Vars,
    depth: usize,
}

impl P<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(&Tok::Op(c)) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn sum(&mut self) -> Result<Q, String> {
        let mut a = self.product()?;
        loop {
            let sign = if self.eat('+') {
                1.0
            } else if self.eat('-') {
                -1.0
            } else {
                return Ok(a);
            };
            let b = self.product()?;
            a = add(a, Q { v: sign * b.v, ..b })?;
        }
    }

    fn product(&mut self) -> Result<Q, String> {
        let mut a = self.unary()?;
        loop {
            if self.eat('*') {
                let b = self.unary()?;
                a = mul(a, b);
            } else if self.eat('/') {
                let b = self.unary()?;
                a = mul(a, Q { v: 1.0 / b.v, len: -b.len, angle: b.angle });
                // An angle divided by an angle is a plain number.
                if b.angle {
                    a.angle = false;
                }
            } else if matches!(self.peek(), Some(Tok::Word(w)) if unit(w).is_some()) {
                // `2 in`, `(3 + 4) mm`: juxtaposed units multiply.
                let Some(Tok::Word(w)) = self.peek().cloned() else { unreachable!() };
                self.i += 1;
                a = mul(a, unit(&w).expect("a unit"));
            } else {
                return Ok(a);
            }
        }
    }

    fn unary(&mut self) -> Result<Q, String> {
        if self.eat('-') {
            let v = self.unary()?;
            return Ok(Q { v: -v.v, ..v });
        }
        if self.eat('+') {
            return self.unary();
        }
        self.power()
    }

    fn power(&mut self) -> Result<Q, String> {
        let base = self.atom()?;
        if self.eat('^') {
            let e = self.unary()?;
            if e.len != 0 {
                return Err("an exponent must be a plain number".into());
            }
            let n = e.v;
            let len = (base.len as f64) * n;
            if (len - len.round()).abs() > 1e-9 {
                return Err("fractional power of a length".into());
            }
            return Ok(Q { v: base.v.powf(n), len: len.round() as i32, angle: base.angle && n == 1.0 });
        }
        Ok(base)
    }

    fn atom(&mut self) -> Result<Q, String> {
        match self.peek().cloned() {
            Some(Tok::Num(v)) => {
                self.i += 1;
                Ok(Q::num(v))
            }
            Some(Tok::Var(name)) => {
                self.i += 1;
                let e = self.vars.expr(&name).ok_or_else(|| format!("unknown variable #{name}"))?;
                eval_depth(&e, self.vars, self.depth + 1)
            }
            Some(Tok::Op('(')) => {
                self.i += 1;
                let v = self.sum()?;
                if !self.eat(')') {
                    return Err("missing )".into());
                }
                Ok(v)
            }
            Some(Tok::Word(w)) => {
                self.i += 1;
                if let Some(u) = unit(&w) {
                    return Ok(u);
                }
                match w.as_str() {
                    "PI" | "pi" => return Ok(Q::num(std::f64::consts::PI)),
                    "true" => return Ok(Q::num(1.0)),
                    "false" => return Ok(Q::num(0.0)),
                    _ => {}
                }
                if !self.eat('(') {
                    return Err(format!("unknown word {w}"));
                }
                let mut args = vec![self.sum()?];
                while self.eat(',') {
                    args.push(self.sum()?);
                }
                if !self.eat(')') {
                    return Err("missing )".into());
                }
                call(&w, &args)
            }
            other => Err(format!("unexpected {other:?}")),
        }
    }
}

fn add(a: Q, b: Q) -> Result<Q, String> {
    // A plain number added to a length counts as mm (as Onshape's inputs do), to an angle as
    // the same kind.
    if a.len == b.len && a.angle == b.angle {
        return Ok(Q { v: a.v + b.v, ..a });
    }
    if a.len == 0 && !a.angle {
        return Ok(Q { v: a.v + b.v, ..b });
    }
    if b.len == 0 && !b.angle {
        return Ok(Q { v: a.v + b.v, ..a });
    }
    Err("adding different kinds of quantity".into())
}

fn mul(a: Q, b: Q) -> Q {
    Q { v: a.v * b.v, len: a.len + b.len, angle: a.angle ^ b.angle }
}

fn call(f: &str, a: &[Q]) -> Result<Q, String> {
    let one = || -> Result<Q, String> { a.first().copied().ok_or_else(|| format!("{f} needs an argument")) };
    let plain = |v: f64| Q::num(v);
    let radians = |q: Q| q.v; // an angle is in radians; a plain number is taken as radians too
    Ok(match f {
        "sqrt" => {
            let x = one()?;
            if x.len % 2 != 0 {
                return Err("square root of an odd power of length".into());
            }
            Q { v: x.v.sqrt(), len: x.len / 2, angle: false }
        }
        "abs" => {
            let x = one()?;
            Q { v: x.v.abs(), ..x }
        }
        "sin" => plain(radians(one()?).sin()),
        "cos" => plain(radians(one()?).cos()),
        "tan" => plain(radians(one()?).tan()),
        "asin" => Q { v: one()?.v.asin(), len: 0, angle: true },
        "acos" => Q { v: one()?.v.acos(), len: 0, angle: true },
        "atan" => Q { v: one()?.v.atan(), len: 0, angle: true },
        "atan2" => {
            let (y, x) = (a.first().ok_or("atan2 needs two arguments")?, a.get(1).ok_or("atan2 needs two arguments")?);
            Q { v: y.v.atan2(x.v), len: 0, angle: true }
        }
        "min" | "max" => {
            let mut it = a.iter().copied();
            let first = it.next().ok_or_else(|| format!("{f} needs arguments"))?;
            it.fold(first, |m, x| if (f == "min") == (x.v < m.v) { x } else { m })
        }
        "floor" => {
            let x = one()?;
            Q { v: x.v.floor(), ..x }
        }
        "ceil" => {
            let x = one()?;
            Q { v: x.v.ceil(), ..x }
        }
        "round" => {
            let x = one()?;
            Q { v: x.v.round(), ..x }
        }
        "exp" => plain(one()?.v.exp()),
        "log" => plain(one()?.v.ln()),
        "log10" => plain(one()?.v.log10()),
        _ => return Err(format!("unknown function {f}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> HashMap<String, String> {
        [
            ("stair_count", "10"),
            ("landing_height", "1325 mm"),
            ("stair_angle", "30 deg"),
            ("riser_height", "#landing_height / #stair_count"),
            ("tread_depth", "#riser_height / tan(#stair_angle)"),
            ("step_pitch", "sqrt(#tread_depth^2 + #riser_height^2)"),
        ]
        .into_iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
    }

    #[test]
    fn units_and_arithmetic() {
        let v = HashMap::new();
        assert_eq!(eval("25 mm", &v).unwrap(), Q { v: 25.0, len: 1, angle: false });
        assert!((eval("9.6*mm", &v).unwrap().v - 9.6).abs() < 1e-12);
        assert!((eval("1 in + 5 mm", &v).unwrap().v - 30.4).abs() < 1e-12);
        assert!((eval("(3 + 4) cm", &v).unwrap().v - 70.0).abs() < 1e-12);
        assert!((eval("0.687 m", &v).unwrap().v - 687.0).abs() < 1e-9);
        assert!((eval("90 deg / 2", &v).unwrap().v - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
    }

    #[test]
    fn variables_and_functions() {
        let v = vars();
        let r = 132.5;
        let t = r / (30f64.to_radians()).tan();
        let p = eval("#step_pitch", &v).unwrap();
        assert_eq!(p.len, 1);
        assert!((p.v - (t * t + r * r).sqrt()).abs() < 1e-9);
        assert_eq!(eval("#stair_count", &v).unwrap().len, 0);
        assert!(eval("#nope", &v).is_err());
    }
}
