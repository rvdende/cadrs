//! Glass surfaces: see-through slabs over a soft wallpaper, after the glass themes of Raven
//! (`src/widgets/common.c`, `rvn_glass_style`).
//!
//! - A [`Glass`] node gets a tint (its fill), a rim and a shadow, each one flat colour: no
//!   gradients (issue #137: they looked dated). The rim is a faint light line in dark (brighter
//!   on clearer glass) and a soft dark outline in light.
//! - The material ([`GlassMaterial`], one for the app in [`GlassStyle`] or one per node) says
//!   how much shows through: **clear** glass is barely tinted, **frosted**
//!   glass is milkier, **tinted** glass is frosted with the accent mixed into it (more on what
//!   floats higher). Bevy has no backdrop blur, so frost is a milkier tint: over the soft
//!   wallpaper the two look alike.
//! - The tint and rim are set as one-colour gradients, which [`crate::color_scheme`] doesn't
//!   map, so they are set again whenever the scheme or the material changes; the shadow is an
//!   ordinary [`BoxShadow`],
//!   which the scheme deepens in dark.
//! - [`wallpaper`] goes on a page's root: a flat pale cyan in light, a warm dark ground in dark.
//! - [`glow`] is the halo of a page's main action.
//!
//! ```ignore
//! commands.spawn((panel_node(&theme), Glass::panel(), panel_shadow(&theme)));
//! ```

use bevy::prelude::*;

use crate::color_scheme::{ActiveTheme, Appearance};
use crate::theme::Theme;

pub struct GlassPlugin;

impl Plugin for GlassPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GlassStyle>().add_systems(
            PostUpdate,
            (shape_frames, sync_wallpapers, sync_glass)
                .chain()
                .before(bevy::ui::UiSystems::Prepare),
        );
    }
}

/// What glass surfaces are made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlassMaterial {
    /// Barely tinted: the most of what is behind.
    Clear,
    /// Milky: what is behind shows as color, not shape.
    #[default]
    Frosted,
    /// Frosted, with the accent mixed into the tint.
    Tinted,
}

/// The app's glass material (a node can name its own).
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GlassStyle {
    pub material: GlassMaterial,
}

/// What a glass surface is, which sets how thick its tint is and how much accent tinted
/// glass takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlassKind {
    /// A card or panel on the page.
    Panel,
    /// A bar across the window (the top bar).
    Bar,
    /// A button or pill.
    Control,
    /// A text field.
    Input,
}

/// A glass surface. Give its node a border (the rim) and a radius ([`panel_node`]).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Glass {
    pub kind: GlassKind,
    /// `None`: the app's [`GlassStyle`].
    pub material: Option<GlassMaterial>,
    /// Whether the glass paints its tint. A control leaves its fill to its
    /// [`crate::Visuals`] (hover and press) and takes only the rim.
    pub fill: bool,
}

impl Glass {
    pub fn panel() -> Self {
        Self {
            kind: GlassKind::Panel,
            material: None,
            fill: true,
        }
    }

    pub fn bar() -> Self {
        Self {
            kind: GlassKind::Bar,
            material: None,
            fill: true,
        }
    }

    /// The rim of a control whose fill follows its state.
    pub fn control_rim() -> Self {
        Self {
            kind: GlassKind::Control,
            material: None,
            fill: false,
        }
    }

    /// The rim of a text field (its fill follows its focus).
    pub fn input_rim() -> Self {
        Self {
            kind: GlassKind::Input,
            material: None,
            fill: false,
        }
    }

