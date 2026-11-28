//! CadQuery's selectors (`cadquery/selectors.py`): the objects (`ParallelDirSelector`, …) and
//! the string syntax (`">Z"`, `"|X and not %CIRCLE"`, `">Z[-2]"`, `">>Y[1]"`, `"%PLANE"`, …).

use super::shape::{Shape, ShapeType};
use super::{CqError, CqResult, value_err};
use crate::native::math::V3;

/// What a direction selector compares: a face's normal (planar faces) or an edge's direction
/// (straight edges); other shapes are skipped.
fn test_vector(s: &Shape) -> Option<V3> {
    match s.shape_type() {
        ShapeType::Face if s.geom_type().name() == "PLANE" => s.normal_at(None).ok(),
        ShapeType::Edge if s.geom_type().name() == "LINE" => s.tangent_at(0.5, true).ok(),
        _ => None,
    }
}

fn angle(a: &V3, b: &V3) -> f64 {
    let (na, nb) = (a.norm(), b.norm());
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    // OCCT's gp_Vec::Angle.
    let c = a.dot(b) / (na * nb);
    let s = a.cross(b).norm() / (na * nb);
    s.atan2(c)
}

/// What an Nth selector sorts by.
#[derive(Debug, Clone, PartialEq)]
pub enum NthKey {
    /// The centre's distance along a direction.
    Center(V3),
    /// The radius (edges and wires).
    Radius,
    /// The length (edges and wires).
    Length,
    /// The area (faces, shells, solids; closed planar wires).
    Area,
}

/// A selector.
#[derive(Debug, Clone, PartialEq)]
#[allow(missing_docs)]
pub enum Selector {
    /// Everything (`Selector()`).
    All,
    /// Normals or directions along `dir` (`DirectionSelector`, `+X`).
    Direction { dir: V3, tol: f64 },
    /// Parallel to `dir` either way (`ParallelDirSelector`, `|X`).
    Parallel { dir: V3, tol: f64 },
    /// Square to `dir` (`PerpendicularDirSelector`, `#X`).
    Perpendicular { dir: V3, tol: f64 },
    /// Of a geometric kind (`TypeSelector`, `%PLANE`).
    Type(String),
    /// The `n`th group of those of nearly equal key, sorted up (`directionMax`) or down
    /// (`CenterNthSelector`, `RadiusNthSelector`, …; `>Z`, `>>Z[1]`).
    Nth {
        key: NthKey,
        n: i64,
        max: bool,
        tol: f64,
    },
    /// [`Self::Nth`] by the centre along `dir` of those parallel to it (`DirectionNthSelector`,
    /// `>Z[-2]`).
    DirectionNth {
        dir: V3,
        n: i64,
        max: bool,
        tol: f64,
    },
    /// The one whose centre is nearest a point (`NearestToPointSelector`).
    NearestToPoint(V3),
    /// Those whose centre (or box) is inside a box (`BoxSelector`).
    InBox { p0: V3, p1: V3, bbox: bool },
    /// Both (`and`).
    And(Box<Selector>, Box<Selector>),
    /// Either (`or`, `+`).
    Sum(Box<Selector>, Box<Selector>),
    /// The first but not the second (`exc`, `-`).
    Subtract(Box<Selector>, Box<Selector>),
    /// All but these (`not`).
    Inverse(Box<Selector>),
}

fn dedup_keep(v: Vec<Shape>) -> Vec<Shape> {
    let mut seen = std::collections::HashSet::new();
    v.into_iter().filter(|s| seen.insert(s.key())).collect()
}

impl Selector {
    /// The selector of a string.
    pub fn parse(s: &str) -> CqResult<Selector> {
        Parser::new(s)?.expression()
    }

