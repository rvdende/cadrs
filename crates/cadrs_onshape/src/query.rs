//! Decoder for the `qCompressed(...)` query strings in Onshape feature JSON.
//!
//! A feature parameter that picks geometry (an extrude's regions, a fillet's edges, a sketch's
//! plane) stores a `BTMIndividualQuery` whose `queryString` is
//! `query=qCompressed(1.0,"<payload>",true)`. The payload is plain (`%…`) or zlib and base64
//! (`&<hex length>$<base64>`). Decoded, it is Onshape's history-based name for the entity: what
//! created it (`operationId`, `queryType` such as `SKETCH_ENTITY`, `CAP_FACE`,
//! `SWEPT_FACE`…) plus disambiguation data. `cadrs` names faces and edges the same way
//! (`cadrs_kernel::naming`), which is what makes the references translatable.
//!
//! The serialization, as reverse-engineered from the scraped documents (every query in them
//! decodes):
//!
//! ```text
//! value := 'M' hex (value value)*    map of n key/value pairs
//!        | 'A' hex value*            array of n values
//!        | 'S' segs '$' chars        string: '.'-separated segments, each a hex length of new
//!                                    characters or '-'hex, a name-table back reference
//!        | 'E' hex                   a string from the name table
//!        | 'R' hex                   a value-table back reference
//!        | 'B' hex '$' chars value   a new type name, then a value of that type
//!        | 'C' hex value             a value of a type from the name table
//!        | 'D' number                a number
//!        | 'T' | 'F' | 'N'           true, false, null
//! ```
//!
//! Two back-reference tables:
//! - the **name table** (`C`, `E`, `-n` segments): every type name, every new string segment
//!   and every joined multi-segment string, in the order read;
//! - the **value table** (`R`): every value when it finishes (strings including map keys,
//!   numbers, untyped maps and arrays, typed values), except booleans and nulls, and except a
//!   typed value's own payload (the typed value is entered instead).

use std::io::Read;

use base64::Engine;

/// A decoded query value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Keys in their serialized order.
    Map(Vec<(String, Value)>),
    Array(Vec<Value>),
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
    /// A value of a named type (`Query`, `Id`, `EntityType`, …).
    Typed(String, Box<Value>),
}

impl Value {
    /// The field `key` of a map (or of a typed map).
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            Value::Typed(_, v) => v.get(key),
            _ => None,
        }
    }

    /// The string, looking through a typed wrapper (`EntityType("FACE")` gives `"FACE"`). An
    /// `Id` gives its joined path.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            Value::Typed(_, v) => v.as_str(),
            Value::Array(a) if a.len() == 1 => a[0].as_str(),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The items of an array (an empty slice otherwise).
    pub fn items(&self) -> &[Value] {
        match self {
            Value::Array(a) => a,
            Value::Typed(_, v) => v.items(),
            _ => &[],
        }
    }

    /// The type name of a typed value.
    pub fn type_name(&self) -> Option<&str> {
        match self {
            Value::Typed(t, _) => Some(t),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cannot decode query: {}", self.0)
    }
}

impl std::error::Error for DecodeError {}

fn err<T>(msg: impl Into<String>) -> Result<T, DecodeError> {
    Err(DecodeError(msg.into()))
}

/// The serialized payload of a `qCompressed(...)` query string, or `None` for other query
/// forms (such as `qSketchRegion(id + "<feature>", true)`).
pub fn payload(query_string: &str) -> Result<Option<String>, DecodeError> {
    const START: &str = "qCompressed(1.0,\"";
    let Some(i) = query_string.find(START) else {
        return Ok(None);
    };
    let rest = &query_string[i + START.len()..];
    let Some(end) = rest.rfind('"') else {
        return err("unterminated qCompressed");
    };
    let s = &rest[..end];
    if let Some(z) = s.strip_prefix('&') {
        let Some((_, b64)) = z.split_once('$') else {
            return err("compressed payload without '$'");
        };
        let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
            .decode(b64.trim_end_matches('='))
            .map_err(|e| DecodeError(format!("base64: {e}")))?;
        let mut out = String::new();
        flate2::read::ZlibDecoder::new(&bytes[..])
            .read_to_string(&mut out)
            .map_err(|e| DecodeError(format!("zlib: {e}")))?;
        Ok(Some(out))
    } else {
        Ok(Some(s.to_string()))
    }
}

