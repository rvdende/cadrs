//! Values typed into dimension boxes: numbers with units and simple arithmetic.
//!
//! Lengths: `25`, `25 mm`, `2.5cm`, `1 in`, `2*10+5`, `(50)/2`, `2*20 mm`, `1 in + 5`. Units are
//! mm, cm, m, in (or `"`) and ft; a value without one is in millimetres. Angles: `30`, `30 deg`,
//! `30°`, `0.5 rad`, `90/2`; degrees by default.
//!
//! **Arithmetic** follows the usual precedence (`*` and `/` before `+` and `-`, parentheses,
//! unary minus). A unit applies to the number right before it. Quantities are checked: a length
//! times a length, or a length plus an angle, is an error; a plain number in a sum with a length
//! counts as millimetres (the default unit), as does a plain result.
//!
//! **Variables and functions** (P3F.4): `#name` reads a variable ([`Variables`]: a length in mm,
//! an angle in degrees or a plain number), so `#piston_d + #clearance` or `2 * #wall + 1 in`
//! evaluate with their units. The functions are `sqrt`, `sin`, `cos`, `tan`, `min`, `max` and
//! `abs` (trig takes an angle; a plain number there is in degrees, as angles are by default),
//! and `pi` is π. `×`, `÷` and `−` read as `*`, `/` and `-`. Values carry length and angle
//! exponents, so `sqrt(#a * #b)` of two lengths is a length and `#len + #angle` is an error.
//!
//! **Workspace units** (X1): a document chooses the length unit values are shown in and a bare
//! number is read in ([`Units`]: mm, cm, m, in, ft or yd, and how many decimals dimensions
//! show). Values are always stored in millimetres; [`Units`] converts for display and input.

use serde::{Deserialize, Serialize};

/// What a value measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantity {
    /// Millimetres.
    Length,
    /// Degrees.
    Angle,
    /// A whole number (a polygon's side count): no unit.
    Count,
}

/// Why a value could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    /// Something that is not a number, unit, operator or parenthesis.
    Unexpected(String),
    /// A unit this quantity does not have (`5 deg` for a length).
    UnknownUnit(String),
    /// Mixed quantities (`1 in * 2 in`, `5 mm + 3 deg`).
    Units,
    DivideByZero,
    /// Missing a closing parenthesis, or an operand.
    Incomplete,
    /// `#name` that is not defined (above this feature, P3F.4).
    UnknownVariable(String),
    /// `name(` that is not one of the functions.
    UnknownFunction(String),
    /// A function given the wrong number of arguments.
    Arguments(String),
    /// A function outside its domain (`sqrt` of a negative).
    Domain(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Empty => write!(f, "Enter a value"),
            ParseError::Unexpected(s) => write!(f, "Unexpected \"{s}\""),
            ParseError::UnknownUnit(u) => write!(f, "Unknown unit \"{u}\""),
            ParseError::Units => write!(f, "Units do not match"),
            ParseError::DivideByZero => write!(f, "Division by zero"),
            ParseError::Incomplete => write!(f, "Incomplete expression"),
            ParseError::UnknownVariable(n) => write!(f, "Variable #{n} is not defined before this feature"),
            ParseError::UnknownFunction(n) => write!(f, "Unknown function \"{n}\""),
            ParseError::Arguments(n) => write!(f, "Wrong number of arguments to {n}"),
            ParseError::Domain(n) => write!(f, "Value out of range for {n}"),
        }
    }
}

/// A value with its dimensions: `len` is the exponent of length (mm), `ang` of angle (degrees).
/// A plain number has both 0.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Val {
    v: f64,
    len: i32,
    ang: i32,
}

impl Val {
    fn plain(v: f64) -> Self {
        Self { v, len: 0, ang: 0 }
    }

    fn is_plain(&self) -> bool {
        self.len == 0 && self.ang == 0
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Unit(String),
    Op(char),
    /// `#name` (P3F.4).
    Var(String),
    /// A function name right before its `(`: sqrt, sin, cos, tan, min, max, abs.
    Func(String),
    /// `pi`.
    Pi,
}

/// The functions an expression can call (P3F.4).
const FUNCTIONS: [&str; 7] = ["sqrt", "sin", "cos", "tan", "min", "max", "abs"];

fn tokenize(text: &str) -> Result<Vec<Tok>, ParseError> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    // Open parentheses: whether each is a function call's, where `,` separates arguments
    // rather than being a decimal comma ("0,5").
    let mut calls: Vec<bool> = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        let decimal_comma = !calls.last().copied().unwrap_or(false);
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' || (c == ',' && decimal_comma) {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_digit() || chars[i] == '.' || (chars[i] == ',' && decimal_comma))
            {
                i += 1;
            }
            // An exponent: 1e3, 2.5E-2.
            if i + 1 < chars.len()
                && (chars[i] == 'e' || chars[i] == 'E')
                && (chars[i + 1].is_ascii_digit()
                    || ((chars[i + 1] == '-' || chars[i + 1] == '+')
                        && chars.get(i + 2).is_some_and(|c| c.is_ascii_digit())))
            {
                i += 2;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let s: String = chars[start..i].iter().collect::<String>().replace(',', ".");
            let v: f64 = s.parse().map_err(|_| ParseError::Unexpected(s.clone()))?;
            out.push(Tok::Num(v));
        } else if c == '#' {
            i += 1;
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            if start == i {
                return Err(ParseError::Unexpected("#".into()));
            }
            out.push(Tok::Var(chars[start..i].iter().collect()));
        } else if c.is_alphabetic() || c == '"' || c == '°' {
            let start = i;
            if c == '"' || c == '°' {
                i += 1;
            } else {
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
            }
            let word: String = chars[start..i].iter().collect();
            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            let lower = word.to_ascii_lowercase();
            if chars.get(j) == Some(&'(') && c.is_alphabetic() {
                if !FUNCTIONS.contains(&lower.as_str()) {
                    return Err(ParseError::UnknownFunction(word));
                }
                out.push(Tok::Func(lower));
            } else if lower == "pi" {
                out.push(Tok::Pi);
            } else {
                out.push(Tok::Unit(word));
            }
        } else if "+-*/(),×÷−".contains(c) {
            if c == '(' {
                calls.push(matches!(out.last(), Some(Tok::Func(_))));
            } else if c == ')' {
                calls.pop();
            }
            out.push(Tok::Op(match c {
                '×' => '*',
                '÷' => '/',
                '−' => '-',
                c => c,
            }));
            i += 1;
        } else {
            return Err(ParseError::Unexpected(c.to_string()));
        }
    }
    Ok(out)
}

