//! Numbers typed into fields (GS24): arithmetic with units, `1.62+2*0.3`, `100 mil`,
//! `0.1in + 1mm`, `(4.7)/2`. A number without a unit is in the field's default unit.

use crate::units::{MIL, NM_PER_MM, Nm};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Mm,
    Mil,
    Inch,
}

impl Unit {
    fn nm(self) -> f64 {
        match self {
            Unit::Mm => NM_PER_MM as f64,
            Unit::Mil => MIL as f64,
            Unit::Inch => MIL as f64 * 1000.0,
        }
    }
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
    default: Unit,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }

    /// sum := term (('+'|'-') term)*
    fn sum(&mut self) -> Result<f64, String> {
        let mut v = self.term()?;
        while let Some(c @ (b'+' | b'-')) = self.peek() {
            self.i += 1;
            let r = self.term()?;
            v = if c == b'+' { v + r } else { v - r };
        }
        Ok(v)
    }

    /// term := factor (('*'|'/') factor)*  (lengths in nm; a product keeps one length)
    fn term(&mut self) -> Result<f64, String> {
        let (mut v, mut has_unit) = self.factor()?;
        while let Some(c @ (b'*' | b'/')) = self.peek() {
            self.i += 1;
            let (r, ru) = self.factor()?;
            if c == b'*' {
                // Scalars multiply lengths; the default unit is applied once, at the end.
                v *= r;
                has_unit |= ru;
            } else {
                if r == 0.0 {
                    return Err("division by zero".into());
                }
                v /= r;
            }
        }
        Ok(if has_unit { v } else { v * self.default.nm() })
    }

    /// factor := '-' factor | '(' sum ')' unit? | number unit?   → (value, carries a unit)
    fn factor(&mut self) -> Result<(f64, bool), String> {
        match self.peek() {
            Some(b'-') => {
                self.i += 1;
                let (v, u) = self.factor()?;
                Ok((-v, u))
            }
            Some(b'(') => {
                self.i += 1;
                // The inner sum is already in nm; strip the default unit back off if no unit
                // follows, so `(1+2)*3` stays a length in the default unit.
                let v = self.sum()? / self.default.nm();
                if self.peek() != Some(b')') {
                    return Err("missing )".into());
                }
                self.i += 1;
                Ok(self.unit(v))
            }
            _ => {
                self.ws();
                let start = self.i;
                while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.') {
                    self.i += 1;
                }
                let n: f64 = std::str::from_utf8(&self.s[start..self.i]).unwrap().parse().map_err(|_| format!("not a number at {}", start + 1))?;
                Ok(self.unit(n))
            }
        }
    }

    fn unit(&mut self, v: f64) -> (f64, bool) {
        self.ws();
        let rest = std::str::from_utf8(&self.s[self.i..]).unwrap_or("");
        for (word, u) in [("mils", Unit::Mil), ("mil", Unit::Mil), ("mm", Unit::Mm), ("in", Unit::Inch), ("\"", Unit::Inch), ("th", Unit::Mil)] {
            if rest.starts_with(word) {
                self.i += word.len();
                return (v * u.nm(), true);
            }
        }
        (v, false)
    }
}

/// Evaluates `text` to nanometres; plain numbers are in `default`.
pub fn eval(text: &str, default: Unit) -> Result<Nm, String> {
    let mut p = P { s: text.as_bytes(), i: 0, default };
    let v = p.sum()?;
    if p.peek().is_some() {
        return Err(format!("unexpected '{}'", &text[p.i..]));
    }
    Ok(v.round() as Nm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::mm;

    #[test]
    fn expressions() {
        assert_eq!(eval("1.62+2*0.3", Unit::Mm), Ok(mm(2.22)));
        assert_eq!(eval("1.62 + 2*0.15", Unit::Mm), Ok(mm(1.92)));
        assert_eq!(eval("100 mil", Unit::Mm), Ok(mm(2.54)));
        assert_eq!(eval("-200", Unit::Mil), Ok(-mm(5.08)));
        assert_eq!(eval("0.1in + 1mm", Unit::Mm), Ok(mm(3.54)));
        assert_eq!(eval("(1+2)*3", Unit::Mm), Ok(mm(9.0)));
        assert_eq!(eval("13/2", Unit::Mm), Ok(mm(6.5)));
        assert_eq!(eval("4.7 - 13/2", Unit::Mm), Ok(mm(-1.8)));
        assert!(eval("1/0", Unit::Mm).is_err());
        assert!(eval("2 apples", Unit::Mm).is_err());
    }
}