    /// The shapes it keeps.
    pub fn filter(&self, objs: &[Shape]) -> CqResult<Vec<Shape>> {
        Ok(match self {
            Selector::All => objs.to_vec(),
            Selector::Direction { dir, tol } => objs
                .iter()
                .filter(|o| test_vector(o).is_some_and(|v| angle(dir, &v) < *tol))
                .cloned()
                .collect(),
            Selector::Parallel { dir, tol } => objs
                .iter()
                .filter(|o| test_vector(o).is_some_and(|v| dir.cross(&v).norm() < *tol))
                .cloned()
                .collect(),
            Selector::Perpendicular { dir, tol } => objs
                .iter()
                .filter(|o| {
                    test_vector(o).is_some_and(|v| {
                        (angle(dir, &v) - std::f64::consts::FRAC_PI_2).abs() < *tol
                    })
                })
                .cloned()
                .collect(),
            Selector::Type(t) => objs
                .iter()
                .filter(|o| o.geom_type().name() == t)
                .cloned()
                .collect(),
            Selector::Nth { key, n, max, tol } => nth(objs, key, *n, *max, *tol)?,
            Selector::DirectionNth { dir, n, max, tol } => {
                let par: Vec<Shape> = objs
                    .iter()
                    .filter(|o| test_vector(o).is_some_and(|v| dir.cross(&v).norm() < *tol))
                    .cloned()
                    .collect();
                nth(&par, &NthKey::Center(*dir), *n, *max, *tol)?
            }
            Selector::NearestToPoint(p) => objs
                .iter()
                .min_by(|a, b| (a.center() - p).norm().total_cmp(&(b.center() - p).norm()))
                .cloned()
                .into_iter()
                .collect(),
            Selector::InBox { p0, p1, bbox } => {
                let inside = |q: V3| (0..3).all(|k| (q[k] < p0[k]) ^ (q[k] < p1[k]));
                objs.iter()
                    .filter(|o| {
                        if *bbox {
                            o.bounding_box()
                                .is_ok_and(|b| inside(b.min) && inside(b.max))
                        } else {
                            inside(o.center())
                        }
                    })
                    .cloned()
                    .collect()
            }
            Selector::And(a, b) => {
                let (l, r) = (a.filter(objs)?, b.filter(objs)?);
                let rk: std::collections::HashSet<_> = r.iter().map(|s| s.key()).collect();
                dedup_keep(l.into_iter().filter(|s| rk.contains(&s.key())).collect())
            }
            Selector::Sum(a, b) => {
                let mut l = a.filter(objs)?;
                l.extend(b.filter(objs)?);
                dedup_keep(l)
            }
            Selector::Subtract(a, b) => {
                let (l, r) = (a.filter(objs)?, b.filter(objs)?);
                let rk: std::collections::HashSet<_> = r.iter().map(|s| s.key()).collect();
                dedup_keep(l.into_iter().filter(|s| !rk.contains(&s.key())).collect())
            }
            Selector::Inverse(a) => {
                Selector::Subtract(Box::new(Selector::All), a.clone()).filter(objs)?
            }
        })
    }
}

/// `_NthSelector.filter`: groups of keys within `tol` of their group's first, sorted up; the
/// `n`th group (counted from the other end when not `max`).
fn nth(objs: &[Shape], key: &NthKey, n: i64, max: bool, tol: f64) -> CqResult<Vec<Shape>> {
    if objs.is_empty() {
        return value_err!("Can not return the Nth element of an empty list");
    }
    let mut keyed: Vec<(f64, Shape)> = Vec::new();
    for o in objs {
        let k = match key {
            NthKey::Center(d) => Some(o.center().dot(d)),
            NthKey::Radius => match o.shape_type() {
                ShapeType::Edge | ShapeType::Wire => o.radius().ok(),
                _ => None,
            },
            NthKey::Length => match o.shape_type() {
                ShapeType::Edge | ShapeType::Wire => Some(o.length()),
                _ => None,
            },
            NthKey::Area => match o.shape_type() {
                ShapeType::Face | ShapeType::Shell | ShapeType::Solid => o.area().ok(),
                ShapeType::Wire => super::make::faces_from_wires(&[o.edge_geoms()], None)
                    .ok()
                    .and_then(|f| f.first().and_then(|f| f.area().ok()))
                    .map(f64::abs),
                _ => None,
            },
        };
        if let Some(k) = k {
            keyed.push((k, o.clone()));
        }
    }
    if keyed.is_empty() {
        return Err(CqError::Index(format!(
            "Attempted to access index {n} of a list with length 0"
        )));
    }
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut clusters: Vec<Vec<Shape>> = vec![vec![]];
    let mut start = keyed[0].0;
    for (k, o) in keyed {
        if (k - start).abs() <= tol {
            clusters.last_mut().unwrap().push(o);
        } else {
            clusters.push(vec![o]);
            start = k;
        }
    }
    if !max {
        clusters.reverse();
    }
    let len = clusters.len() as i64;
    let i = if n < 0 { len + n } else { n };
    if i < 0 || i >= len {
        return Err(CqError::Index(format!(
            "Attempted to access index {n} of a list with length {len}"
        )));
    }
    Ok(clusters.swap_remove(i as usize))
}