/// A unit's factor to mm (a length) or degrees (an angle), and which it is.
fn unit_of(unit: &str) -> Option<(f64, Quantity)> {
    let u = unit.to_ascii_lowercase();
    Some(match u.as_str() {
        "mm" | "millimeter" | "millimeters" => (1.0, Quantity::Length),
        "cm" | "centimeter" | "centimeters" => (10.0, Quantity::Length),
        "m" | "meter" | "meters" => (1000.0, Quantity::Length),
        "in" | "inch" | "inches" | "\"" => (25.4, Quantity::Length),
        "ft" | "foot" | "feet" => (304.8, Quantity::Length),
        "yd" | "yard" | "yards" => (914.4, Quantity::Length),
        "deg" | "°" | "degree" | "degrees" => (1.0, Quantity::Angle),
        "rad" | "radian" | "radians" => (180.0 / std::f64::consts::PI, Quantity::Angle),
        _ => return None,
    })
}

/// A variable's value as an expression sees it (P3F.4): millimetres for a length, degrees for
/// an angle, a plain number for a count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VarValue {
    pub value: f64,
    pub quantity: Quantity,
}

/// Looks up `#name` for an expression (P3F.4). `None`: not defined (at that point of the
/// feature list).
pub trait Variables {
    fn variable(&self, name: &str) -> Option<VarValue>;
}

/// No variables at all.
pub struct NoVariables;

impl Variables for NoVariables {
    fn variable(&self, _name: &str) -> Option<VarValue> {
        None
    }
}

impl<F: Fn(&str) -> Option<VarValue>> Variables for F {
    fn variable(&self, name: &str) -> Option<VarValue> {
        self(name)
    }
}

impl Variables for [(String, VarValue)] {
    fn variable(&self, name: &str) -> Option<VarValue> {
        self.iter().rev().find(|(n, _)| n == name).map(|(_, v)| *v)
    }
}

impl Variables for &[(String, VarValue)] {
    fn variable(&self, name: &str) -> Option<VarValue> {
        (**self).variable(name)
    }
}

impl Variables for Vec<(String, VarValue)> {
    fn variable(&self, name: &str) -> Option<VarValue> {
        self.as_slice().variable(name)
    }
}

/// The variables an expression names (`#name`), in order, without repeats.
pub fn variable_names(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '#' {
            let start = i + 1;
            let mut j = start;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            let name: String = chars[start..j].iter().collect();
            if !name.is_empty() && !out.contains(&name) {
                out.push(name);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// Whether `name` is a valid variable name: a letter or `_`, then letters, digits or `_`.
pub fn is_variable_name(name: &str) -> bool {
    let mut c = name.chars();
    c.next().is_some_and(|f| f.is_alphabetic() || f == '_') && c.all(|x| x.is_alphanumeric() || x == '_')
}

struct Parser<'a> {
    toks: &'a [Tok],
    i: usize,
    /// What a plain number counts as (in mm) when it meets a length.
    default: f64,
    vars: &'a dyn Variables,
}

/// A length unit a workspace can use (Onshape's "Workspace units" dialog).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum LengthUnit {
    #[default]
    Millimeter,
    Centimeter,
    Meter,
    Inch,
    Foot,
    Yard,
}

impl LengthUnit {
    pub const ALL: [LengthUnit; 6] = [
        LengthUnit::Millimeter,
        LengthUnit::Centimeter,
        LengthUnit::Meter,
        LengthUnit::Inch,
        LengthUnit::Foot,
        LengthUnit::Yard,
    ];

    /// Millimetres per unit.
    pub fn mm(self) -> f64 {
        match self {
            LengthUnit::Millimeter => 1.0,
            LengthUnit::Centimeter => 10.0,
            LengthUnit::Meter => 1000.0,
            LengthUnit::Inch => 25.4,
            LengthUnit::Foot => 304.8,
            LengthUnit::Yard => 914.4,
        }
    }

    /// The abbreviation shown after values ("mm", "in").
    pub fn symbol(self) -> &'static str {
        match self {
            LengthUnit::Millimeter => "mm",
            LengthUnit::Centimeter => "cm",
            LengthUnit::Meter => "m",
            LengthUnit::Inch => "in",
            LengthUnit::Foot => "ft",
            LengthUnit::Yard => "yd",
        }
    }

    /// The name in the units dialog ("Millimeter").
    pub fn label(self) -> &'static str {
        match self {
            LengthUnit::Millimeter => "Millimeter",
            LengthUnit::Centimeter => "Centimeter",
            LengthUnit::Meter => "Meter",
            LengthUnit::Inch => "Inch",
            LengthUnit::Foot => "Foot",
            LengthUnit::Yard => "Yard",
        }
    }
}

