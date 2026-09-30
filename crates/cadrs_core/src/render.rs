//! Render Studio (P3F.6, `intro-to-parametric-cad.md` P3.6): a tab that references a Part
//! Studio or an Assembly and renders it photorealistically with `cadrs_render`'s path tracer.
//!
//! - [`RenderStudio`] is the tab's content (saved with the document; every change goes through
//!   [`SetRenderStudio`], so it undoes): the source tab, the environment and its rotation, the
//!   background, the ground shadow, the camera (a named view or the view captured from the
//!   source tab, perspective or orthographic), the output size, samples, seed, denoising and
//!   exposure.
//! - [`scene`] turns parts into the path tracer's triangles with **PBR materials** from the
//!   parts' appearances and materials ([`pbr`]): the colour is the face's appearance (face,
//!   feature, part) and its opacity; a metal material (aluminium, steel, iron, brass, bronze,
//!   copper, titanium) makes the surface metallic with that metal's roughness, and a part with
//!   no appearance of its own shows the metal's colour; plastics and unassigned parts are
//!   dielectrics.
//! - [`camera`] frames the model from the chosen view; [`settings`] maps the tab's settings.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::appearance::{self, Appearance};
use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, ElementKind, PartProps};
use crate::ids::{ElementId, FeatureId};
use crate::parts::Part;
use crate::rebuild::Build;

pub use cadrs_render::{Background as RenderBackgroundKind, EnvironmentPreset};

/// The environment (procedural, see `cadrs_render::env`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum RenderEnvironment {
    #[default]
    Studio,
    SoftLight,
    Outdoor,
    Sunset,
}

impl RenderEnvironment {
    pub const ALL: [RenderEnvironment; 4] = [RenderEnvironment::Studio, RenderEnvironment::SoftLight, RenderEnvironment::Outdoor, RenderEnvironment::Sunset];

    pub fn preset(self) -> EnvironmentPreset {
        match self {
            RenderEnvironment::Studio => EnvironmentPreset::Studio,
            RenderEnvironment::SoftLight => EnvironmentPreset::SoftLight,
            RenderEnvironment::Outdoor => EnvironmentPreset::Outdoor,
            RenderEnvironment::Sunset => EnvironmentPreset::Sunset,
        }
    }

    pub fn label(self) -> &'static str {
        self.preset().label()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum RenderBackground {
    /// The environment's backdrop.
    #[default]
    Environment,
    White,
    /// Transparent (PNG alpha).
    Transparent,
}

impl RenderBackground {
    pub const ALL: [RenderBackground; 3] = [RenderBackground::Environment, RenderBackground::White, RenderBackground::Transparent];

    pub fn label(self) -> &'static str {
        match self {
            RenderBackground::Environment => "Environment",
            RenderBackground::White => "White",
            RenderBackground::Transparent => "Transparent",
        }
    }
}

/// Where the camera looks from: a named view, or the view captured from the source tab.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum RenderView {
    /// The source tab's view when it was captured: azimuth, elevation and roll (degrees, as
    /// the 3D view's camera).
    Current { azimuth: f32, elevation: f32, roll: f32 },
    Isometric,
    #[default]
    Trimetric,
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
}

impl RenderView {
    /// The named views, in the Camera list's order.
    pub const NAMED: [RenderView; 8] = [
        RenderView::Isometric,
        RenderView::Trimetric,
        RenderView::Front,
        RenderView::Back,
        RenderView::Left,
        RenderView::Right,
        RenderView::Top,
        RenderView::Bottom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            RenderView::Current { .. } => "Current view",
            RenderView::Isometric => "Isometric",
            RenderView::Trimetric => "Trimetric",
            RenderView::Front => "Front",
            RenderView::Back => "Back",
            RenderView::Left => "Left",
            RenderView::Right => "Right",
            RenderView::Top => "Top",
            RenderView::Bottom => "Bottom",
        }
    }

    /// (azimuth, elevation, roll) in degrees, as the 3D view's standard views.
    pub fn angles(self) -> (f32, f32, f32) {
        match self {
            RenderView::Current { azimuth, elevation, roll } => (azimuth, elevation, roll),
            RenderView::Isometric => (45.0, 35.264_39, 0.0),
            RenderView::Trimetric => (30.0, 30.0, 0.0),
            RenderView::Front => (0.0, 0.0, 0.0),
            RenderView::Back => (180.0, 0.0, 0.0),
            RenderView::Left => (-90.0, 0.0, 0.0),
            RenderView::Right => (90.0, 0.0, 0.0),
            RenderView::Top => (0.0, 90.0, 0.0),
            RenderView::Bottom => (0.0, -90.0, 0.0),
        }
    }

    /// The unit vector toward the camera and the screen's up.
    pub fn frame(self) -> ([f32; 3], [f32; 3]) {
        let (az, el, roll) = self.angles();
        let (a, e) = (az.to_radians(), el.to_radians());
        let back = [a.sin() * e.cos(), -a.cos() * e.cos(), e.sin()];
        // Screen right with no roll is horizontal; up = back × right.
        let r0 = [a.cos(), a.sin(), 0.0];
        let cross = |u: [f32; 3], v: [f32; 3]| [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let u0 = cross(back, r0);
        let (s, c) = roll.to_radians().sin_cos();
        let right = [r0[0] * c + u0[0] * s, r0[1] * c + u0[1] * s, r0[2] * c + u0[2] * s];
        (back, cross(back, right))
    }
}

