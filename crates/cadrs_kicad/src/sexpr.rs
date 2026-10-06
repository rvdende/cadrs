//! KiCad's S-expressions: `(name arg arg (child …) …)`, atoms bare or `"quoted"` with
//! backslash escapes.

use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Sexp {
    List(Vec<Sexp>),
    /// A bare atom: a keyword or number.
    Atom(String),
    /// A quoted string.
    Str(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for ParseError {}

/// Parses one expression (the file's root list).
pub fn parse(text: &str) -> Result<Sexp, ParseError> {
    let b = text.as_bytes();
    let mut i = 0;
    let mut stack: Vec<Vec<Sexp>> = vec![];
    let err = |offset, m: &str| ParseError { offset, message: m.into() };
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            return Err(err(i, "unexpected end of file"));
        }
        let item = match b[i] {
            b'(' => {
                stack.push(vec![]);
                i += 1;
                continue;
            }
            b')' => {
                i += 1;
                Sexp::List(stack.pop().ok_or_else(|| err(i - 1, "unbalanced ')'"))?)
            }
            b'"' => {
                i += 1;
                let mut s = Vec::new();
                loop {
                    match b.get(i) {
                        None => return Err(err(i, "unterminated string")),
                        Some(b'"') => break,
                        Some(b'\\') => {
                            i += 1;
                            match b.get(i) {
                                Some(b'n') => s.push(b'\n'),
                                Some(b't') => s.push(b'\t'),
                                Some(b'r') => s.push(b'\r'),
                                Some(&c) => s.push(c),
                                None => return Err(err(i, "unterminated string")),
                            }
                        }
                        Some(&c) => s.push(c),
                    }
                    i += 1;
                }
                i += 1;
                Sexp::Str(String::from_utf8_lossy(&s).into_owned())
            }
            _ => {
                let start = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'(' && b[i] != b')' {
                    i += 1;
                }
                Sexp::Atom(String::from_utf8_lossy(&b[start..i]).into_owned())
            }
        };
        match stack.last_mut() {
            Some(top) => top.push(item),
            None => return Ok(item),
        }
    }
}

impl Sexp {
    /// The list's items (empty for atoms).
    pub fn items(&self) -> &[Sexp] {
        match self {
            Sexp::List(v) => v,
            _ => &[],
        }
    }

    /// The list's first atom: its keyword.
    pub fn head(&self) -> &str {
        match self.items().first() {
            Some(Sexp::Atom(s)) => s,
            _ => "",
        }
    }

    /// An atom's or string's text.
    pub fn text(&self) -> Option<&str> {
        match self {
            Sexp::Atom(s) | Sexp::Str(s) => Some(s),
            Sexp::List(_) => None,
        }
    }

    /// The argument after the keyword, 0-based.
    pub fn arg(&self, i: usize) -> Option<&Sexp> {
        self.items().get(i + 1)
    }

    pub fn str_arg(&self, i: usize) -> Option<&str> {
        self.arg(i).and_then(Sexp::text)
    }

    pub fn f64_arg(&self, i: usize) -> Option<f64> {
        self.str_arg(i).and_then(|s| s.parse().ok())
    }

    /// The child lists with keyword `name`.
    pub fn all<'a, 'n>(&'a self, name: &'n str) -> impl Iterator<Item = &'a Sexp> + use<'a, 'n> {
        self.items().iter().filter(move |c| matches!(c, Sexp::List(_)) && c.head() == name)
    }

    /// The first child list with keyword `name`.
    pub fn find(&self, name: &str) -> Option<&Sexp> {
        self.all(name).next()
    }

    /// The child lists.
    pub fn children(&self) -> impl Iterator<Item = &Sexp> {
        self.items().iter().filter(|c| matches!(c, Sexp::List(_)))
    }

    /// `(name value …)`'s first argument as text.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.find(name).and_then(|c| c.str_arg(0))
    }

    pub fn get_f64(&self, name: &str) -> Option<f64> {
        self.find(name).and_then(|c| c.f64_arg(0))
    }

    /// A yes/no flag in any of KiCad's spellings: `(name yes)`, `(name no)`, `(name)` or a bare
    /// `name` atom among the arguments. `None` when absent.
    pub fn flag(&self, name: &str) -> Option<bool> {
        for c in self.items().iter().skip(1) {
            match c {
                Sexp::Atom(a) if a == name => return Some(true),
                Sexp::List(_) if c.head() == name => {
                    return Some(!matches!(c.str_arg(0), Some("no" | "false")));
                }
                _ => {}
            }
        }
        None
    }

    /// Whether a bare atom is among the arguments (`(attr smd exclude_from_bom)`).
    pub fn has_atom(&self, name: &str) -> bool {
        self.items().iter().skip(1).any(|c| matches!(c, Sexp::Atom(a) if a == name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_lists_and_strings() {
        let s = parse(r#"(kicad_sch (version 20250114) (text "a \"b\"\nc" (at 1.5 -2 90)) (hide yes) bold)"#).unwrap();
        assert_eq!(s.head(), "kicad_sch");
        assert_eq!(s.get("version"), Some("20250114"));
        let t = s.find("text").unwrap();
        assert_eq!(t.str_arg(0), Some("a \"b\"\nc"));
        assert_eq!(t.find("at").unwrap().f64_arg(1), Some(-2.0));
        assert_eq!(s.flag("hide"), Some(true));
        assert_eq!(s.flag("bold"), Some(true));
        assert_eq!(s.flag("italic"), None);
    }

    #[test]
    fn reports_errors() {
        assert!(parse("(a (b)").is_err());
        assert!(parse("(a \"b)").is_err());
        assert!(parse(")").is_err());
    }
}