/// A mass unit a workspace can use (the "Workspace units" dialog's Mass units, X8, P3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum MassUnit {
    #[default]
    Kilogram,
    Gram,
    Pound,
    Ounce,
}

impl MassUnit {
    pub const ALL: [MassUnit; 4] = [MassUnit::Kilogram, MassUnit::Gram, MassUnit::Pound, MassUnit::Ounce];

    /// Kilograms per unit (the international avoirdupois pound is exactly 0.45359237 kg, the
    /// ounce a sixteenth of it).
    pub fn kg(self) -> f64 {
        match self {
            MassUnit::Kilogram => 1.0,
            MassUnit::Gram => 1e-3,
            MassUnit::Pound => 0.453_592_37,
            MassUnit::Ounce => 0.453_592_37 / 16.0,
        }
    }

    /// The abbreviation shown after values ("kg", "lb").
    pub fn symbol(self) -> &'static str {
        match self {
            MassUnit::Kilogram => "kg",
            MassUnit::Gram => "g",
            MassUnit::Pound => "lb",
            MassUnit::Ounce => "oz",
        }
    }

    /// The name in the units dialog ("Kilogram").
    pub fn label(self) -> &'static str {
        match self {
            MassUnit::Kilogram => "Kilogram",
            MassUnit::Gram => "Gram",
            MassUnit::Pound => "Pound",
            MassUnit::Ounce => "Ounce",
        }
    }
}

/// A document's workspace units: the length unit values are shown and typed in, and the most
/// decimals a dimension shows. The default is a metric workspace: millimetres, 3 decimals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Units {
    pub length: LengthUnit,
    /// The mass unit of Mass properties (mass, density, inertia; P3.5).
    #[serde(default)]
    pub mass: MassUnit,
    /// Decimals of committed dimension values and measurements (trailing zeros dropped for
    /// dimensions).
    pub decimals: u8,
    /// Decimals of committed angle values (the units dialog's "Angle decimal places").
    #[serde(default = "default_angle_decimals")]
    pub angle_decimals: u8,
}

fn default_angle_decimals() -> u8 {
    3
}

impl Default for Units {
    fn default() -> Self {
        Self {
            length: LengthUnit::Millimeter,
            mass: MassUnit::Kilogram,
            decimals: 3,
            angle_decimals: 3,
        }
    }
}

/// The most decimals the units dialog offers.
pub const MAX_DECIMALS: u8 = 6;

/// A value to `d` decimals, as the Mass properties panel shows it: one that would round to zero
/// but isn't (a centre of mass a hair off an axis, a small product of inertia) in scientific
/// notation with 4 significant digits, as Onshape does (`ex4-step18.png`: −7.545e−5, 4.422e−5);
/// below 1e−12 it is zero (numerical noise), and never "-0.000".
pub fn fixed_or_scientific(v: f64, d: usize) -> String {
    fixed_or_scientific_of(v, d, 0.0)
}

/// [`fixed_or_scientific`] for a value of a quantity whose size is `scale` (a part's size for a
/// centre of mass, its largest moment for a product of inertia): below 1e−9 of it, the value is
/// the kernel's rounding noise and shows as zero (P3.11: the reflector's centre of mass read
/// "−1.067e−10 mm" on its symmetry plane; Onshape's funnel values, 1e−5 of its size, stay).
pub fn fixed_or_scientific_of(v: f64, d: usize, scale: f64) -> String {
    if v.abs() < 1e-9 * scale.abs() && (v * 10f64.powi(d as i32)).round() == 0.0 {
        return format!("{:.d$}", 0.0);
    }
    if (v * 10f64.powi(d as i32)).round() != 0.0 {
        return format!("{v:.d$}");
    }
    if v.abs() < 1e-12 {
        return format!("{:.d$}", 0.0);
    }
    format!("{v:.3e}")
}

impl Units {
    pub fn new(length: LengthUnit, decimals: u8) -> Self {
        Self {
            length,
            mass: MassUnit::Kilogram,
            decimals: decimals.min(MAX_DECIMALS),
            angle_decimals: 3,
        }
    }

    /// The same units with `mass` for masses.
    pub fn with_mass(self, mass: MassUnit) -> Self {
        Self { mass, ..self }
    }

    /// A mass in kg, in the workspace mass unit with it: "7.850 kg".
    pub fn mass(&self, kg: f64) -> String {
        let d = self.decimals as usize;
        format!("{:.d$} {}", kg / self.mass.kg(), self.mass.symbol())
    }

    /// A moment of inertia in kg·mm², in the workspace units (mass · length²), without the unit
    /// (the panel's heading carries it).
    pub fn inertia(&self, kg_mm2: f64) -> String {
        self.inertia_of(kg_mm2, 0.0)
    }

    /// [`Self::inertia`] of a tensor whose largest moment is `largest` (kg·mm²): products below
    /// 1e−9 of it are noise, shown as zero (see [`fixed_or_scientific_of`]).
    pub fn inertia_of(&self, kg_mm2: f64, largest: f64) -> String {
        let f = self.length.mm();
        let k = self.mass.kg() * f * f;
        fixed_or_scientific_of(kg_mm2 / k, self.decimals as usize, largest / k)
    }