/// Decodes a `qCompressed(...)` query string; `None` for other query forms.
pub fn decode(query_string: &str) -> Result<Option<Value>, DecodeError> {
    let Some(p) = payload(query_string)? else {
        return Ok(None);
    };
    let Some(body) = p.strip_prefix('%') else {
        return err(format!("payload does not start with '%': {:.20}", p));
    };
    let mut r = Reader { s: body.as_bytes(), i: 0, names: Vec::new(), values: Vec::new() };
    let v = r.value(false)?;
    if r.i != r.s.len() {
        return err(format!("{} trailing bytes", r.s.len() - r.i));
    }
    Ok(Some(v))
}

struct Reader<'a> {
    s: &'a [u8],
    i: usize,
    names: Vec<String>,
    values: Vec<Value>,
}

impl Reader<'_> {
    fn peek(&self) -> Result<u8, DecodeError> {
        self.s.get(self.i).copied().ok_or_else(|| DecodeError("unexpected end".into()))
    }

    fn next(&mut self) -> Result<u8, DecodeError> {
        let c = self.peek()?;
        self.i += 1;
        Ok(c)
    }

    /// A signed hex integer.
    fn hex(&mut self) -> Result<i64, DecodeError> {
        let neg = self.peek()? == b'-';
        if neg {
            self.i += 1;
        }
        let start = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_hexdigit() && !self.s[self.i].is_ascii_uppercase() {
            self.i += 1;
        }
        let t = std::str::from_utf8(&self.s[start..self.i]).unwrap_or_default();
        let n = i64::from_str_radix(t, 16).map_err(|_| DecodeError(format!("bad hex at {start}")))?;
        Ok(if neg { -n } else { n })
    }

    fn number(&mut self) -> Result<f64, DecodeError> {
        let start = self.i;
        let digit = |i: usize| self.s.get(i).is_some_and(u8::is_ascii_digit);
        if self.s.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.') {
            self.i += 1;
        }
        // An exponent only when digits follow: 'E' is also the name-table tag.
        if matches!(self.s.get(self.i), Some(b'e' | b'E')) {
            let sign = usize::from(matches!(self.s.get(self.i + 1), Some(b'-' | b'+')));
            if digit(self.i + 1 + sign) {
                self.i += 1 + sign;
                while digit(self.i) {
                    self.i += 1;
                }
            }
        }
        let t = std::str::from_utf8(&self.s[start..self.i]).unwrap_or_default();
        t.parse().map_err(|_| DecodeError(format!("bad number {t:?}")))
    }

    fn chars(&mut self, n: usize) -> Result<String, DecodeError> {
        let end = self.i + n;
        if end > self.s.len() {
            return err("string runs past the end");
        }
        let out = String::from_utf8_lossy(&self.s[self.i..end]).into_owned();
        self.i = end;
        Ok(out)
    }

    fn name(&self, n: i64) -> Result<String, DecodeError> {
        usize::try_from(n)
            .ok()
            .and_then(|n| self.names.get(n))
            .cloned()
            .ok_or_else(|| DecodeError(format!("name {n} out of range")))
    }

    fn string(&mut self) -> Result<String, DecodeError> {
        let mut segs = Vec::new();
        loop {
            segs.push(self.hex()?);
            match self.next()? {
                b'$' => break,
                b'.' => {}
                c => return err(format!("bad string separator {:?}", c as char)),
            }
        }
        let mut parts = Vec::with_capacity(segs.len());
        for n in segs {
            if n < 0 {
                parts.push(self.name(-n)?);
            } else {
                let p = self.chars(n as usize)?;
                self.names.push(p.clone());
                parts.push(p);
            }
        }
        let joined = parts.join(".");
        if parts.len() > 1 {
            self.names.push(joined.clone());
        }
        Ok(joined)
    }

    fn value(&mut self, typed_payload: bool) -> Result<Value, DecodeError> {
        let v = match self.next()? {
            b'R' => {
                let n = self.hex()?;
                return usize::try_from(n)
                    .ok()
                    .and_then(|n| self.values.get(n))
                    .cloned()
                    .ok_or_else(|| DecodeError(format!("value {n} out of range")));
            }
            b'M' => {
                let n = self.hex()?;
                let mut m = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    let k = match self.value(false)? {
                        Value::Str(s) => s,
                        other => format!("{other:?}"),
                    };
                    let v = self.value(false)?;
                    m.push((k, v));
                }
                Value::Map(m)
            }
            b'A' => {
                let n = self.hex()?;
                let mut a = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    a.push(self.value(false)?);
                }
                Value::Array(a)
            }
            b'S' => Value::Str(self.string()?),
            b'E' => {
                let n = self.hex()?;
                Value::Str(self.name(n)?)
            }
            c @ (b'B' | b'C') => {
                let name = if c == b'B' {
                    let n = self.hex()?;
                    if self.next()? != b'$' {
                        return err("type name without '$'");
                    }
                    let name = self.chars(n as usize)?;
                    self.names.push(name.clone());
                    name
                } else {
                    let n = self.hex()?;
                    self.name(n)?
                };
                Value::Typed(name, Box::new(self.value(true)?))
            }
            b'D' => Value::Num(self.number()?),
            b'T' => return Ok(Value::Bool(true)),
            b'F' => return Ok(Value::Bool(false)),
            b'N' => return Ok(Value::Null),
            c => return err(format!("unknown tag {:?} at {}", c as char, self.i - 1)),
        };
        if !typed_payload {
            self.values.push(v.clone());
        }
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sketch 1's plane in a scraped document: the Front plane.
    const FRONT: &str = r#"query=qCompressed(1.0,"%B5$QueryM4Sa$entityTypeBa$EntityTypeS4$FACESb$historyTypeS8$CREATIONSb$operationIdB2$IdA1S5.7$FrontplaneOpS9$queryTypeS5$DUMMY",true)"#;

    /// An extrude's region: the face the sketch's imprint made, disambiguated by adjacent
    /// edges (with back references to whole queries and numbers).
    const REGION: &str = r#"query=qCompressed(1.0,"%B5$QueryM5S12$disambiguationDataA1M2S12$disambiguationTypeS8$TOPOLOGYS8$entitiesA1A2C0M5Sb$derivedFromC0M6R0A1M2R1S13$ORIGINAL_DEPENDENCYS9$originalsA1C0M5Sa$entityTypeBa$EntityTypeS4$EDGESb$historyTypeS8$CREATIONSb$operationIdB2$IdA1S11.6$FJsdj390kbY00vE_1wireOpS9$queryTypeSd$SKETCH_ENTITYSe$sketchEntityIdSc$r15giJ5DaqKRR7R8R9RaS7$isStartFRbCeA1S11.9$FLD6lWkCIjLlONs_1opExtrudeReS8$CAP_EDGER7R8R9RaRbCeA1S11.7$FszbPo4NarCEC97_1imprintReS7$IMPRINTD1R7C9S4$FACER9RaRbR1cReR1d",true)"#;

    #[test]
    fn plain_query() {
        let q = decode(FRONT).unwrap().unwrap();
        assert_eq!(q.type_name(), Some("Query"));
        assert_eq!(q.get("entityType").and_then(Value::as_str), Some("FACE"));
        assert_eq!(q.get("operationId").and_then(Value::as_str), Some("Front.planeOp"));
        assert_eq!(q.get("queryType").and_then(Value::as_str), Some("DUMMY"));
    }

    #[test]
    fn back_references() {
        let q = decode(REGION).unwrap().unwrap();
        // The outer query: the FACE the sketch imprint made.
        assert_eq!(q.get("entityType").and_then(Value::as_str), Some("FACE"));
        assert_eq!(q.get("operationId").and_then(Value::as_str), Some("FszbPo4NarCEC97_1.imprint"));
        assert_eq!(q.get("queryType").and_then(Value::as_str), Some("IMPRINT"));
        // Its one adjacent edge: [query, 1], derived from the cap edge of the extruded
        // sketch entity.
        let dd = &q.get("disambiguationData").unwrap().items()[0];
        assert_eq!(dd.get("disambiguationType").and_then(Value::as_str), Some("TOPOLOGY"));
        let pair = &dd.get("entities").unwrap().items()[0];
        assert_eq!(pair.items()[1], Value::Num(1.0));
        let edge = pair.items()[0].get("derivedFrom").unwrap();
        assert_eq!(edge.get("queryType").and_then(Value::as_str), Some("CAP_EDGE"));
        assert_eq!(edge.get("isStart"), Some(&Value::Bool(false)));
        let original = &edge.get("disambiguationData").unwrap().items()[0].get("originals").unwrap().items()[0];
        assert_eq!(original.get("sketchEntityId").and_then(Value::as_str), Some("r15giJ5DaqKR"));
    }

    #[test]
    fn other_query_forms() {
        assert_eq!(decode(r#"query = qSketchRegion(id + "FNP2rXizZqnAwTP_1", true);"#).unwrap(), None);
    }
}
