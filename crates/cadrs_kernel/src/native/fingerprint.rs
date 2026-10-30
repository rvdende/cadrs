//! Geometry hashed to the bit, for the caches keyed by what a result depends on (a face's mass
//! integrals, its mesh): two SipHash streams, told apart by a first byte, make a 128-bit key
//! two different inputs don't share in practice.

use std::hash::Hasher;

use super::curve::Curve;
use super::math::Frame;
use super::surface::Surface;

/// A 128-bit key: everything written goes to both streams.
pub struct Key128([std::hash::DefaultHasher; 2]);

impl Key128 {
    /// An empty key.
    pub fn new() -> Self {
        let mut h = [
            std::hash::DefaultHasher::new(),
            std::hash::DefaultHasher::new(),
        ];
        h[1].write_u8(1);
        Self(h)
    }

    /// The key of everything written.
    pub fn key(&self) -> u128 {
        (self.0[0].finish() as u128) << 64 | self.0[1].finish() as u128
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
            Curve::Nurbs(n) => {
                self.write_u8(3);
                self.write_usize(n.degree);
                self.write_usize(n.knots.len());
                self.f64s(&n.knots);
                for p in &n.cps {
                    self.f64s(p.as_slice());
                }
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
            Surface::Nurbs(n) => {
                self.write_u8(7);
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
        }
    }
}

impl Default for Key128 {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher for Key128 {
    fn write(&mut self, bytes: &[u8]) {
        for h in &mut self.0 {
            h.write(bytes);
        }
    }

    /// The first stream's half ([`Self::key`] is the whole).
    fn finish(&self) -> u64 {
        self.0[0].finish()
    }
}
