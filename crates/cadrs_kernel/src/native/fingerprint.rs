//! Geometry hashed to the bit, for the caches keyed by what a result depends on (a face's mass
//! integrals, its mesh, its boundary's polygons): two 64-bit streams, each taking every word
//! through a mixer of its own, make a 128-bit key two different inputs don't share in practice.
//!
//! The keys are made for every face of every boolean, of every number its curves and surface
//! hold, so the streams are cheap ones: a word is folded into each state and the state put
//! through a bijective mixer (two multiplications and three shifts), where two SipHash streams
//! fed a word at a time took most of a boolean's bookkeeping. The keys only name entries of
//! caches in memory: nothing outside the process depends on their values.

use std::hash::Hasher;

use super::curve::Curve;
use super::math::Frame;
use super::nurbs::{NurbsCurve, NurbsSurface};
use super::surface::Surface;

/// The fewest control points of a B-spline whose key is kept with it ([`kept`]): a shorter
/// one is hashed as fast as its key is looked up.
const LONG: usize = 8;

/// How many B-splines' keys a thread keeps before starting over.
const KEPT_KEYS: usize = 1 << 16;

/// B-splines' keys by their addresses, each with the B-spline itself, weakly: an address held
/// so is that B-spline's and no other's for as long as the entry stands (the allocation isn't
/// given back while a weak pointer is on it, nor its contents changed in place).
type Kept<T> = rustc_hash::FxHashMap<usize, (std::sync::Weak<T>, u128)>;

thread_local! {
    static CURVE_KEYS: std::cell::RefCell<Kept<NurbsCurve>> = std::cell::RefCell::default();
    static SURFACE_KEYS: std::cell::RefCell<Kept<NurbsSurface>> = std::cell::RefCell::default();
}

/// The key of the B-spline `of`: this thread's kept one, or `make`'s, kept.
fn kept<T>(
    table: &'static std::thread::LocalKey<std::cell::RefCell<Kept<T>>>,
    of: &std::sync::Arc<T>,
    make: impl FnOnce() -> u128,
) -> u128 {
    let at = std::sync::Arc::as_ptr(of) as usize;
    if let Some(key) = table.with(|m| m.borrow().get(&at).map(|(_, key)| *key)) {
        return key;
    }
    let key = make();
    table.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() >= KEPT_KEYS {
            m.clear();
        }
        m.insert(at, (std::sync::Arc::downgrade(of), key));
    });
    key
}

/// A 128-bit key: everything written goes to both streams.
pub struct Key128 {
    a: u64,
    b: u64,
}

/// The first stream's mixer ("moremur", a stronger variant of MurmurHash3's finalizer).
#[inline(always)]
fn mix_a(mut x: u64) -> u64 {
    x ^= x >> 27;
    x = x.wrapping_mul(0x3C79_AC49_2BA7_B653);
    x ^= x >> 33;
    x = x.wrapping_mul(0x1C69_B3F7_4AC4_AE35);
    x ^ (x >> 27)
}

