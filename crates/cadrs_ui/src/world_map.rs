//! A low-poly map of the world with pins: where shops are, and where you are.
//!
//! Natural Earth's 1:110m coastlines (public domain, `data/coastlines-110m.bin`) drawn into an
//! image in an equirectangular projection, so it costs no assets and works headless. Pins are
//! latitude and longitude; [`project`] gives where one lands, for laying UI over the map.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use tiny_skia::{Paint, PathBuilder, Pixmap, Stroke, Transform};

const COASTLINES: &[u8] = include_bytes!("../data/coastlines-110m.bin");

/// The coastlines: lines of (longitude, latitude) in degrees.
pub fn coastlines() -> Vec<Vec<(f32, f32)>> {
    let mut lines = Vec::new();
    let mut at = 0;
    let read = |at: usize| i16::from_le_bytes([COASTLINES[at], COASTLINES[at + 1]]) as f32 / 100.0;
    while at + 2 <= COASTLINES.len() {
        let n = u16::from_le_bytes([COASTLINES[at], COASTLINES[at + 1]]) as usize;
        at += 2;
        let mut line = Vec::with_capacity(n);
        for _ in 0..n {
            line.push((read(at), read(at + 2)));
            at += 4;
        }
        lines.push(line);
    }
    lines
}

/// Where (latitude, longitude) lands on a `width` × `height` map, in pixels from its top left.
pub fn project(lat: f64, lon: f64, width: f32, height: f32) -> Vec2 {
    Vec2::new(
        ((lon + 180.0) / 360.0) as f32 * width,
        ((90.0 - lat) / 180.0) as f32 * height,
    )
}

/// A pin on the map.
#[derive(Clone, Copy, Debug)]
pub struct Pin {
    pub lat: f64,
    pub lon: f64,
    pub color: Color,
    /// How sure the place is, in metres (drawn as a halo); 0 for a point.
    pub accuracy_m: f64,
}

fn rgba(c: Color) -> tiny_skia::Color {
    let c = c.to_srgba();
    tiny_skia::Color::from_rgba(c.red, c.green, c.blue, c.alpha).unwrap_or(tiny_skia::Color::BLACK)
}

/// The map as an image `width` × `height` pixels (2:1 suits the projection).
pub fn image(width: u32, height: u32, sea: Color, coast: Color, pins: &[Pin]) -> Image {
    let mut pixmap = Pixmap::new(width.max(1), height.max(1)).expect("a size");
    pixmap.fill(rgba(sea));
    let (w, h) = (width as f32, height as f32);
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color(rgba(coast));
    let stroke = Stroke {
        width: (w / 300.0).max(1.0),
        ..Default::default()
    };
    for line in coastlines() {
        let mut path = PathBuilder::new();
        for (i, (lon, lat)) in line.iter().enumerate() {
            let p = project(*lat as f64, *lon as f64, w, h);
            if i == 0 {
                path.move_to(p.x, p.y);
            } else {
                path.line_to(p.x, p.y);
            }
        }
        if let Some(path) = path.finish() {
            pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }
    for pin in pins {
        let p = project(pin.lat, pin.lon, w, h);
        // The halo: how far off the place may be (a degree of latitude is about 111 km).
        let halo = (pin.accuracy_m / 111_000.0 / 180.0) as f32 * h;
        let mut color = rgba(pin.color);
        if halo > 3.0 {
            color.set_alpha(0.25);
            paint.set_color(color);
            if let Some(c) = PathBuilder::from_circle(p.x, p.y, halo) {
                pixmap.fill_path(
                    &c,
                    &paint,
                    tiny_skia::FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
        }
        paint.set_color(rgba(pin.color));
        let r = (w / 110.0).max(3.0);
        if let Some(c) = PathBuilder::from_circle(p.x, p.y, r) {
            pixmap.fill_path(
                &c,
                &paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
    Image::new(
        Extent3d {
            width: pixmap.width(),
            height: pixmap.height(),
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixmap.take(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_coastlines_read_back() {
        let lines = coastlines();
        assert_eq!(lines.len(), 134);
        assert_eq!(lines.iter().map(Vec::len).sum::<usize>(), 5128);
        assert!(
            lines
                .iter()
                .flatten()
                .all(|(lon, lat)| lon.abs() <= 180.0 && lat.abs() <= 90.0)
        );
    }

    #[test]
    fn places_land_where_they_should() {
        let p = project(0.0, 0.0, 720.0, 360.0);
        assert_eq!(p, Vec2::new(360.0, 180.0));
        let cape_town = project(-33.9, 18.4, 720.0, 360.0);
        assert!(cape_town.x > 360.0 && cape_town.y > 180.0);
    }

    #[test]
    fn a_map_is_drawn() {
        let img = image(
            360,
            180,
            Color::WHITE,
            Color::BLACK,
            &[Pin {
                lat: -33.9,
                lon: 18.4,
                color: Color::srgb(1.0, 0.0, 0.0),
                accuracy_m: 100_000.0,
            }],
        );
        let data = img.data.as_ref().unwrap();
        // Some coast was drawn (dark pixels on white).
        assert!(data.chunks(4).any(|p| p[0] < 100 && p[1] < 100));
        // The pin is red.
        let at = project(-33.9, 18.4, 360.0, 180.0);
        let i = ((at.y as usize) * 360 + at.x as usize) * 4;
        assert!(data[i] > 200 && data[i + 1] < 80, "{:?}", &data[i..i + 4]);
    }
}