    pub fn material(mut self, m: GlassMaterial) -> Self {
        self.material = Some(m);
        self
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    let (a, b) = (a.to_linear(), b.to_linear());
    Color::LinearRgba(LinearRgba::new(
        a.red + (b.red - a.red) * t,
        a.green + (b.green - a.green) * t,
        a.blue + (b.blue - a.blue) * t,
        a.alpha + (b.alpha - a.alpha) * t,
    ))
}

/// The tint and the rim's light (in dark), for `kind` in `material`.
fn glass_look(t: &Theme, dark: bool, kind: GlassKind, material: GlassMaterial) -> (Color, f32) {
    // Raven: panels and inputs take the panel color, controls their own; the rim catches
    // more light on a pale surface.
    let alpha = match (kind, dark) {
        (GlassKind::Panel, false) => 0.9,
        // Dark glass over the grid wallpaper: nearly solid, so the crosses don't show through
        // the rows.
        (GlassKind::Panel, true) => 0.86,
        (GlassKind::Bar, false) => 0.5,
        (GlassKind::Bar, true) => 0.8,
        (GlassKind::Control, false) => 0.7,
        (GlassKind::Control, true) => 0.55,
        (GlassKind::Input, _) => 0.6,
    };
    // A shade lighter than the page and the 3D view behind, so a panel floats over them (in
    // light, a cool frost over the white).
    let base = if dark {
        Color::srgb_u8(0x2a, 0x29, 0x25)
    } else {
        Color::WHITE
    };
    // Raven's 0.55 in dark reads as an outline on our charcoal: a faint catch of light.
    let mut rim = if dark { 0.2 } else { 0.85 };
    let mut tint = base.with_alpha(alpha);
    match material {
        GlassMaterial::Clear => {
            tint = tint.with_alpha(alpha * 0.55);
            rim = (rim * 1.2_f32).min(1.0);
        }
        GlassMaterial::Frosted => {
            // No blur: frost is a milkier tint.
            tint = tint.with_alpha(alpha + 0.08);
        }
        GlassMaterial::Tinted => {
            let strength = match kind {
                GlassKind::Input => 0.25,
                GlassKind::Bar => 0.4,
                _ => 0.55,
            };
            let a = (alpha * 1.35 + 0.14).clamp(0.0, 0.78);
            let accent = if dark { t.primary_active } else { t.primary };
            tint = mix(base, accent, strength * if dark { 0.45 } else { 0.3 }).with_alpha(a);
        }
    }
    // A palette made for solid surfaces would hide everything: glass keeps some of what is
    // behind.
    if tint.alpha() > 0.9 {
        tint = tint.with_alpha(0.9);
    }
    if matches!(kind, GlassKind::Input) {
        rim *= 0.55;
    } else if matches!(kind, GlassKind::Bar) {
        rim *= 0.6;
    }
    (tint, rim)
}

fn sync_glass(
    appearance: Res<Appearance>,
    style: Res<GlassStyle>,
    theme: Res<ActiveTheme>,
    mut commands: Commands,
    q: Query<(Entity, &Glass, Ref<Glass>)>,
) {
    let all = appearance.is_changed() || style.is_changed() || theme.is_changed();
    let dark = appearance.is_dark();
    for (e, g, r) in &q {
        if !(all || r.is_changed()) {
            continue;
        }
        let (tint, rim) = glass_look(&theme, dark, g.kind, g.material.unwrap_or(style.material));
        let white = |a: f32| Color::WHITE.with_alpha(a.clamp(0.0, 1.0));
        // One colour all round: a faint light line in dark, a soft dark outline in light (a
        // white rim vanishes on a pale page).
        let ink = |a: f32| Color::srgba(0.0, 0.2, 0.24, a);
        let border = flat_border(if dark { white(rim * 0.35) } else { ink(0.16) });
        let mut ec = commands.entity(e);
        // Controls and fields have no outline: their fill is enough.
        if matches!(g.kind, GlassKind::Control | GlassKind::Input) {
            ec.try_remove::<BorderGradient>()
                .try_insert(BorderColor::all(Color::NONE));
        } else {
            ec.try_insert(border);
        }
        if g.fill {
            ec.try_insert(BackgroundGradient::from(LinearGradient::to_bottom(vec![
                ColorStop::new(tint, Val::Percent(0.0)),
                ColorStop::new(tint, Val::Percent(100.0)),
            ])));
        }
    }
}

/// A one-colour rim (a gradient, so the scheme leaves it to [`sync_glass`]).
fn flat_border(c: Color) -> BorderGradient {
    BorderGradient::from(LinearGradient::to_bottom(vec![
        ColorStop::new(c, Val::Percent(0.0)),
        ColorStop::new(c, Val::Percent(100.0)),
    ]))
}

/// The soft shadow under a glass panel.
pub fn panel_shadow(t: &Theme) -> BoxShadow {
    BoxShadow(vec![
        ShadowStyle {
            color: t.glass_shadow,
            x_offset: Val::ZERO,
            y_offset: Val::Px(10.0),
            spread_radius: Val::Px(-4.0),
            blur_radius: Val::Px(30.0),
        },
        ShadowStyle {
            color: t.glass_shadow.with_alpha(t.glass_shadow.alpha() * 0.5),
            x_offset: Val::ZERO,
            y_offset: Val::Px(1.0),
            spread_radius: Val::ZERO,
            blur_radius: Val::Px(3.0),
        },
    ])
}

/// A glass panel: its tint, rim and shadow (its node needs a border: [`panel_node`]).
pub fn panel(t: &Theme) -> impl Bundle {
    (Glass::panel(), panel_shadow(t))
}

/// A glass panel's node: a 1 px rim and the theme's panel radius.
pub fn panel_node(t: &Theme) -> Node {
    Node {
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::all(Val::Px(t.radius_panel)),
        flex_direction: FlexDirection::Column,
        ..default()
    }
}

/// A soft halo in the primary color, for the one main action of a page.
pub fn glow(t: &Theme) -> BoxShadow {
    BoxShadow(vec![ShadowStyle {
        color: t.primary.with_alpha(0.4),
        x_offset: Val::ZERO,
        y_offset: Val::Px(3.0),
        spread_radius: Val::ZERO,
        blur_radius: Val::Px(20.0),
    }])
}

// ---------------------------------------------------------------------------------------------
// Controls

/// The height of a page's controls (buttons, fields, toggles), so a row of them lines up.
pub const CONTROL_HEIGHT: f32 = 32.0;
/// Their corner radius.
pub const CONTROL_RADIUS: f32 = 5.0;
/// The height of a top bar's controls.
pub const BAR_CONTROL_HEIGHT: f32 = 32.0;
/// Their corner radius.
pub const BAR_CONTROL_RADIUS: f32 = 5.0;

/// A secondary control's colors: a soft fill with no outline, deeper when hovered, the
/// accent's label when selected.
pub fn control_visuals(t: &Theme) -> crate::Visuals {
    crate::Visuals {
        background: crate::StateColors::new(t.control, t.control_hover, t.list_active, t.control)
            .with_selected(t.list_selected),
        border: crate::StateColors::all(Color::NONE),
        foreground: crate::StateColors::new(
            t.foreground,
            t.foreground,
            t.foreground,
            t.disabled_foreground,
        )
        .with_selected(t.link),
        focus_ring: t.focus_ring,
    }
}

/// Gives a button the secondary control's look: its colors, and no outline.
pub fn control(t: &Theme) -> impl Bundle {
    (control_visuals(t), Glass::control_rim())
}

/// A control node's shape: the page's (or a top bar's) height and radius, with a 1 px rim.
pub fn shape_control(n: &mut Node, height: f32, radius: f32) {
    n.height = Val::Px(height);
    n.border = UiRect::all(Val::Px(1.0));
    n.border_radius = BorderRadius::all(Val::Px(radius));
}

/// A glass frame around a group of buttons (a segmented control, the top bar's settings):
/// `height` and `radius` outside; the buttons in it get the radius less the inset, so the
/// corners nest.
#[derive(Component, Debug, Clone, Copy)]
pub struct GlassFrame {
    pub height: f32,
    pub radius: f32,
}

impl GlassFrame {
    /// A page control's frame.
    pub fn page() -> Self {
        Self {
            height: CONTROL_HEIGHT,
            radius: CONTROL_RADIUS,
        }
    }