    /// A density in kg/m³, in the workspace units, and its unit: kg/m³ or g/cm³ in a metric
    /// workspace (as material tables give them), mass per cubic length unit otherwise
    /// ("0.033 lb/in³").
    pub fn density(&self, kg_m3: f64) -> (f64, String) {
        match (self.length, self.mass) {
            (LengthUnit::Millimeter | LengthUnit::Centimeter | LengthUnit::Meter, MassUnit::Kilogram) => {
                (kg_m3, "kg/m\u{b3}".into())
            }
            (LengthUnit::Millimeter | LengthUnit::Centimeter | LengthUnit::Meter, MassUnit::Gram) => {
                (kg_m3 / 1000.0, "g/cm\u{b3}".into())
            }
            (l, m) => {
                let m3 = (l.mm() / 1000.0).powi(3);
                (kg_m3 * m3 / m.kg(), format!("{}/{}\u{b3}", m.symbol(), l.symbol()))
            }
        }
    }

    /// A density typed in the workspace units (see [`Units::density`]), in kg/m³.
    pub fn density_to_si(&self, v: f64) -> f64 {
        let (one, _) = self.density(1.0);
        v / one
    }

    /// The same units with `decimals` for angles.
    pub fn with_angle_decimals(self, decimals: u8) -> Self {
        Self {
            angle_decimals: decimals.min(MAX_DECIMALS),
            ..self
        }
    }

    /// A length in millimetres, in the workspace unit.
    pub fn to_unit(&self, mm: f64) -> f64 {
        mm / self.length.mm()
    }

    /// A value in the workspace unit, in millimetres.
    pub fn to_mm(&self, v: f64) -> f64 {
        v * self.length.mm()
    }

    /// A live length while drawing: 5 decimals in the workspace unit, trailing zeros dropped.
    pub fn live(&self, mm: f64) -> String {
        format_trimmed(self.to_unit(mm), 5)
    }

    /// A committed dimension value of `q` (angles stay in degrees).
    pub fn value(&self, v: f64, q: Quantity) -> String {
        match q {
            Quantity::Length => format_trimmed(self.to_unit(v), self.decimals as usize),
            Quantity::Angle => format_trimmed(v, self.angle_decimals as usize),
            Quantity::Count => format!("{}", v.round() as i64),
        }
    }

    /// A live value of `q` (5 decimals).
    pub fn live_value(&self, v: f64, q: Quantity) -> String {
        match q {
            Quantity::Length => self.live(v),
            Quantity::Angle => format_trimmed(v, 5),
            Quantity::Count => format!("{}", v.round() as i64),
        }
    }

    /// The text an edit box opens with: the value with its unit ("50 mm", "1.9685 in",
    /// "47.729 deg").
    pub fn with_unit(&self, v: f64, q: Quantity) -> String {
        match q {
            Quantity::Length => format!("{} {}", self.live(v), self.length.symbol()),
            Quantity::Angle => format!("{} deg", format_trimmed(v, 5)),
            Quantity::Count => format!("{}", v.round() as i64),
        }
    }

    /// An area in mm², in the workspace unit squared, with the unit: "1234.500 mm²".
    pub fn area(&self, mm2: f64) -> String {
        let f = self.length.mm();
        let d = self.decimals as usize;
        format!("{:.d$} {}\u{b2}", mm2 / (f * f), self.length.symbol())
    }

    /// A volume in mm³, in the workspace unit cubed, with the unit, as Onshape's Mass and section
    /// properties show it: "368749.705 mm³" (the workspace decimals, no digit grouping).
    pub fn volume(&self, mm3: f64) -> String {
        let f = self.length.mm();
        let d = self.decimals as usize;
        format!("{:.d$} {}\u{b3}", mm3 / (f * f * f), self.length.symbol())
    }

    /// A length in mm with the workspace decimals kept (a coordinate in a readout): "12.500 mm".
    pub fn fixed_length(&self, mm: f64) -> String {
        self.fixed_length_of(mm, 0.0)
    }

    /// [`Self::fixed_length`] of a coordinate on something `size` mm across: below 1e−9 of it,
    /// zero (see [`fixed_or_scientific_of`]).
    pub fn fixed_length_of(&self, mm: f64, size: f64) -> String {
        let v = self.to_unit(mm);
        format!("{} {}", fixed_or_scientific_of(v, self.decimals as usize, self.to_unit(size)), self.length.symbol())
    }

    /// Evaluates typed text for `q`: a bare number (or a plain result) is in the workspace unit
    /// for a length; explicit units (`5 mm`, `1 in`) are honoured. Returns millimetres (or
    /// degrees).
    pub fn eval(&self, text: &str, q: Quantity) -> Result<f64, ParseError> {
        let default = if q == Quantity::Length {
            self.length.mm()
        } else {
            1.0
        };
        eval_with(text, q, default, &NoVariables)
    }

    /// [`Units::eval`] with variables (`#name`, P3F.4): `#piston_d + #clearance`.
    pub fn eval_vars(&self, text: &str, q: Quantity, vars: &dyn Variables) -> Result<f64, ParseError> {
        let default = if q == Quantity::Length { self.length.mm() } else { 1.0 };
        eval_with(text, q, default, vars)
    }

    /// [`Units::eval`] for a length; `None` if it cannot be read.
    pub fn parse_length(&self, text: &str) -> Option<f64> {
        self.eval(text, Quantity::Length).ok()
    }
}


impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i)
    }

    /// `a` and `b` brought to the same dimensions for a sum, a `min` or a `max`: a plain number
    /// with a length is in the default unit, with an angle in degrees.
    fn same(&self, a: Val, b: Val) -> Result<(Val, Val), ParseError> {
        let d = self.default;
        Ok(match (a, b) {
            (x, y) if x.len == y.len && x.ang == y.ang => (x, y),
            (x, y) if y.is_plain() && x.len == 1 && x.ang == 0 => (x, Val { v: y.v * d, ..x }),
            (x, y) if x.is_plain() && y.len == 1 && y.ang == 0 => (Val { v: x.v * d, ..y }, y),
            (x, y) if y.is_plain() && x.len == 0 && x.ang == 1 => (x, Val { v: y.v, ..x }),
            (x, y) if x.is_plain() && y.len == 0 && y.ang == 1 => (Val { v: x.v, ..y }, y),
            _ => return Err(ParseError::Units),
        })
    }

    fn expr(&mut self) -> Result<Val, ParseError> {
        let mut acc = self.term()?;
        while let Some(Tok::Op(op @ ('+' | '-'))) = self.peek().cloned() {
            self.i += 1;
            let rhs = self.term()?;
            let (a, b) = self.same(acc, rhs)?;
            acc = Val {
                v: if op == '+' { a.v + b.v } else { a.v - b.v },
                ..a
            };
        }
        Ok(acc)
    }

    fn term(&mut self) -> Result<Val, ParseError> {
        let mut acc = self.unary()?;
        while let Some(Tok::Op(op @ ('*' | '/'))) = self.peek().cloned() {
            self.i += 1;
            let rhs = self.unary()?;
            acc = if op == '*' {
                Val {
                    v: acc.v * rhs.v,
                    len: acc.len + rhs.len,
                    ang: acc.ang + rhs.ang,
                }
            } else {
                if rhs.v == 0.0 {
                    return Err(ParseError::DivideByZero);
                }
                Val {
                    v: acc.v / rhs.v,
                    len: acc.len - rhs.len,
                    ang: acc.ang - rhs.ang,
                }
            };
        }
        Ok(acc)
    }

    fn unary(&mut self) -> Result<Val, ParseError> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.i += 1;
                let v = self.unary()?;
                Ok(Val { v: -v.v, ..v })
            }
            Some(Tok::Op('+')) => {
                self.i += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }

    /// The arguments of a function call, after its name: `( expr {, expr} )`.
    fn args(&mut self) -> Result<Vec<Val>, ParseError> {
        if self.peek() != Some(&Tok::Op('(')) {
            return Err(ParseError::Incomplete);
        }
        self.i += 1;
        let mut out = vec![self.expr()?];
        loop {
            match self.peek() {
                Some(Tok::Op(',')) => {
                    self.i += 1;
                    out.push(self.expr()?);
                }
                Some(Tok::Op(')')) => {
                    self.i += 1;
                    return Ok(out);
                }
                _ => return Err(ParseError::Incomplete),
            }
        }
    }

    fn call(&mut self, name: &str) -> Result<Val, ParseError> {
        let args = self.args()?;
        let one = |args: &[Val]| -> Result<Val, ParseError> {
            match args {
                [a] => Ok(*a),
                _ => Err(ParseError::Arguments(name.to_string())),
            }
        };
        match name {
            "sqrt" => {
                let a = one(&args)?;
                if a.len % 2 != 0 || a.ang % 2 != 0 {
                    return Err(ParseError::Units);
                }
                if a.v < 0.0 {
                    return Err(ParseError::Domain(name.to_string()));
                }
                Ok(Val { v: a.v.sqrt(), len: a.len / 2, ang: a.ang / 2 })
            }
            "sin" | "cos" | "tan" => {
                let a = one(&args)?;
                // An angle, or a plain number of degrees (angles are in degrees by default).
                if a.len != 0 || !(a.ang == 0 || a.ang == 1) {
                    return Err(ParseError::Units);
                }
                let r = a.v.to_radians();
                Ok(Val::plain(match name {
                    "sin" => r.sin(),
                    "cos" => r.cos(),
                    _ => r.tan(),
                }))
            }
            "abs" => {
                let a = one(&args)?;
                Ok(Val { v: a.v.abs(), ..a })
            }
            _ => {
                // min, max
                let mut acc = args[0];
                for b in &args[1..] {
                    let (x, y) = self.same(acc, *b)?;
                    acc = if (name == "min") == (y.v < x.v) { y } else { x };
                }
                Ok(acc)
            }
        }
    }

    fn atom(&mut self) -> Result<Val, ParseError> {
        let mut val = match self.peek().cloned() {
            Some(Tok::Num(v)) => {
                self.i += 1;
                Val::plain(v)
            }
            Some(Tok::Pi) => {
                self.i += 1;
                return Ok(Val::plain(std::f64::consts::PI));
            }
            Some(Tok::Var(name)) => {
                self.i += 1;
                let x = self.vars.variable(&name).ok_or(ParseError::UnknownVariable(name))?;
                return Ok(match x.quantity {
                    Quantity::Length => Val { v: x.value, len: 1, ang: 0 },
                    Quantity::Angle => Val { v: x.value, len: 0, ang: 1 },
                    Quantity::Count => Val::plain(x.value),
                });
            }
            Some(Tok::Func(name)) => {
                self.i += 1;
                self.call(&name)?
            }
            Some(Tok::Op('(')) => {
                self.i += 1;
                let v = self.expr()?;
                if self.peek() != Some(&Tok::Op(')')) {
                    return Err(ParseError::Incomplete);
                }
                self.i += 1;
                v
            }
            Some(Tok::Unit(u)) => return Err(ParseError::Unexpected(u)),
            Some(Tok::Op(c)) => return Err(ParseError::Unexpected(c.to_string())),
            None => return Err(ParseError::Incomplete),
        };
        // A unit right after a number or a parenthesis applies to it.
        if let Some(Tok::Unit(u)) = self.peek().cloned() {
            self.i += 1;
            if !val.is_plain() {
                return Err(ParseError::Units);
            }
            let (f, q) = unit_of(&u).ok_or(ParseError::UnknownUnit(u))?;
            val = match q {
                Quantity::Angle => Val { v: val.v * f, len: 0, ang: 1 },
                _ => Val { v: val.v * f, len: 1, ang: 0 },
            };
        }
        Ok(val)
    }
}

