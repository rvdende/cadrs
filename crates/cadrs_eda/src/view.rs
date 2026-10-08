//! A 2D canvas view (GS2): where the schematic sheet or the board sits on screen. Pan by
//! dragging, zoom about the pointer (the point under it stays put), or zoom and centre on the
//! pointer (KiCad's "center and warp cursor"), fit to a box. Screen pixels have Y down; design
//! coordinates have Y up.

use crate::units::{Bounds, NM_PER_MM, Pt};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// The design point (nm) at the viewport's centre.
    pub center: [f64; 2],
    /// Pixels per millimetre.
    pub scale: f64,
    /// Viewport size in pixels.
    pub size: [f64; 2],
}

/// How far zoom may go (pixels per mm).
pub const MIN_SCALE: f64 = 0.05;
pub const MAX_SCALE: f64 = 5000.0;

impl View {
    pub fn new(size: [f64; 2]) -> View {
        View { center: [0.0, 0.0], scale: 4.0, size }
    }

    fn k(&self) -> f64 {
        self.scale / NM_PER_MM as f64
    }

    /// Design point → screen pixel.
    pub fn to_screen(&self, p: [f64; 2]) -> [f64; 2] {
        let k = self.k();
        [self.size[0] / 2.0 + (p[0] - self.center[0]) * k, self.size[1] / 2.0 - (p[1] - self.center[1]) * k]
    }

    /// Screen pixel → design point.
    pub fn to_design(&self, s: [f64; 2]) -> [f64; 2] {
        let k = self.k();
        [self.center[0] + (s[0] - self.size[0] / 2.0) / k, self.center[1] - (s[1] - self.size[1] / 2.0) / k]
    }

    pub fn to_design_pt(&self, s: [f64; 2]) -> Pt {
        let d = self.to_design(s);
        Pt::new(d[0].round() as i64, d[1].round() as i64)
    }

    /// Drags the view by a pointer movement of `d` pixels (middle or right button).
    pub fn pan(&mut self, d: [f64; 2]) {
        let k = self.k();
        self.center[0] -= d[0] / k;
        self.center[1] += d[1] / k;
    }

    /// Zooms by `factor` (> 1 in) keeping the design point under `cursor` where it is.
    pub fn zoom_at(&mut self, cursor: [f64; 2], factor: f64) {
        let before = self.to_design(cursor);
        self.scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let after = self.to_design(cursor);
        self.center[0] += before[0] - after[0];
        self.center[1] += before[1] - after[1];
    }

    /// Zooms by `factor` and centres on the point under `cursor`; returns where the pointer
    /// should be warped to (the viewport's centre).
    pub fn zoom_center(&mut self, cursor: [f64; 2], factor: f64) -> [f64; 2] {
        self.center = self.to_design(cursor);
        self.scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        [self.size[0] / 2.0, self.size[1] / 2.0]
    }

    /// Shows all of `b` with a margin (fraction of the viewport on each side).
    pub fn fit(&mut self, b: Bounds, margin: f64) {
        let c = b.center();
        self.center = [c.x as f64, c.y as f64];
        let s = b.size();
        let (w, h) = ((s.w as f64 / NM_PER_MM as f64).max(1e-3), (s.h as f64 / NM_PER_MM as f64).max(1e-3));
        let usable = 1.0 - 2.0 * margin;
        self.scale = ((self.size[0] * usable) / w).min((self.size[1] * usable) / h).clamp(MIN_SCALE, MAX_SCALE);
    }

    /// The design box visible on screen.
    pub fn visible(&self) -> Bounds {
        let a = self.to_design([0.0, 0.0]);
        let b = self.to_design(self.size);
        Bounds { min: Pt::new(a[0].min(b[0]) as i64, a[1].min(b[1]) as i64), max: Pt::new(a[0].max(b[0]) as i64, a[1].max(b[1]) as i64) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_keeps_the_point_under_the_pointer() {
        let mut v = View::new([800.0, 600.0]);
        let cursor = [620.0, 140.0];
        let before = v.to_design(cursor);
        v.zoom_at(cursor, 1.25);
        let after = v.to_design(cursor);
        assert!((before[0] - after[0]).abs() < 1e-6 && (before[1] - after[1]).abs() < 1e-6);
        // Round trip.
        let s = v.to_screen(after);
        assert!((s[0] - cursor[0]).abs() < 1e-9 && (s[1] - cursor[1]).abs() < 1e-9);
    }

    #[test]
    fn pan_fit_and_centre_zoom() {
        let mut v = View::new([800.0, 600.0]);
        let p = v.to_design([400.0, 300.0]);
        v.pan([10.0, -20.0]);
        // The point that was at the centre moved with the pointer.
        let s = v.to_screen(p);
        assert!((s[0] - 410.0).abs() < 1e-9 && (s[1] - 280.0).abs() < 1e-9);
        // Fit a 297 × 210 mm sheet: it fills the height or width with a 5 % margin.
        v.fit(Bounds { min: Pt::ZERO, max: Pt::mm(297.0, 210.0) }, 0.05);
        let shown = v.visible();
        assert!(shown.contains(Pt::ZERO) && shown.contains(Pt::mm(297.0, 210.0)));
        assert!((v.scale - (800.0 * 0.9 / 297.0)).abs() < 1e-9);
        // Centre-and-warp: the pointed-at point moves to the middle.
        let target = v.to_design([100.0, 100.0]);
        let warp = v.zoom_center([100.0, 100.0], 2.0);
        let mid = v.to_design(warp);
        assert!((mid[0] - target[0]).abs() < 1e-6 && (mid[1] - target[1]).abs() < 1e-6);
    }
}