    /// A top bar control's frame.
    pub fn bar() -> Self {
        Self {
            height: BAR_CONTROL_HEIGHT,
            radius: BAR_CONTROL_RADIUS,
        }
    }
}

const FRAME_INSET: f32 = 3.0;

#[allow(clippy::type_complexity)]
fn shape_frames(
    theme: Res<Theme>,
    mut commands: Commands,
    q: Query<(Entity, &GlassFrame, Option<&Children>), Or<(Added<GlassFrame>, Changed<Children>)>>,
    mut nodes: Query<&mut Node>,
) {
    for (e, f, children) in &q {
        if let Ok(mut n) = nodes.get_mut(e) {
            shape_control(&mut n, f.height, f.radius);
            n.padding = UiRect::all(Val::Px(FRAME_INSET - 1.0));
            n.column_gap = Val::Px(2.0);
            n.align_items = AlignItems::Center;
        }
        commands.entity(e).try_insert((
            Glass::control_rim(),
            BackgroundColor(theme.control),
            BorderColor::all(Color::NONE),
        ));
        let inner = f.height - 2.0 * FRAME_INSET;
        for c in children.into_iter().flatten() {
            if let Ok(mut n) = nodes.get_mut(*c) {
                n.height = Val::Px(inner);
                n.min_width = Val::Px(inner);
                n.border = UiRect::ZERO;
                n.border_radius = BorderRadius::all(Val::Px((f.radius - FRAME_INSET).max(2.0)));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The wallpaper

/// A node painted with the scheme's wallpaper.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Wallpaper;

/// The dark wallpaper's pattern: the schematic's grid, a faint line every [`GRID_PX`] with a
/// cross where two meet.
#[derive(Component, Debug, Clone, Copy, Default)]
struct WallpaperGrid;

/// The wallpaper grid's pitch (logical px).
const GRID_PX: u32 = 44;

/// The wallpaper for a page's root node (its first child is the dark scheme's grid).
pub fn wallpaper() -> impl Bundle {
    (
        Wallpaper,
        BackgroundGradient(wallpaper_layers(false)),
        Children::spawn(Spawn((
            Name::new("wallpaper-grid"),
            WallpaperGrid,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            Visibility::Hidden,
            Pickable::IGNORE,
        ))),
    )
}

/// One tile of the wallpaper grid: lines through the middle in the schematic's faint grid
/// colour, a brighter cross where they meet.
fn grid_tile() -> Image {
    let n = GRID_PX as usize;
    let c = n / 2;
    let mut px = vec![0u8; n * n * 4];
    let mut put = |x: usize, y: usize, rgba: [u8; 4]| {
        let i = (y * n + x) * 4;
        px[i..i + 4].copy_from_slice(&rgba);
    };
    let line = [0x5a, 0x52, 0x3e, 0x1c];
    let cross = [0x8a, 0x80, 0x66, 0x5c];
    for i in 0..n {
        put(i, c, line);
        put(c, i, line);
    }
    for d in 0..=4 {
        for (x, y) in [(c + d, c), (c - d, c), (c, c + d), (c, c - d)] {
            put(x, y, cross);
        }
    }
    Image::new(
        bevy::render::render_resource::Extent3d {
            width: GRID_PX,
            height: GRID_PX,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        px,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    )
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::srgba_u8(r, g, b, (a * 255.0).round() as u8)
}

/// The page's ground: one flat colour per scheme.
fn wallpaper_layers(dark: bool) -> Vec<Gradient> {
    // One flat colour (issue #137: no gradients): the schematic's warm ground in dark, a pale
    // cyan in light. A one-colour gradient, as the scheme doesn't remap gradients.
    let ground = if dark {
        rgba(0x20, 0x1f, 0x1c, 1.0)
    } else {
        rgba(0xe7, 0xf0, 0xf5, 1.0)
    };
    vec![
        LinearGradient::to_bottom(vec![
            ColorStop::new(ground, Val::Percent(0.0)),
            ColorStop::new(ground, Val::Percent(100.0)),
        ])
        .into(),
    ]
}

#[allow(clippy::type_complexity)]
fn sync_wallpapers(
    appearance: Res<Appearance>,
    mut q: Query<(&mut BackgroundGradient, Ref<Wallpaper>)>,
    mut grids: Query<(Entity, &mut Visibility, Has<ImageNode>), With<WallpaperGrid>>,
    mut images: ResMut<Assets<Image>>,
    mut tile: Local<Option<Handle<Image>>>,
    mut commands: Commands,
) {
    let dark = appearance.is_dark();
    for (mut g, w) in &mut q {
        if appearance.is_changed() || w.is_added() {
            g.0 = wallpaper_layers(dark);
        }
    }
    for (e, mut vis, has_image) in &mut grids {
        vis.set_if_neq(if dark {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        if !has_image {
            let handle = tile.get_or_insert_with(|| images.add(grid_tile())).clone();
            commands.entity(e).try_insert(ImageNode {
                image: handle,
                image_mode: NodeImageMode::Tiled {
                    tile_x: true,
                    tile_y: true,
                    stretch_value: 1.0,
                },
                ..default()
            });
        }
    }
}