/// Evaluates a value typed for a quantity: millimetres for a length, degrees for an angle.
pub fn eval(text: &str, q: Quantity) -> Result<f64, ParseError> {
    eval_with(text, q, 1.0, &NoVariables)
}

/// [`eval`] with variables (`#name`, P3F.4).
pub fn eval_vars(text: &str, q: Quantity, vars: &dyn Variables) -> Result<f64, ParseError> {
    eval_with(text, q, 1.0, vars)
}

/// [`eval`] with a plain number (a plain result, or a plain number in a sum with a length)
/// worth `default` millimetres, and `vars` for `#name`.
fn eval_with(text: &str, q: Quantity, default: f64, vars: &dyn Variables) -> Result<f64, ParseError> {
    let toks = tokenize(text)?;
    if toks.is_empty() {
        return Err(ParseError::Empty);
    }
    let mut p = Parser {
        toks: &toks,
        i: 0,
        default,
        vars,
    };
    let v = p.expr()?;
    if let Some(t) = p.peek() {
        return Err(ParseError::Unexpected(match t {
            Tok::Num(v) => v.to_string(),
            Tok::Unit(u) => u.clone(),
            Tok::Op(c) => c.to_string(),
            Tok::Var(n) => format!("#{n}"),
            Tok::Func(n) => n.clone(),
            Tok::Pi => "pi".into(),
        }));
    }
    if !v.v.is_finite() {
        return Err(ParseError::DivideByZero);
    }
    match (q, v.len, v.ang) {
        // A plain result is in the default unit.
        (Quantity::Length, 0, 0) => Ok(v.v * default),
        (Quantity::Length, 1, 0) | (Quantity::Angle, 0, 0 | 1) | (Quantity::Count, 0, 0) => Ok(v.v),
        _ => Err(ParseError::Units),
    }
}

/// What an expression's result measures, whatever the field (a Variable's own expression,
/// P3F.4): a length (mm), an angle (degrees) or a plain number. A plain number stays plain.
pub fn eval_any(text: &str, default: f64, vars: &dyn Variables) -> Result<VarValue, ParseError> {
    let toks = tokenize(text)?;
    if toks.is_empty() {
        return Err(ParseError::Empty);
    }
    let mut p = Parser {
        toks: &toks,
        i: 0,
        default,
        vars,
    };
    let v = p.expr()?;
    if p.peek().is_some() {
        return Err(ParseError::Incomplete);
    }
    if !v.v.is_finite() {
        return Err(ParseError::DivideByZero);
    }
    let quantity = match (v.len, v.ang) {
        (0, 0) => Quantity::Count,
        (1, 0) => Quantity::Length,
        (0, 1) => Quantity::Angle,
        _ => return Err(ParseError::Units),
    };
    Ok(VarValue { value: v.v, quantity })
}

/// Parses a length (with arithmetic) and returns it in millimetres.
pub fn parse_length(text: &str) -> Option<f64> {
    eval(text, Quantity::Length).ok()
}

/// Formats a live value the way Onshape shows it while drawing: 5 decimals, trailing zeros
/// dropped, at least one digit ("49.12062", "45.7388", "30").
pub fn format_live(v: f64) -> String {
    format_trimmed(v, 5)
}

/// Formats a committed dimension value (up to 3 decimals, trailing zeros dropped: "50", "12.5").
pub fn format_dimension(v: f64) -> String {
    format_trimmed(v, 3)
}

/// The value an edit box opens with: the value with its unit ("50 mm", "47.729 deg").
pub fn format_with_unit(v: f64, q: Quantity) -> String {
    match q {
        Quantity::Length => format!("{} mm", format_trimmed(v, 5)),
        Quantity::Angle => format!("{} deg", format_trimmed(v, 5)),
        Quantity::Count => format!("{}", v.round() as i64),
    }
}

fn format_trimmed(v: f64, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    };
    if s == "-0" { "0".into() } else { s }
}