/// A Render Studio tab's settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderStudio {
    /// The Part Studio or Assembly rendered.
    pub source: Option<ElementId>,
    #[serde(default)]
    pub environment: RenderEnvironment,
    /// Turns the environment's lights about the vertical (degrees).
    #[serde(default)]
    pub environment_rotation: f32,
    #[serde(default)]
    pub background: RenderBackground,
    #[serde(default = "yes")]
    pub ground_shadow: bool,
    #[serde(default)]
    pub view: RenderView,
    #[serde(default = "yes")]
    pub perspective: bool,
    pub width: u32,
    pub height: u32,
    pub samples: u32,
    pub seed: u64,
    #[serde(default = "yes")]
    pub denoise: bool,
    /// Stops.
    #[serde(default)]
    pub exposure: f32,
}

fn yes() -> bool {
    true
}

impl RenderStudio {
    pub fn new(source: Option<ElementId>) -> Self {
        RenderStudio {
            source,
            environment: RenderEnvironment::Studio,
            environment_rotation: 0.0,
            background: RenderBackground::Environment,
            ground_shadow: true,
            view: RenderView::Trimetric,
            perspective: true,
            width: 1920,
            height: 1080,
            samples: 64,
            seed: 1,
            denoise: true,
            exposure: 0.0,
        }
    }
}

/// The resolutions offered (width, height, label).
pub const RESOLUTIONS: [(u32, u32, &str); 5] = [
    (1280, 720, "1280 × 720 (HD)"),
    (1920, 1080, "1920 × 1080 (Full HD)"),
    (2560, 1440, "2560 × 1440 (QHD)"),
    (3840, 2160, "3840 × 2160 (4K)"),
    (1080, 1080, "1080 × 1080 (square)"),
];

/// The sample counts offered (samples, label).
pub const QUALITIES: [(u32, &str); 4] = [(16, "Draft (16 samples)"), (64, "Good (64 samples)"), (256, "High (256 samples)"), (1024, "Best (1024 samples)")];

/// A Render Studio tab's settings (`None` for other tabs).
pub fn studio(doc: &Document, element: ElementId) -> Option<&RenderStudio> {
    match &doc.element(element)?.kind {
        ElementKind::Render(r) => Some(r),
        _ => None,
    }
}

/// Replaces a Render Studio tab's settings.
#[derive(Debug, Clone)]
pub struct SetRenderStudio {
    pub element: ElementId,
    pub studio: RenderStudio,
    /// Shown in the undo menu ("Environment Outdoor").
    pub label: String,
}