// ------------------------------------------------------------------ the string syntax

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(String),
    Op(String),
    LParen,
    RParen,
    Index(i64),
    Vector(V3),
}

struct Parser {
    toks: Vec<Tok>,
    at: usize,
    src: String,
}

fn axis(name: &str) -> Option<V3> {
    Some(match name {
        "X" => V3::new(1.0, 0.0, 0.0),
        "Y" => V3::new(0.0, 1.0, 0.0),
        "Z" => V3::new(0.0, 0.0, 1.0),
        "XY" => V3::new(1.0, 1.0, 0.0),
        "YZ" => V3::new(0.0, 1.0, 1.0),
        "XZ" => V3::new(1.0, 0.0, 1.0),
        _ => return None,
    })
}

const TYPES: [&str; 17] = [
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
    "BEZIER",
];

impl Parser {
    fn new(s: &str) -> CqResult<Self> {
        let c: Vec<char> = s.chars().collect();
        let mut toks = Vec::new();
        let mut i = 0;
        let bad = || CqError::Value(format!("Invalid selector string: {s:?}"));
        while i < c.len() {
            let ch = c[i];
            if ch.is_whitespace() {
                i += 1;
            } else if ch == '(' {
                // A vector "(x, y, z)" or a group.
                let rest: String = c[i..].iter().collect();
                if let Some(close) = rest.find(')') {
                    let inner = &rest[1..close];
                    let parts: Vec<&str> = inner.split(',').collect();
                    if parts.len() == 3
                        && let (Ok(x), Ok(y), Ok(z)) = (
                            parts[0].trim().parse::<f64>(),
                            parts[1].trim().parse::<f64>(),
                            parts[2].trim().parse::<f64>(),
                        )
                    {
                        toks.push(Tok::Vector(V3::new(x, y, z)));
                        i += close + 1;
                        continue;
                    }
                }
                toks.push(Tok::LParen);
                i += 1;
            } else if ch == ')' {
                toks.push(Tok::RParen);
                i += 1;
            } else if ch == '[' {
                let mut j = i + 1;
                let mut num = String::new();
                while j < c.len() && c[j] != ']' {
                    if !c[j].is_whitespace() {
                        num.push(c[j]);
                    }
                    j += 1;
                }
                toks.push(Tok::Index(num.parse().map_err(|_| bad())?));
                i = j + 1;
            } else if ch == '>' || ch == '<' {
                if i + 1 < c.len() && c[i + 1] == ch {
                    toks.push(Tok::Op(format!("{ch}{ch}")));
                    i += 2;
                } else {
                    toks.push(Tok::Op(ch.to_string()));
                    i += 1;
                }
            } else if "|#+-%".contains(ch) {
                toks.push(Tok::Op(ch.to_string()));
                i += 1;
            } else if ch.is_alphanumeric() || ch == '_' {
                let mut w = String::new();
                while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                    w.push(c[i]);
                    i += 1;
                }
                toks.push(Tok::Word(w));
            } else {
                return Err(bad());
            }
        }
        Ok(Self {
            toks,
            at: 0,
            src: s.to_string(),
        })
    }

    fn bad(&self) -> CqError {
        CqError::Value(format!("Invalid selector string: {:?}", self.src))
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }

    fn word_is(&self, w: &[&str]) -> bool {
        matches!(self.peek(), Some(Tok::Word(x)) if w.contains(&x.as_str()))
    }

    fn expression(mut self) -> CqResult<Selector> {
        let s = self.not_expr()?;
        if self.at != self.toks.len() {
            return Err(self.bad());
        }
        Ok(s)
    }

    // Precedence (pyparsing's infix_notation, first binds tightest): and, or, exc, not.
    fn not_expr(&mut self) -> CqResult<Selector> {
        if self.word_is(&["not"]) {
            self.at += 1;
            let inner = self.not_expr()?;
            return Ok(Selector::Inverse(Box::new(inner)));
        }
        self.exc_expr()
    }

    fn exc_expr(&mut self) -> CqResult<Selector> {
        let mut l = self.or_expr()?;
        while self.word_is(&["exc", "except"]) {
            self.at += 1;
            let r = self.or_or_not()?;
            l = Selector::Subtract(Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn or_or_not(&mut self) -> CqResult<Selector> {
        if self.word_is(&["not"]) {
            self.not_expr()
        } else {
            self.or_expr()
        }
    }

    fn or_expr(&mut self) -> CqResult<Selector> {
        let mut l = self.and_expr()?;
        while self.word_is(&["or"]) {
            self.at += 1;
            let r = if self.word_is(&["not"]) {
                self.not_expr()?
            } else {
                self.and_expr()?
            };
            l = Selector::Sum(Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn and_expr(&mut self) -> CqResult<Selector> {
        let mut l = self.atom_or_group()?;
        while self.word_is(&["and"]) {
            self.at += 1;
            let r = if self.word_is(&["not"]) {
                self.not_expr()?
            } else {
                self.atom_or_group()?
            };
            l = Selector::And(Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn atom_or_group(&mut self) -> CqResult<Selector> {
        if self.peek() == Some(&Tok::LParen) {
            self.at += 1;
            let s = self.not_expr()?;
            if self.peek() != Some(&Tok::RParen) {
                return Err(self.bad());
            }
            self.at += 1;
            return Ok(s);
        }
        self.atom()
    }

    fn direction(&mut self) -> CqResult<V3> {
        match self.peek().cloned() {
            Some(Tok::Word(w)) => {
                let v = axis(&w).ok_or_else(|| self.bad())?;
                self.at += 1;
                Ok(v)
            }
            Some(Tok::Vector(v)) => {
                self.at += 1;
                Ok(v)
            }
            _ => Err(self.bad()),
        }
    }

    fn index(&mut self) -> Option<i64> {
        if let Some(Tok::Index(i)) = self.peek().cloned() {
            self.at += 1;
            Some(i)
        } else {
            None
        }
    }

    fn atom(&mut self) -> CqResult<Selector> {
        const TOL: f64 = 1e-4;
        match self.peek().cloned() {
            Some(Tok::Op(op)) => {
                self.at += 1;
                match op.as_str() {
                    "%" => match self.peek().cloned() {
                        Some(Tok::Word(w)) => {
                            let up = w.to_uppercase();
                            if !TYPES.contains(&up.as_str()) {
                                return Err(self.bad());
                            }
                            self.at += 1;
                            Ok(Selector::Type(up))
                        }
                        _ => Err(self.bad()),
                    },
                    ">" | "<" => {
                        let dir = self.direction()?;
                        let max = op == ">";
                        Ok(match self.index() {
                            Some(n) => Selector::DirectionNth {
                                dir,
                                n,
                                max,
                                tol: TOL,
                            },
                            None => Selector::Nth {
                                key: NthKey::Center(dir),
                                n: -1,
                                max,
                                tol: TOL,
                            },
                        })
                    }
                    ">>" | "<<" => {
                        let dir = self.direction()?;
                        let max = op == ">>";
                        let n = self.index().unwrap_or(-1);
                        Ok(Selector::Nth {
                            key: NthKey::Center(dir),
                            n,
                            max,
                            tol: TOL,
                        })
                    }
                    "|" => Ok(Selector::Parallel {
                        dir: self.direction()?,
                        tol: TOL,
                    }),
                    "#" => Ok(Selector::Perpendicular {
                        dir: self.direction()?,
                        tol: TOL,
                    }),
                    "+" => Ok(Selector::Direction {
                        dir: self.direction()?,
                        tol: TOL,
                    }),
                    "-" => Ok(Selector::Direction {
                        dir: -self.direction()?,
                        tol: TOL,
                    }),
                    _ => Err(self.bad()),
                }
            }
            Some(Tok::Word(w)) => {
                let named = match w.as_str() {
                    "front" => Some((V3::z(), true)),
                    "back" => Some((V3::z(), false)),
                    "left" => Some((V3::x(), false)),
                    "right" => Some((V3::x(), true)),
                    "top" => Some((V3::y(), true)),
                    "bottom" => Some((V3::y(), false)),
                    _ => None,
                };
                if let Some((dir, max)) = named {
                    self.at += 1;
                    return Ok(Selector::Nth {
                        key: NthKey::Center(dir),
                        n: -1,
                        max,
                        tol: TOL,
                    });
                }
                let dir = self.direction()?;
                Ok(Selector::Direction { dir, tol: TOL })
            }
            Some(Tok::Vector(_)) => {
                let dir = self.direction()?;
                Ok(Selector::Direction { dir, tol: TOL })
            }
            _ => Err(self.bad()),
        }
    }
}