#[cfg(test)]
mod tests {
    #[test]
    fn volume_area_and_lengths_as_mass_properties_show_them() {
        let mm = Units::default();
        // The Control Arm's self-check (PS6.6), as Onshape shows it.
        assert_eq!(mm.volume(368_749.704_8), "368749.705 mm\u{b3}");
        assert_eq!(mm.area(50_179.711_0), "50179.711 mm\u{b2}");
        // A value that would round to zero shows in scientific notation (as Onshape's Mass
        // properties, `ex4-step18.png`); noise below 1e-12 is zero.
        assert_eq!(mm.fixed_length(-0.000_1), "-1.000e-4 mm");
        assert_eq!(mm.fixed_length(-7.545e-5), "-7.545e-5 mm");
        assert_eq!(mm.fixed_length(1e-15), "0.000 mm");
        assert_eq!(mm.fixed_length(12.5), "12.500 mm");
        // P3.11: noise on a symmetry plane of a part ~100 mm across is zero (the reflector's
        // centre of mass, −1.067e−10 and 7.039e−8 mm), not below 1e−9 of it (the funnel's).
        assert_eq!(mm.fixed_length_of(-1.067e-10, 99.5), "0.000 mm");
        assert_eq!(mm.fixed_length_of(7.039e-8, 99.5), "0.000 mm");
        assert_eq!(mm.fixed_length_of(-7.545e-5, 99.5), "-7.545e-5 mm");
        assert_eq!(mm.inertia_of(-4.307e-6, 13_934.047), "0.000");
        assert_eq!(mm.inertia_of(4.422e-5, 1.0), "4.422e-5");
        let inch = Units::new(LengthUnit::Inch, 3);
        assert_eq!(inch.volume(25.4 * 25.4 * 25.4), "1.000 in\u{b3}");
    }

    use super::*;

    fn len(s: &str) -> Result<f64, ParseError> {
        eval(s, Quantity::Length)
    }