impl Command for SetRenderStudio {
    fn label(&self) -> String {
        self.label.clone()
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if let Some(s) = self.studio.source
            && !doc.element(s).is_some_and(|e| matches!(e.kind, ElementKind::PartStudio { .. } | ElementKind::Assembly))
        {
            return Err(CommandError::Invalid("a render needs a Part Studio or an Assembly".into()));
        }
        if self.studio.width == 0 || self.studio.height == 0 || self.studio.width > 8192 || self.studio.height > 8192 {
            return Err(CommandError::Invalid("the size must be 1 to 8192 pixels".into()));
        }
        let el = doc.element_mut(self.element).ok_or(CommandError::ElementNotFound(self.element))?;
        match &mut el.kind {
            ElementKind::Render(r) => {
                **r = self.studio.clone();
                Ok(())
            }
            _ => Err(CommandError::Invalid("not a Render Studio".into())),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Materials and the scene

/// A metal a material's name names, with its colour (sRGB) and roughness.
fn metal(name: &str) -> Option<([u8; 3], f32)> {
    let n = name.to_ascii_lowercase();
    let table: [(&str, [u8; 3], f32); 8] = [
        ("alumin", [214, 218, 222], 0.32),
        ("stainless", [196, 198, 200], 0.28),
        ("cast iron", [120, 120, 122], 0.6),
        ("steel", [170, 172, 176], 0.35),
        ("iron", [150, 150, 152], 0.5),
        ("brass", [225, 190, 110], 0.28),
        ("bronze", [196, 142, 90], 0.35),
        ("copper", [230, 150, 120], 0.3),
    ];
    table
        .iter()
        .find(|(k, _, _)| n.contains(k))
        .map(|(_, c, r)| (*c, *r))
        .or_else(|| n.contains("titanium").then_some(([180, 176, 170], 0.4)))
}

/// Glossy plastics (roughness).
fn plastic_roughness(name: &str) -> f32 {
    let n = name.to_ascii_lowercase();
    if n.contains("acrylic") || n.contains("polycarbonate") {
        0.12
    } else if n.contains("abs") || n.contains("acetal") {
        0.35
    } else {
        0.45
    }
}

/// A face's PBR material: its appearance's colour and opacity, finished by the part's material.
/// `explicit` is true when the colour was chosen (a face, feature or part appearance), not the
/// default palette colour: a metal without a chosen colour shows the metal's own.
pub fn pbr(appearance: Appearance, explicit: bool, material: Option<&crate::material::Material>) -> cadrs_render::Material {
    let opacity = appearance.alpha as f32 / 255.0;
    match material.map(|m| (m, metal(&m.name))) {
        Some((_, Some((colour, roughness)))) => {
            let rgb = if explicit { appearance.rgb } else { colour };
            cadrs_render::Material::from_srgb(rgb, 1.0, roughness, opacity)
        }
        Some((m, None)) => cadrs_render::Material::from_srgb(appearance.rgb, 0.0, plastic_roughness(&m.name), opacity),
        None => cadrs_render::Material::from_srgb(appearance.rgb, 0.0, 0.42, opacity),
    }
}

/// A short description of a part's render material ("Aluminum - 6061: metal, roughness 0.32").
pub fn describe(part: &Part, props: &[PartProps]) -> String {
    match crate::parts::part_material(part, props) {
        Some(m) => match metal(&m.name) {
            Some((_, r)) => format!("{}: metal, roughness {r:.2}", m.name),
            None => format!("{}: dielectric, roughness {:.2}", m.name, plastic_roughness(&m.name)),
        },
        None => "No material: dielectric, roughness 0.42".into(),
    }
}

/// The parts shown (not hidden) as the path tracer's scene.
pub fn scene(parts: &[Part], props: &[PartProps], appearances: &[(FeatureId, Appearance)]) -> cadrs_render::Scene {
    let mut b = cadrs_render::SceneBuilder::new();
    for part in parts {
        if props.iter().any(|p| p.part == part.id && p.hidden) {
            continue;
        }
        let s = &part.solid;
        let material = crate::parts::part_material(part, props);
        let own_colour = props.iter().any(|p| (p.part == part.id || Some(p.part) == part.source) && p.appearance.is_some());
        // Each face's material, by triangle.
        let mut tri_material = vec![u32::MAX; s.indices.len() / 3];
        for f in &s.faces {
            let (a, source) = appearance::face_appearance(part, &f.name, props, appearances);
            let explicit = own_colour || source != appearance::Source::Part;
            let m = b.material(pbr(a, explicit, material));
            for k in f.first_triangle..(f.first_triangle + f.triangle_count).min(tri_material.len()) {
                tri_material[k] = m;
            }
        }
        let fallback = b.material(pbr(appearance::part_appearance(part, props), own_colour, material));
        let pos: Vec<[f32; 3]> = s.positions.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
        let nrm: Vec<[f32; 3]> = s.normals.iter().map(|n| [n[0] as f32, n[1] as f32, n[2] as f32]).collect();
        b.add_mesh(&pos, &nrm, &s.indices, |k| match tri_material.get(k) {
            Some(&m) if m != u32::MAX => m,
            _ => fallback,
        });
    }
    b.build()
}

/// Parts, their settings and the feature appearances they show.
pub type SourceParts = (Vec<Part>, Vec<PartProps>, Vec<(FeatureId, Appearance)>);

/// The source tab's parts, their settings and its (or its source studios') feature
/// appearances. `build_of` gives a Part Studio's current rebuild.
pub fn source_parts(
    doc: &Document,
    source: ElementId,
    mut build_of: impl FnMut(ElementId) -> Option<Arc<Build>>,
) -> Option<SourceParts> {
    let el = doc.element(source)?;
    match &el.kind {
        ElementKind::PartStudio { .. } => {
            let b = build_of(source)?;
            Some((b.parts.clone(), el.part_props().to_vec(), el.feature_appearances().to_vec()))
        }
        ElementKind::Assembly => {
            let (parts, props) = crate::assembly::instance_parts(doc, &el.assembly, &mut build_of);
            let mut appearances = Vec::new();
            for o in crate::assembly::structure::occurrences(doc, &el.assembly) {
                if let Some(e) = doc.element(o.element) {
                    for a in e.feature_appearances() {
                        if !appearances.contains(a) {
                            appearances.push(*a);
                        }
                    }
                }
            }
            Some((parts, props, appearances))
        }
        _ => None,
    }
}

/// The path tracer's settings for a studio's, at `width` × `height` with `samples`.
pub fn settings(r: &RenderStudio, width: u32, height: u32, samples: u32) -> cadrs_render::Settings {
    cadrs_render::Settings {
        width,
        height,
        samples: samples.max(1),
        seed: r.seed,
        environment: r.environment.preset(),
        environment_rotation: r.environment_rotation,
        background: match r.background {
            RenderBackground::Environment => cadrs_render::Background::Environment,
            RenderBackground::White => cadrs_render::Background::White,
            RenderBackground::Transparent => cadrs_render::Background::Transparent,
        },
        ground_shadow: r.ground_shadow,
        exposure: r.exposure,
        denoise: r.denoise,
        max_bounces: 5,
    }
}

/// The camera: the studio's view, framing the scene at `aspect` (width / height).
pub fn camera(r: &RenderStudio, scene: &cadrs_render::Scene, aspect: f32) -> cadrs_render::Camera {
    let (back, up) = r.view.frame();
    let bounds = scene.bounds().unwrap_or(([-50.0; 3], [50.0; 3]));
    let projection = if r.perspective { cadrs_render::Projection::Perspective { fov_y: 30.0 } } else { cadrs_render::Projection::Orthographic { height: 1.0 } };
    cadrs_render::Camera::fit(bounds, back, up, projection, aspect, 0.08)
}

/// Renders a studio's source at its size and samples on `threads` threads (tests and scripted
/// runs; the app renders progressively).
pub fn render(doc: &Document, r: &RenderStudio, build_of: impl FnMut(ElementId) -> Option<Arc<Build>>, threads: usize) -> Option<image::RgbaImage> {
    let (parts, props, appearances) = source_parts(doc, r.source?, build_of)?;
    let scene = Arc::new(scene(&parts, &props, &appearances));
    let cam = camera(r, &scene, r.width as f32 / r.height as f32);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    cadrs_render::render(scene, cam, settings(r, r.width, r.height, r.samples), threads, &cancel, |_, _| {})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_match_the_3d_view() {
        // Front: the camera on −Y, Z up.
        let (b, u) = RenderView::Front.frame();
        assert!((b[1] + 1.0).abs() < 1e-6 && (u[2] - 1.0).abs() < 1e-6);
        // Top: from +Z with +Y up.
        let (b, u) = RenderView::Top.frame();
        assert!((b[2] - 1.0).abs() < 1e-6 && (u[1] - 1.0).abs() < 1e-5, "{u:?}");
        // Isometric: equal components.
        let (b, _) = RenderView::Isometric.frame();
        assert!((b[0] - b[2]).abs() < 1e-4 && (b[0] + b[1]).abs() < 1e-4);
    }

    #[test]
    fn materials_finish_appearances() {
        let blue = Appearance::rgb(40, 90, 200);
        let al = crate::material::library("Aluminum - 6061").unwrap();
        // A metal: metallic, its own colour unless one was chosen.
        let m = pbr(blue, false, Some(&al));
        assert_eq!(m.metallic, 1.0);
        assert!(m.base_color[0] > 0.6);
        let m = pbr(blue, true, Some(&al));
        assert!(m.base_color[2] > m.base_color[0]);
        // A plastic and no material: dielectric in the appearance's colour; opacity kept.
        let abs = crate::material::library("ABS").unwrap();
        let m = pbr(blue.with_alpha(128), false, Some(&abs));
        assert_eq!(m.metallic, 0.0);
        assert!((m.opacity - 128.0 / 255.0).abs() < 1e-6);
        assert_eq!(pbr(blue, false, None).metallic, 0.0);
    }
}