/// The second stream's (SplitMix64's finalizer).
#[inline(always)]
fn mix_b(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

impl Key128 {
    /// An empty key.
    pub fn new() -> Self {
        Self {
            a: 0x243F_6A88_85A3_08D3,
            b: 0x1319_8A2E_0370_7344,
        }
    }

    /// One word into both streams. (The constants added keep a state off zero, the mixers'
    /// one fixed point, where a run of zero words would leave it.)
    #[inline(always)]
    fn word(&mut self, w: u64) {
        self.a = mix_a(self.a ^ w).wrapping_add(0x9E37_79B9_7F4A_7C15);
        self.b = mix_b(self.b.wrapping_add(w)).wrapping_add(0xD1B5_4A32_D192_ED03);
    }

    /// The key of everything written.
    pub fn key(&self) -> u128 {
        (mix_a(self.a) as u128) << 64 | mix_b(self.b) as u128
    }

    /// Numbers, by their bits.
    pub fn f64s(&mut self, xs: &[f64]) {
        for x in xs {
            self.write_u64(x.to_bits());
        }
    }

    fn frame(&mut self, f: &Frame) {
        self.f64s(f.o.coords.as_slice());
        for a in [&f.x, &f.y, &f.z] {
            self.f64s(a.as_slice());
        }
    }

    /// A curve: its kind and every number it is made of.
    pub fn curve(&mut self, c: &Curve) {
        match c {
            Curve::Line { o, d } => {
                self.write_u8(0);
                self.f64s(o.coords.as_slice());
                self.f64s(d.as_slice());
            }
            Curve::Circle { c, x, y, r } => {
                self.write_u8(1);
                for v in [&c.coords, x, y] {
                    self.f64s(v.as_slice());
                }
                self.f64s(&[*r]);
            }
            Curve::Ellipse { c, x, y, a, b } => {
                self.write_u8(2);
                for v in [&c.coords, x, y] {
                    self.f64s(v.as_slice());
                }
                self.f64s(&[*a, *b]);
            }
            // (A long B-spline by its own key, made once for as long as the curve lives: a
            // boolean keys every face of both its bodies by their curves, most of them the
            // curves the booleans before it keyed.)
            Curve::Nurbs(n) if n.cps.len() >= LONG => {
                self.write_u8(8);
                let key = kept(&CURVE_KEYS, n, || {
                    let mut h = Key128::new();
                    h.nurbs_curve(n);
                    h.key()
                });
                self.write_u128(key);
            }
            Curve::Nurbs(n) => {
                self.write_u8(3);
                self.nurbs_curve(n);
            }
        }
    }

    fn nurbs_curve(&mut self, n: &NurbsCurve) {
        self.write_usize(n.degree);
        self.write_usize(n.knots.len());
        self.f64s(&n.knots);
        for p in &n.cps {
            self.f64s(p.as_slice());
        }
    }

    fn nurbs_surface(&mut self, n: &NurbsSurface) {
        self.write_usize(n.du);
        self.write_usize(n.dv);
        self.write_usize(n.ku.len());
        self.f64s(&n.ku);
        self.write_usize(n.kv.len());
        self.f64s(&n.kv);
        self.write_usize(n.cps.len());
        for row in &n.cps {
            self.write_usize(row.len());
            for p in row {
                self.f64s(p.as_slice());
            }
        }
    }

    /// A surface: its kind and every number it is made of.
    pub fn surface(&mut self, s: &Surface) {
        match s {
            Surface::Plane { f } => {
                self.write_u8(0);
                self.frame(f);
            }
            Surface::Cylinder { f, r } => {
                self.write_u8(1);
                self.frame(f);
                self.f64s(&[*r]);
            }
            Surface::Cone { f, r, a } => {
                self.write_u8(2);
                self.frame(f);
                self.f64s(&[*r, *a]);
            }
            Surface::Sphere { f, r } => {
                self.write_u8(3);
                self.frame(f);
                self.f64s(&[*r]);
            }
            Surface::Torus { f, rmaj, rmin } => {
                self.write_u8(4);
                self.frame(f);
                self.f64s(&[*rmaj, *rmin]);
            }
            Surface::Revolution { f, m } => {
                self.write_u8(5);
                self.frame(f);
                self.curve(m);
            }
            Surface::Extrusion { c, d } => {
                self.write_u8(6);
                self.curve(c);
                self.f64s(d.as_slice());
            }
            Surface::Nurbs(n) if n.cps.len() * n.cps.first().map_or(0, Vec::len) >= LONG => {
                self.write_u8(9);
                let key = kept(&SURFACE_KEYS, n, || {
                    let mut h = Key128::new();
                    h.nurbs_surface(n);
                    h.key()
                });
                self.write_u128(key);
            }
            Surface::Nurbs(n) => {
                self.write_u8(7);
                self.nurbs_surface(n);
            }
        }
    }
}

impl Default for Key128 {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher for Key128 {
    /// Bytes eight at a time; what is left over as one word more, its length above it (so
    /// "ab" then "c" isn't "a" then "bc").
    fn write(&mut self, bytes: &[u8]) {
        let (chunks, rest) = bytes.as_chunks::<8>();
        for c in chunks {
            self.word(u64::from_le_bytes(*c));
        }
        if !rest.is_empty() {
            let mut w = [0u8; 8];
            w[..rest.len()].copy_from_slice(rest);
            w[7] = rest.len() as u8;
            self.word(u64::from_le_bytes(w));
        }
    }

    #[inline]
    fn write_u64(&mut self, w: u64) {
        self.word(w);
    }

    #[inline]
    fn write_usize(&mut self, w: usize) {
        self.word(w as u64);
    }

    /// The first stream's half ([`Self::key`] is the whole).
    fn finish(&self) -> u64 {
        mix_a(self.a)
    }
}