    fn close(a: Result<f64, ParseError>, b: f64) {
        let a = a.unwrap();
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    #[test]
    fn parses_lengths() {
        assert_eq!(parse_length("50"), Some(50.0));
        assert_eq!(parse_length(" 5 mm"), Some(5.0));
        assert_eq!(parse_length("2.5cm"), Some(25.0));
        assert_eq!(parse_length("1 in"), Some(25.4));
        assert_eq!(parse_length("0,5"), Some(0.5));
        assert_eq!(parse_length("abc"), None);
        assert_eq!(parse_length("5 parsecs"), None);
        assert_eq!(parse_length(""), None);
    }

    #[test]
    fn units() {
        close(len("25 mm"), 25.0);
        close(len("1 in"), 25.4);
        close(len("1\""), 25.4);
        close(len("2 cm"), 20.0);
        close(len("0.1 m"), 100.0);
        close(len("1 ft"), 304.8);
        close(len("1 IN"), 25.4);
        close(len("1e1"), 10.0);
        // An angle is not a length.
        assert_eq!(len("5 deg"), Err(ParseError::Units));
        assert_eq!(len("5 parsec"), Err(ParseError::UnknownUnit("parsec".into())));
    }

    #[test]
    fn arithmetic() {
        close(len("2*10+5"), 25.0);
        close(len("(50)/2"), 25.0);
        close(len("2*20 mm"), 40.0);
        close(len("2 * (3 + 4)"), 14.0);
        close(len("10 - 2 - 3"), 5.0);
        close(len("-5 + 20"), 15.0);
        close(len("1 in + 5"), 30.4);
        close(len("1 in / 2"), 12.7);
        close(len("2 in / 1 in * 10"), 20.0);
        close(len("(1 in)"), 25.4);
        close(len("8 / 4 / 2"), 1.0);
    }

    #[test]
    fn errors() {
        assert_eq!(len(""), Err(ParseError::Empty));
        assert_eq!(len("   "), Err(ParseError::Empty));
        assert_eq!(len("1 in * 2 in"), Err(ParseError::Units));
        assert_eq!(len("5 / 0"), Err(ParseError::DivideByZero));
        assert_eq!(len("(5 + 2"), Err(ParseError::Incomplete));
        assert_eq!(len("5 +"), Err(ParseError::Incomplete));
        assert!(matches!(len("5 5"), Err(ParseError::Unexpected(_))));
        assert!(matches!(len("5 $"), Err(ParseError::Unexpected(_))));
        assert!(matches!(len("mm"), Err(ParseError::Unexpected(_))));
        assert_eq!(len("5 mm mm"), Err(ParseError::Unexpected("mm".into())));
    }

    fn vars() -> Vec<(String, VarValue)> {
        let l = |v| VarValue { value: v, quantity: Quantity::Length };
        vec![
            ("piston_d".into(), l(40.0)),
            // 0.5 mm typed as 0.019685 in: mixed units still add up in mm.
            ("clearance".into(), l(0.5)),
            ("clearance_in".into(), l(0.02 * 25.4)),
            ("angle".into(), VarValue { value: 30.0, quantity: Quantity::Angle }),
            ("n".into(), VarValue { value: 3.0, quantity: Quantity::Count }),
        ]
    }

    #[test]
    fn variables_with_mixed_units() {
        let v = vars();
        let e = |s: &str| eval_vars(s, Quantity::Length, &v);
        // P3F.4: the course's bore, `#piston_d + #clearance`.
        close(e("#piston_d + #clearance"), 40.5);
        close(e("#piston_d + #clearance_in"), 40.508);
        close(e("#piston_d + 0.02 in"), 40.508);
        close(e("#piston_d + 1"), 41.0);
        close(e("#piston_d * #n"), 120.0);
        close(e("#piston_d / 2 + 10 mm"), 30.0);
        // An inch workspace reads a plain number in inches, variables stay what they are.
        let inch = Units::new(LengthUnit::Inch, 3);
        close(inch.eval_vars("#piston_d + 1", Quantity::Length, &v), 65.4);
        assert_eq!(e("#missing + 1"), Err(ParseError::UnknownVariable("missing".into())));
        assert_eq!(e("#piston_d + #angle"), Err(ParseError::Units));
        assert_eq!(e("#piston_d * #piston_d"), Err(ParseError::Units));
        assert_eq!(eval_vars("#piston_d", Quantity::Count, &v), Err(ParseError::Units));
        close(eval_vars("#n * 2", Quantity::Count, &v), 6.0);
        close(eval_vars("#angle * 2", Quantity::Angle, &v), 60.0);
        assert_eq!(variable_names("#a + #b_2*#a - 2"), vec!["a".to_string(), "b_2".to_string()]);
        assert!(is_variable_name("piston_d") && !is_variable_name("2x") && !is_variable_name("a b"));
    }

    #[test]
    fn functions() {
        let v = vars();
        let e = |s: &str| eval_vars(s, Quantity::Length, &v);
        close(e("sqrt(#piston_d * #piston_d)"), 40.0);
        close(e("sqrt(16) mm"), 4.0);
        close(e("sin(30 deg) * 10 mm"), 5.0);
        close(e("sin(#angle) * 10"), 5.0);
        close(e("cos(60) * 10 mm"), 5.0);
        close(e("tan(45 deg) * 2 in"), 50.8);
        close(e("min(3 mm, 1 in, 5)"), 3.0);
        close(e("max(#piston_d, 2 in)"), 50.8);
        close(e("abs(-4 mm)"), 4.0);
        close(e("2 × (3 + 4) ÷ 7 − 1"), 1.0);
        close(e("pi * 10"), std::f64::consts::PI * 10.0);
        close(e("MAX(1, 2)"), 2.0);
        assert_eq!(e("sqrt(-4)"), Err(ParseError::Domain("sqrt".into())));
        assert_eq!(e("sqrt(#piston_d)"), Err(ParseError::Units));
        assert_eq!(e("sin(1 mm)"), Err(ParseError::Units));
        assert_eq!(e("foo(1)"), Err(ParseError::UnknownFunction("foo".into())));
        assert_eq!(e("sin(1, 2)"), Err(ParseError::Arguments("sin".into())));
        assert_eq!(e("min(1 mm, 2 deg)"), Err(ParseError::Units));
        assert_eq!(e("max(1, 2"), Err(ParseError::Incomplete));
        // A variable's own expression keeps its quantity.
        assert_eq!(eval_any("40 mm", 1.0, &v).map(|x| x.quantity), Ok(Quantity::Length));
        assert_eq!(eval_any("3", 1.0, &v).map(|x| x.quantity), Ok(Quantity::Count));
        assert_eq!(eval_any("#angle / 2", 1.0, &v), Ok(VarValue { value: 15.0, quantity: Quantity::Angle }));
    }

    #[test]
    fn angles() {
        let ang = |s| eval(s, Quantity::Angle);
        close(ang("30"), 30.0);
        close(ang("30 deg"), 30.0);
        close(ang("30°"), 30.0);
        close(ang("90/2"), 45.0);
        close(ang("0.5 rad"), 0.5 * 180.0 / std::f64::consts::PI);
        assert!(ang("5 mm").is_err());
    }

    #[test]
    fn workspace_units_convert_display_and_input() {
        let inch = Units::new(LengthUnit::Inch, 3);
        // Display: stored millimetres shown in inches.
        assert_eq!(inch.value(25.4, Quantity::Length), "1");
        assert_eq!(inch.value(50.0, Quantity::Length), "1.969");
        assert_eq!(inch.live(50.0), "1.9685");
        assert_eq!(inch.with_unit(25.4, Quantity::Length), "1 in");
        // Angles are not lengths.
        assert_eq!(inch.value(30.0, Quantity::Angle), "30");
        // Input: a bare number is in inches; explicit units win.
        close(inch.eval("2", Quantity::Length), 50.8);
        close(inch.eval("10 mm", Quantity::Length), 10.0);
        close(inch.eval("1 + 1", Quantity::Length), 50.8);
        close(inch.eval("1 in + 1", Quantity::Length), 50.8);
        close(inch.eval("2 * 3", Quantity::Length), 6.0 * 25.4);
        close(inch.eval("30", Quantity::Angle), 30.0);
        // Areas in the unit squared.
        assert_eq!(inch.area(645.16), "1.000 in\u{b2}");
        let mm = Units::default();
        assert_eq!(mm.area(1234.5), "1234.500 mm\u{b2}");
        assert_eq!(mm.value(12.3456, Quantity::Length), "12.346");
        close(mm.eval("5", Quantity::Length), 5.0);
        let cm = Units::new(LengthUnit::Centimeter, 2);
        assert_eq!(cm.value(123.0, Quantity::Length), "12.3");
        close(cm.eval("1.5", Quantity::Length), 15.0);
        close(Units::new(LengthUnit::Yard, 3).eval("1", Quantity::Length), 914.4);
        close(Units::new(LengthUnit::Foot, 3).eval("1 yd", Quantity::Length), 914.4);
        assert_eq!(Units::new(LengthUnit::Meter, 9).decimals, MAX_DECIMALS);
    }

    #[test]
    fn formats_values() {
        assert_eq!(format_live(49.120_624), "49.12062");
        assert_eq!(format_live(45.738_80), "45.7388");
        assert_eq!(format_live(30.0), "30");
        assert_eq!(format_dimension(50.0), "50");
        assert_eq!(format_dimension(12.5), "12.5");
        assert_eq!(format_with_unit(50.0, Quantity::Length), "50 mm");
        assert_eq!(format_with_unit(47.7291, Quantity::Angle), "47.7291 deg");
    }
}
