//! The view ball in the viewport's bottom right corner: a glass ball that turns with the view.
//! The X, Y and Z axes (red, green, blue) run from its centre to knobs on its surface, the
//! negative axes' knobs paler; three rings in the axis planes show how it is turned, and what is
//! on its far side fades behind the glass. Clicking a knob animates the view to that standard
//! view (X the Right view, Y the Back, Z the Top; the pale knobs Left, Front and Bottom);
//! clicking one of the small dots between them (P3.9, P6.4: a corner) turns it to the
//! trimetric view from that corner; dragging the ball turns the view freely. The ring round it
//! has nudge arrows (15°) and, top left and right, roll arrows (15° about the view direction).
//! Hovering names what a click does, beside the ball.
//!
//! The ball is a tiny 3D scene on render layer 1, drawn by its own camera into an image that a
//! UI node shows, so it composes with the UI like any other widget. The Repair panel's view and
//! the Sheet metal table's flat view have balls of their own, cameras on the same scene.

use crate::GizmoGroups;
use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, ScalingMode};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendState, RenderPipelineDescriptor, ShaderType, TextureFormat,
};
use bevy::shader::ShaderRef;
use bevy::text::FontWeight;
use bevy::ui_render::prelude::{MaterialNode, UiMaterial, UiMaterialKey, UiMaterialPlugin};
use bevy::ui_widgets::Activate;
use bevy::ui_widgets::popover::PopoverSide;
use cadrs_ui::i18n::tr;
use cadrs_ui::{Theme, by_scheme};

use crate::AppState;
use crate::camera::{StandardView, ViewState};
use crate::viewport::ViewportView;

pub struct ViewCubePlugin;

impl Plugin for ViewCubePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "view_ball.wesl");
        embedded_asset!(app, "view_ball_image.wesl");
        app.add_plugins((
            MaterialPlugin::<BallMaterial>::default(),
            UiMaterialPlugin::<BallImageMaterial>::default(),
        ))
        .gizmo_group::<CubeArcGizmos>()
        .gizmo_group::<RollGizmos>()
        .init_resource::<BallHover>()
        .init_resource::<BallDrag>()
        .add_systems(Startup, setup_cube)
        .add_systems(
            Update,
            (
                sync_cube_camera,
                cube_hover,
                style_ball,
                draw_cube,
                show_caption,
                render_cube_when_changed,
            )
                .chain()
                .after(crate::viewport::apply_view_to_camera)
                .run_if(in_state(AppState::Document)),
        )
        .add_systems(
            Update,
            (attach_ball_images, sync_view_options_chevron).run_if(in_state(AppState::Document)),
        )
        .add_systems(
            PostUpdate,
            place_cube_labels
                .before(bevy::ui::UiSystems::Layout)
                .run_if(in_state(AppState::Document)),
        )
        .add_systems(OnEnter(AppState::Document), activate_cube_camera::<true>)
        .add_systems(OnExit(AppState::Document), activate_cube_camera::<false>)
        .add_observer(on_cube_click)
        .add_observer(on_ball_drag_start)
        .add_observer(on_ball_drag)
        .add_observer(on_ball_drag_end)
        .add_observer(on_arrow)
        .add_observer(on_roll);
    }
}

/// Size of the view ball widget (logical px): the ball, its knobs and the ring round them.
pub const CUBE_WIDGET: Vec2 = Vec2::new(136.0, 136.0);

/// The screen rect the view ball widget covers in a viewport (bottom right). Sketch glyphs and
/// labels are not drawn over it.
pub fn cube_rect(viewport: Rect) -> Rect {
    Rect::new(
        viewport.max.x - CUBE_WIDGET.x,
        viewport.max.y - CUBE_WIDGET.y,
        viewport.max.x,
        viewport.max.y,
    )
}
/// Where the ball's centre sits in the widget.
const CUBE_CENTER: Vec2 = Vec2::new(68.0, 68.0);
/// Pixels per ball unit (the ball's radius is one unit).
const CUBE_PX: f32 = 36.0;
/// A knob's radius, its centre on the ball's surface.
const KNOB_R: f32 = 0.2;
/// A corner dot's radius.
const DOT_R: f32 = 0.075;
/// The axes' radius, and the origin's.
const AXIS_R: f32 = 0.035;
const ORIGIN_R: f32 = 0.07;
/// The rings' tube radius.
const RING_TUBE: f32 = 0.014;
/// The ring round the ball (the nudge and roll arrows sit on it), in ball units.
const RING_R: f32 = 1.5;
/// How far from a knob's or dot's outline a pointer still picks it (ball units).
const HIT_SLOP: f32 = 0.05;
/// The ring's line width in the image (twice the logical width, [`CUBE_SUPERSAMPLE`]).
pub(crate) const RING_WIDTH: f32 = 2.6;
/// The ball image is rendered at twice the widget size for crisp edges.
pub(crate) const CUBE_SUPERSAMPLE: f32 = 2.0;
pub(crate) const CUBE_LAYER: usize = 1;
/// The main ball's ring and roll arrows: drawn in the main view's screen plane, so only the
/// main ball's camera sees them (the Repair ball draws its own on [`REPAIR_CUBE_ARC_LAYER`]).
pub(crate) const CUBE_ARC_LAYER: usize = 3;
/// The Repair view ball's ring ([`crate::repair`]).
pub(crate) const REPAIR_CUBE_ARC_LAYER: usize = 4;

/// The ring round the ball.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct CubeArcGizmos;

/// The roll arrows on the ring: a little bolder than it, so it reads as a dial to turn.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct RollGizmos;

#[derive(Resource)]
pub struct ViewCubeImage(pub Handle<Image>);

#[derive(Component)]
struct CubeCamera;

/// The node over the ball that hovers and clicks it.
#[derive(Component)]
struct CubeImageNode;

/// A standard view, and the knob on the ball that turns to it.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CubeFace {
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
}

impl CubeFace {
    const ALL: [CubeFace; 6] = [
        CubeFace::Front,
        CubeFace::Back,
        CubeFace::Left,
        CubeFace::Right,
        CubeFace::Top,
        CubeFace::Bottom,
    ];

    /// Where its knob is on the ball: the direction the view looks from.
    fn normal(self) -> Vec3 {
        match self {
            CubeFace::Front => -Vec3::Y,
            CubeFace::Back => Vec3::Y,
            CubeFace::Left => -Vec3::X,
            CubeFace::Right => Vec3::X,
            CubeFace::Top => Vec3::Z,
            CubeFace::Bottom => -Vec3::Z,
        }
    }

    fn label(self) -> &'static str {
        match self {
            CubeFace::Front => "Front",
            CubeFace::Back => "Back",
            CubeFace::Left => "Left",
            CubeFace::Right => "Right",
            CubeFace::Top => "Top",
            CubeFace::Bottom => "Bottom",
        }
    }

    /// The catalog key of the caption beside the ball (its view's name).
    fn caption_key(self) -> &'static str {
        match self {
            CubeFace::Front => "view-ball-front",
            CubeFace::Back => "view-ball-back",
            CubeFace::Left => "view-ball-left",
            CubeFace::Right => "view-ball-right",
            CubeFace::Top => "view-ball-top",
            CubeFace::Bottom => "view-ball-bottom",
        }
    }

    fn view(self) -> StandardView {
        match self {
            CubeFace::Front => StandardView::Front,
            CubeFace::Back => StandardView::Back,
            CubeFace::Left => StandardView::Left,
            CubeFace::Right => StandardView::Right,
            CubeFace::Top => StandardView::Top,
            CubeFace::Bottom => StandardView::Bottom,
        }
    }
}

/// What the pointer is over on the ball.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BallSpot {
    /// A knob: its standard view.
    Knob(CubeFace),
    /// A corner's dot (P6.4): the signs of its x, y and z; its trimetric view.
    Corner(IVec3),
    /// The glass between them: a drag turns the view.
    Glass,
}

/// The spot on the ball under the pointer (the glass while it is dragged).
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct BallHover(pub Option<BallSpot>);

/// A drag on the ball: whether one is going on, and how far it went (a click that ends one
/// doesn't also pick what it ends on).
#[derive(Resource, Default)]
struct BallDrag {
    active: bool,
    moved: f32,
}

/// The eight corners (P6.4): the signs of their x, y and z.
fn corners() -> impl Iterator<Item = IVec3> {
    (0..8).map(|i| IVec3::new(i & 1, (i >> 1) & 1, (i >> 2) & 1) * 2 - IVec3::ONE)
}

/// The dimetric view's azimuth and elevation (degrees): seen from 45° round, the X and Y axes
/// are foreshortened alike (0.75) and Z differently (0.935): P6.3, with sin 20.705° = √(1/8).
pub const DIMETRIC: (f32, f32) = (45.0, 20.705);

/// The trimetric view from a corner (P6.4): the default view's angles (30° round from the
/// Front, 30° up; the three axes foreshortened differently) mirrored into that corner's octant.
pub fn corner_view(corner: IVec3) -> (f32, f32) {
    let (az0, el0) = StandardView::Default.angles();
    let az = if corner.y < 0 { az0 } else { 180.0 - az0 };
    let az = if corner.x < 0 { -az } else { az };
    (az, if corner.z < 0 { -el0 } else { el0 })
}

/// A part of the ball's scene, styled by [`style_ball`].
#[derive(Component, Debug, Clone, Copy, PartialEq)]
enum BallPart {
    Glass,
    Origin,
    /// The X, Y or Z axis, from the origin to its knob.
    Axis(usize),
    /// The ring in the plane across this axis.
    Ring(usize),
    Knob(CubeFace),
    Corner(IVec3),
}

impl BallPart {
    fn spot(self) -> Option<BallSpot> {
        match self {
            BallPart::Glass => Some(BallSpot::Glass),
            BallPart::Knob(f) => Some(BallSpot::Knob(f)),
            BallPart::Corner(c) => Some(BallSpot::Corner(c)),
            _ => None,
        }
    }

    /// Its shader's colours and mode (linear), in the scheme in force.
    fn params(self, theme: &Theme, hovered: bool) -> BallParams {
        let lin = |c: Color| c.to_linear().to_vec4();
        let axis = |i: usize| [theme.axis_x, theme.axis_y, theme.axis_z][i];
        // What the far side fades toward: the glass seen over the canvas.
        let far = lin(by_scheme(
            Color::srgb(0.86, 0.89, 0.93),
            Color::srgb(0.30, 0.33, 0.38),
        ));
        let solid = |c: Color, fade: f32| BallParams {
            color: lin(c),
            far,
            mode: Vec4::new(0.0, fade, 0.0, 0.0),
        };
        let lighter = |c: Color, k: f32| c.mix(&Color::WHITE, k);
        match self {
            BallPart::Glass => BallParams {
                color: lin(by_scheme(
                    Color::srgba(0.80, 0.86, 0.93, 0.18),
                    Color::srgba(0.55, 0.60, 0.68, 0.16),
                )),
                far: lin(by_scheme(
                    Color::srgb(0.55, 0.63, 0.73),
                    Color::srgb(0.68, 0.74, 0.83),
                )),
                mode: Vec4::new(1.0, 0.0, if hovered { 0.8 } else { 0.62 }, 0.9),
            },
            BallPart::Origin => solid(
                by_scheme(Color::srgb(0.35, 0.38, 0.43), Color::srgb(0.80, 0.83, 0.88)),
                0.0,
            ),
            BallPart::Axis(i) => solid(axis(i), 0.55),
            BallPart::Ring(_) => solid(
                by_scheme(Color::srgb(0.58, 0.64, 0.72), Color::srgb(0.56, 0.61, 0.68)),
                0.8,
            ),
            BallPart::Knob(f) => {
                let n = f.normal();
                let i = n.abs().max_position();
                let c = if n[i] > 0.0 {
                    axis(i)
                } else {
                    // The negative axes' knobs: paler.
                    axis(i).mix(&by_scheme(Color::WHITE, Color::srgb(0.35, 0.37, 0.41)), 0.5)
                };
                solid(if hovered { lighter(c, 0.35) } else { c }, 0.7)
            }
            BallPart::Corner(_) => solid(
                if hovered {
                    theme.primary
                } else {
                    by_scheme(Color::srgb(0.50, 0.56, 0.64), Color::srgb(0.70, 0.75, 0.82))
                },
                0.7,
            ),
        }
    }

    /// Hovered knobs and dots grow.
    fn scale(self, hovered: bool) -> f32 {
        match (self, hovered) {
            (BallPart::Knob(_), true) => 1.18,
            (BallPart::Corner(_), true) => 1.8,
            _ => 1.0,
        }
    }
}

/// The ball's parts' material (`view_ball.wesl`): unlit, the far side faded.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, Copy, PartialEq, Default)]
pub struct BallMaterial {
    #[uniform(0)]
    pub params: BallParams,
}

/// What `view_ball.wesl` draws with. Colours are linear RGBA.
#[derive(Debug, Clone, Copy, PartialEq, ShaderType, Default)]
pub struct BallParams {
    pub color: Vec4,
    /// What the far side fades toward; the glass's rim colour.
    pub far: Vec4,
    /// x: 1 for the glass; y: how far the far side fades; z: the glass's rim opacity; w: its
    /// highlight's.
    pub mode: Vec4,
}

impl Material for BallMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://cadrs_app/view_ball.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        if self.params.mode.x > 0.5 {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        }
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}

/// A ball's image in the UI (`view_ball_image.wesl`): blended as the premultiplied colours its
/// camera rendered.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct BallImageMaterial {
    #[texture(0)]
    #[sampler(1)]
    image: Handle<Image>,
}

impl UiMaterial for BallImageMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://cadrs_app/view_ball_image.wesl".into()
    }

    fn specialize(descriptor: &mut RenderPipelineDescriptor, _key: UiMaterialKey<Self>) {
        if let Some(target) = descriptor
            .fragment
            .as_mut()
            .and_then(|f| f.targets.first_mut())
            .and_then(|t| t.as_mut())
        {
            target.blend = Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        }
    }
}

/// A node showing a ball's image (the main view's, the Repair view's, the flat view's): given
/// its [`BallImageMaterial`] by [`attach_ball_images`].
#[derive(Component, Clone)]
pub(crate) struct BallImage(pub Handle<Image>);

fn attach_ball_images(
    q: Query<(Entity, &BallImage), Without<MaterialNode<BallImageMaterial>>>,
    mut materials: ResMut<Assets<BallImageMaterial>>,
    mut commands: Commands,
) {
    for (e, image) in &q {
        let m = materials.add(BallImageMaterial {
            image: image.0.clone(),
        });
        commands.entity(e).try_insert(MaterialNode(m));
    }
}

/// An X, Y or Z letter on its knob.
#[derive(Component)]
struct AxisLabel(Vec3);

/// A nudge arrow: rotates the view by (yaw, pitch) degrees.
#[derive(Component, Clone, Copy)]
struct CubeArrow(f32, f32);

/// A roll arrow on the ring: rolls the view by this many degrees (counter-clockwise on screen
/// when positive).
#[derive(Component, Clone, Copy)]
struct RollArrow(f32);

/// The roll arrows' arcs on the ring (degrees, counter-clockwise from screen right): from where
/// each starts to its arrowhead, and the roll it does.
const ROLL_ARCS: [(f32, f32, f32); 2] = [(115.0, 155.0, 15.0), (65.0, 25.0, -15.0)];

/// The name of what a click on the hovered spot does, beside the ball.
#[derive(Component)]
struct BallCaption;

fn setup_cube(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<BallMaterial>>,
    mut store: ResMut<GizmoConfigStore>,
) {
    let size = (CUBE_WIDGET * CUBE_SUPERSAMPLE).as_uvec2();
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    image.asset_usage = RenderAssetUsages::default();
    let handle = images.add(image);
    commands.insert_resource(ViewCubeImage(handle.clone()));

    let (config, _) = store.config_mut::<CubeArcGizmos>();
    config.render_layers = RenderLayers::layer(CUBE_ARC_LAYER);
    config.line.width = RING_WIDTH;
    let (config, _) = store.config_mut::<RollGizmos>();
    config.render_layers = RenderLayers::layer(CUBE_ARC_LAYER);
    config.line.width = 3.4;
    config.line.joints = bevy::gizmos::config::GizmoLineJoint::Round(4);

    commands.spawn((
        Name::new("view-cube-camera"),
        CubeCamera,
        Camera3d::default(),
        RenderTarget::Image(handle.into()),
        Camera {
            order: -1,
            is_active: false,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
        cube_projection(),
        Tonemapping::None,
        DebandDither::Disabled,
        RenderLayers::from_layers(&[CUBE_LAYER, CUBE_ARC_LAYER]),
        Transform::default(),
    ));

    let mut part = |name: String, part: BallPart, mesh: Handle<Mesh>, at: Transform| {
        // The glass is see-through from its first frame ([`style_ball`] colours them all).
        let glass = part == BallPart::Glass;
        let material = materials.add(BallMaterial {
            params: BallParams {
                mode: Vec4::new(if glass { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0),
                ..default()
            },
        });
        commands.spawn((
            Name::new(name),
            part,
            Mesh3d(mesh),
            MeshMaterial3d(material),
            at,
            RenderLayers::layer(CUBE_LAYER),
        ));
    };
    part(
        "view-ball-glass".into(),
        BallPart::Glass,
        meshes.add(Sphere::new(1.0).mesh().uv(64, 32)),
        Transform::default(),
    );
    part(
        "view-ball-origin".into(),
        BallPart::Origin,
        meshes.add(Sphere::new(ORIGIN_R).mesh().uv(16, 10)),
        Transform::default(),
    );
    // The axes, from the origin to their knobs.
    let len = 1.0 - KNOB_R;
    let axis = meshes.add(Cylinder::new(AXIS_R, len));
    for (i, (a, letter)) in [(Vec3::X, "x"), (Vec3::Y, "y"), (Vec3::Z, "z")]
        .into_iter()
        .enumerate()
    {
        part(
            format!("view-ball-axis-{letter}"),
            BallPart::Axis(i),
            axis.clone(),
            Transform::from_translation(a * len / 2.0)
                .with_rotation(Quat::from_rotation_arc(Vec3::Y, a)),
        );
    }
    // The rings in the YZ, XZ and XY planes (a torus lies across Y).
    let ring = meshes.add(
        Torus {
            minor_radius: RING_TUBE,
            major_radius: 1.0,
        }
        .mesh()
        .minor_resolution(8)
        .major_resolution(128),
    );
    for (i, rot) in [
        Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2),
        Quat::IDENTITY,
        Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
    ]
    .into_iter()
    .enumerate()
    {
        part(
            format!("view-ball-ring-{}", ["x", "y", "z"][i]),
            BallPart::Ring(i),
            ring.clone(),
            Transform::from_rotation(rot),
        );
    }
    let knob = meshes.add(Sphere::new(KNOB_R).mesh().uv(32, 18));
    for face in CubeFace::ALL {
        part(
            format!("view-ball-knob-{}", face.label().to_lowercase()),
            BallPart::Knob(face),
            knob.clone(),
            Transform::from_translation(face.normal()),
        );
    }
    let dot = meshes.add(Sphere::new(DOT_R).mesh().uv(16, 10));
    for c in corners() {
        part(
            format!("view-ball-corner-{}{}{}", sign(c.x), sign(c.y), sign(c.z)),
            BallPart::Corner(c),
            dot.clone(),
            Transform::from_translation(c.as_vec3().normalize()),
        );
    }
}

/// "p" or "m" for a corner's name ("view-ball-corner-pmp": x+, y−, z+).
fn sign(v: i32) -> &'static str {
    if v > 0 { "p" } else { "m" }
}

/// How many frames the view ball's camera renders after something it shows changed: one for
/// the change, one for its gizmos and layout to catch up, one to spare.
const CUBE_RENDER_FRAMES: u16 = 3;
/// It renders this long when the first document opens, so its shaders are ready before it
/// keeps its image.
const CUBE_FIRST_FRAMES: u16 = 180;

/// The view ball's image keeps its last render: its camera renders only while what it shows
/// changes (it would cost a camera pass of its own every frame).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn render_cube_when_changed(
    view: Res<ViewportView>,
    hover: Res<BallHover>,
    (theme, active): (Res<Theme>, Res<cadrs_ui::ActiveTheme>),
    parts: Query<(), (With<BallPart>, Changed<Transform>)>,
    rolls: Query<(), (With<RollArrow>, Changed<Hovered>)>,
    mut material_events: MessageReader<AssetEvent<BallMaterial>>,
    mut cameras: Query<&mut Camera, With<CubeCamera>>,
    mut left: Local<Option<u16>>,
) {
    let restyled = material_events
        .read()
        .any(|e| matches!(e, AssetEvent::Modified { .. }));
    let left = left.get_or_insert(CUBE_FIRST_FRAMES);
    if theme.is_changed()
        || active.is_changed()
        || view.is_changed()
        || hover.is_changed()
        || !parts.is_empty()
        || !rolls.is_empty()
        || restyled
    {
        *left = (*left).max(CUBE_RENDER_FRAMES);
    }
    for mut c in &mut cameras {
        if c.is_active != (*left > 0) {
            c.is_active = *left > 0;
        }
    }
    *left = left.saturating_sub(1);
}

fn activate_cube_camera<const ON: bool>(mut q: Query<&mut Camera, With<CubeCamera>>) {
    for mut c in &mut q {
        c.is_active = ON;
    }
}

/// The ball's colours in the scheme in force that aren't its parts' ([`BallPart::params`]).
pub(crate) trait CubeColors {
    /// The nudge arrows: UI icons, which the scheme maps itself.
    fn view_cube_arrow(&self) -> Color;
    /// The ring round the ball (gizmos, so not mapped).
    fn view_cube_arc(&self) -> Color;
}

impl CubeColors for Theme {
    fn view_cube_arrow(&self) -> Color {
        // Readable over the light canvas as over the dark one.
        Color::srgb_u8(0x9a, 0xab, 0xb3)
    }
    fn view_cube_arc(&self) -> Color {
        by_scheme(
            Color::srgb_u8(0xa8, 0xb4, 0xc0),
            Color::srgb_u8(0x6a, 0x72, 0x7c),
        )
    }
}

/// Colours and sizes the ball's parts for the scheme in force and what is hovered.
fn style_ball(
    hover: Res<BallHover>,
    active: Res<cadrs_ui::ActiveTheme>,
    mut q: Query<(Ref<BallPart>, &MeshMaterial3d<BallMaterial>, &mut Transform)>,
    mut materials: ResMut<Assets<BallMaterial>>,
) {
    let all = hover.is_changed() || active.is_changed();
    for (part, mat, mut at) in &mut q {
        if !all && !part.is_added() {
            continue;
        }
        let hovered = part.spot().is_some() && part.spot() == hover.0;
        let params = part.params(&active, hovered);
        if materials.get(&mat.0).is_some_and(|m| m.params != params)
            && let Some(mut m) = materials.get_mut(&mat.0)
        {
            m.params = params;
        }
        let scale = Vec3::splat(part.scale(hovered));
        if at.scale != scale {
            at.scale = scale;
        }
    }
}

/// Spawns the view ball widget (inside the viewport area, bottom right).
pub fn spawn_view_cube(p: &mut ChildSpawnerCommands, theme: &Theme, image: Handle<Image>) {
    p.spawn((
        Name::new("view-cube"),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(0.0),
            bottom: Val::Px(0.0),
            width: Val::Px(CUBE_WIDGET.x),
            height: Val::Px(CUBE_WIDGET.y),
            ..default()
        },
        Pickable::IGNORE,
    ))
    .with_children(|w| {
        w.spawn((
            Name::new("view-cube-render"),
            BallImage(image),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(CUBE_WIDGET.x),
                height: Val::Px(CUBE_WIDGET.y),
                ..default()
            },
            Pickable::IGNORE,
        ));
        // An invisible node over the ball: hovering and clicking it picks a knob or a dot,
        // dragging it turns the view. Outside it the pointer still reaches the viewport.
        let reach = 2.0 * (1.0 + KNOB_R) * CUBE_PX;
        w.spawn((
            Name::new("view-cube-image"),
            CubeImageNode,
            Hovered::default(),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(CUBE_CENTER.x - reach / 2.0),
                top: Val::Px(CUBE_CENTER.y - reach / 2.0),
                width: Val::Px(reach),
                height: Val::Px(reach),
                border_radius: BorderRadius::MAX,
                ..default()
            },
        ));
        for (axis, letter) in [(Vec3::X, "X"), (Vec3::Y, "Y"), (Vec3::Z, "Z")] {
            w.spawn((
                Name::new(format!("view-cube-axis-{}", letter.to_lowercase())),
                axis_letter(theme, letter),
                AxisLabel(axis),
            ));
        }
        let arrow = theme.view_cube_arrow();
        let ring = RING_R * CUBE_PX;
        for (name, icon_name, at, yaw, pitch) in [
            (
                "view-cube-up",
                "caret-up-filled",
                Vec2::new(0.0, -ring),
                0.0,
                -15.0,
            ),
            (
                "view-cube-down",
                "caret-down-filled",
                Vec2::new(0.0, ring),
                0.0,
                15.0,
            ),
            (
                "view-cube-left",
                "caret-left-filled",
                Vec2::new(-ring, 0.0),
                -15.0,
                0.0,
            ),
            (
                "view-cube-right",
                "caret-right-filled",
                Vec2::new(ring, 0.0),
                15.0,
                0.0,
            ),
        ] {
            let at = CUBE_CENTER + at;
            w.spawn((
                cadrs_ui::IconButton::new(name, icon_name)
                    .icon_size(14.0)
                    .build(theme),
                CubeArrow(yaw, pitch),
            ))
            .insert((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(at.x - 8.0),
                    top: Val::Px(at.y - 8.0),
                    width: Val::Px(16.0),
                    height: Val::Px(16.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                cadrs_ui::Visuals {
                    background: cadrs_ui::StateColors::all(Color::NONE),
                    border: cadrs_ui::StateColors::all(Color::NONE),
                    foreground: cadrs_ui::StateColors::new(
                        arrow,
                        theme.muted_foreground,
                        theme.foreground,
                        arrow,
                    ),
                    focus_ring: theme.focus_ring,
                },
            ));
        }
        // The roll arrows: invisible nodes over their arcs (drawn by [`draw_cube`]).
        for (from, to, roll) in ROLL_ARCS {
            let a = ((from + to) / 2.0).to_radians();
            let at = CUBE_CENTER + Vec2::new(a.cos(), -a.sin()) * ring;
            w.spawn((
                Name::new(if roll > 0.0 {
                    "view-cube-roll-left"
                } else {
                    "view-cube-roll-right"
                }),
                RollArrow(roll),
                Hovered::default(),
                cadrs_ui::Tooltip::new(tr(if roll > 0.0 {
                    "view-cube-roll-ccw"
                } else {
                    "view-cube-roll-cw"
                })),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(at.x - 13.0),
                    top: Val::Px(at.y - 13.0),
                    width: Val::Px(26.0),
                    height: Val::Px(26.0),
                    ..default()
                },
            ));
        }
        // What a click does, beside the ball while it is hovered.
        w.spawn((
            Name::new("view-cube-caption"),
            BallCaption,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Percent(100.0),
                top: Val::Px(CUBE_CENTER.y - 12.0),
                height: Val::Px(24.0),
                padding: UiRect::horizontal(Val::Px(8.0)),
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(theme.radius_panel)),
                display: Display::None,
                ..default()
            },
            cadrs_ui::glass::panel(theme),
            Pickable::IGNORE,
        ))
        .with_child((
            theme.text("", 12.0, FontWeight::MEDIUM, theme.foreground),
            Pickable::IGNORE,
        ));
    });
}

/// An axis letter on its knob (the main ball's and the flat view's).
pub(crate) fn axis_letter(theme: &Theme, letter: &str) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            ..default()
        },
        theme.text(letter, 10.0, FontWeight::BOLD, Color::WHITE),
        Pickable::IGNORE,
    )
}

/// The View options button at the tab bar's right end, after the view tools: a ▲ chevron like
/// a folder tab's (▼ while its menu is open), the menu opening upward.
pub(crate) fn spawn_view_options(bar: &mut ChildSpawnerCommands, t: &Theme) {
    bar.spawn((
        cadrs_ui::ToolButton::new("render-mode", "part")
            .icon_size(18.0)
            .tooltip(tr("view-options"))
            .build(t),
        ViewOptionsButton,
        cadrs_ui::prelude::observe(|a: On<Activate>, mut commands: Commands| {
            let anchor = a.entity;
            commands.queue(move |world: &mut World| {
                let state = ViewMenuState::of(world);
                let theme = world.resource::<Theme>().clone();
                let mut commands = world.commands();
                cadrs_ui::open_menu(&mut commands, anchor, view_menu(state).build(&theme));
                world.flush();
            });
        }),
        cadrs_ui::prelude::observe(on_view_menu),
    ))
    .insert(Node {
        width: Val::Auto,
        padding: UiRect::horizontal(Val::Px(5.0)),
        column_gap: Val::Px(2.0),
        ..crate::document::view_tool_node(t)
    })
    .with_children(|b| {
        b.spawn((
            cadrs_ui::icon("chevron-up", 18.0, t.foreground),
            ViewOptionsChevron,
            Pickable::IGNORE,
        ));
    });
}

#[derive(Component)]
struct ViewOptionsButton;

#[derive(Component)]
struct ViewOptionsChevron;

/// The View options chevron: ▲, ▼ while its menu is open; and the button pressed then (as a
/// folder tab's, `crate::tab_folders`).
#[allow(clippy::type_complexity)]
fn sync_view_options_chevron(
    q: Query<
        (
            Entity,
            &Children,
            Has<cadrs_ui::menu::MenuOpenAnchor>,
            Has<cadrs_ui::ForceState>,
        ),
        With<ViewOptionsButton>,
    >,
    mut q_icon: Query<&mut cadrs_ui::Icon, With<ViewOptionsChevron>>,
    mut commands: Commands,
) {
    for (button, children, open, pressed) in &q {
        if open && !pressed {
            commands
                .entity(button)
                .try_insert(cadrs_ui::ForceState(cadrs_ui::VisualState::Pressed));
        } else if !open && pressed {
            commands.entity(button).try_remove::<cadrs_ui::ForceState>();
        }
        let want = if open { "chevron-down" } else { "chevron-up" };
        for c in children.iter() {
            if let Ok(mut icon) = q_icon.get_mut(c)
                && icon.name != want
            {
                icon.name = want.into();
            }
        }
    }
}

/// What the view menu shows checked or enabled.
#[derive(Debug, Clone, Copy)]
pub struct ViewMenuState {
    pub render: crate::camera::RenderMode,
    /// Each render group's last mode (its row's label).
    pub groups: (crate::camera::RenderMode, crate::camera::RenderMode),
    pub perspective: bool,
    pub previous: bool,
    pub section: bool,
    /// A Part Studio or an Assembly (not a PCB Studio): named views and section views.
    pub modeling: bool,
    /// The camera is kept up (issue #97, the preference).
    pub keep_up: bool,
    /// Walk mode is on (issue #97, [`crate::walk`]).
    pub walking: bool,
    /// A sketch is being edited (no walk mode: its tools have the keys).
    pub sketching: bool,
}

impl ViewMenuState {
    pub fn of(world: &World) -> Self {
        let view = world.resource::<ViewportView>().view;
        let kind = *world.resource::<crate::viewport::ActiveKind>();
        let element = world.resource::<ViewportView>().element;
        Self {
            render: view.render,
            groups: world
                .resource::<crate::view_options::RenderGroups>()
                .labels(element, view.render),
            perspective: view.perspective,
            previous: crate::view_options::has_previous(world),
            section: crate::section_view::active(world),
            modeling: matches!(
                kind,
                crate::viewport::ActiveKind::PartStudio | crate::viewport::ActiveKind::Assembly
            ),
            // (Walking keeps the camera up too.)
            keep_up: world
                .get_resource::<crate::preferences_ui::LocalPreferences>()
                .is_some_and(|p| p.0.keep_camera_up)
                || world
                    .get_resource::<crate::walk::Walking>()
                    .is_some_and(|w| w.on),
            walking: world
                .get_resource::<crate::walk::Walking>()
                .is_some_and(|w| w.on),
            sketching: world.contains_resource::<crate::sketch::SketchSession>(),
        }
    }
}

/// The View options menu, grouped like Onshape's (`reference/onshape/view/view-cube-menu4-01.png`).
/// P3E.3a: the render modes in their two groups (the shaded modes, then the hidden-line ones;
/// the group in use is checked, and each is named after the mode last picked in it),
/// Perspective view, Named views…, Previous view, Zoom to window and Section view… work. What
/// cadrs does not have yet is disabled.
fn view_menu(st: ViewMenuState) -> cadrs_ui::Menu {
    use cadrs_ui::MenuItem as I;
    let line = st.render.line_drawing();
    // Each group's row reads the mode last picked in it.
    let (shaded_label, line_label) = (st.groups.0.label(), st.groups.1.label());
    let check = |on: bool, item: I| if on { item.icon("check") } else { item };
    // At the tab bar's right end: up from there, its right edges in line.
    cadrs_ui::Menu::new("view-menu")
        .side(PopoverSide::Top)
        .align_end()
        .min_width(222.0)
        .item_height(23.0)
        .item(I::new("view-isometric", tr("view-menu-isometric")).shortcut("Shift+7"))
        .item(I::new("view-dimetric", tr("view-menu-dimetric")))
        .item(I::new("view-trimetric", tr("view-menu-trimetric")))
        .separator()
        .item(
            I::new("view-graphics", tr("view-menu-graphics-preferences"))
                .icon("settings")
                .disabled(true),
        )
        .separator()
        .item(I::new("view-named", tr("view-menu-named-views")).disabled(!st.modeling))
        .item(I::new("view-previous", tr("view-menu-previous-view")).disabled(!st.previous))
        .separator()
        .item(I::new("view-zoom-to-fit", tr("common-zoom-to-fit")).shortcut("F"))
        .item(I::new("view-zoom-window", tr("view-shortcut-zoom-window")))
        .separator()
        .item(check(
            st.perspective,
            I::new("view-perspective", tr("view-menu-perspective")),
        ))
        .item(check(
            st.keep_up,
            I::new("view-keep-up", tr("view-menu-keep-up")),
        ))
        .item(
            check(st.walking, I::new("view-walk", tr("view-menu-walk-mode")))
                .disabled(!st.modeling || st.sketching),
        )
        .item(I::new("view-orient-normal", tr("view-menu-orient-normal")).disabled(true))
        .separator()
        .item(
            check(!line, I::new("view-shaded", shaded_label))
                .submenu(crate::view_options::render_items(st.render, false)),
        )
        .item(
            check(line, I::new("view-hidden-edges", line_label))
                .submenu(crate::view_options::render_items(st.render, true)),
        )
        .item(
            I::new("view-tangent-edges", tr("view-menu-tangent-edges"))
                .icon("check")
                .submenu(vec![]),
        )
        .separator()
        .item(I::new("view-high-quality", tr("view-menu-high-quality")).disabled(true))
        .item(I::new("view-boundary", tr("view-menu-boundary-edges")).disabled(true))
        .separator()
        .item(
            I::new(
                "view-section-item",
                tr(if st.section {
                    "view-menu-exit-section"
                } else {
                    "view-menu-section-view"
                }),
            )
            .icon("section-view")
            .disabled(!st.modeling),
        )
}

fn on_view_menu(ev: On<cadrs_ui::MenuAction>, view: Res<ViewportView>, mut commands: Commands) {
    if let Some(slug) = ev.item.strip_prefix("view-render-") {
        if let Some(mode) = crate::camera::RenderMode::ALL
            .into_iter()
            .find(|m| m.slug() == slug)
        {
            commands
                .queue(move |world: &mut World| crate::view_options::set_render_mode(world, mode));
        }
        return;
    }
    match ev.item.as_str() {
        // The isometric, dimetric (two axes foreshortened equally) and trimetric (Onshape's
        // default) views turn and zoom to fit, so a big model is framed whole.
        "view-isometric" => {
            commands.queue(crate::viewport::isometric_and_fit);
            return;
        }
        "view-dimetric" => {
            commands.queue(|world: &mut World| {
                crate::viewport::orient_and_fit(world, |v| ViewState {
                    azimuth: DIMETRIC.0,
                    elevation: DIMETRIC.1,
                    roll: 0.0,
                    ..v
                })
            });
            return;
        }
        "view-trimetric" => {
            commands.queue(|world: &mut World| {
                crate::viewport::orient_and_fit(world, |v| {
                    v.oriented(crate::camera::StandardView::Default)
                })
            });
            return;
        }
        "view-zoom-to-fit" => {
            commands.queue(crate::viewport::zoom_to_fit);
            return;
        }
        "view-zoom-window" => {
            commands.queue(crate::view_options::start_zoom_window);
            return;
        }
        "view-previous" => {
            commands.queue(crate::view_options::previous_view);
            return;
        }
        "view-named" => {
            commands.queue(crate::view_options::open_named_views);
            return;
        }
        "view-perspective" => {
            let on = !view.view.perspective;
            commands
                .queue(move |world: &mut World| crate::view_options::set_perspective(world, on));
            return;
        }
        "view-keep-up" => {
            commands.queue(|world: &mut World| {
                let on = !world
                    .resource::<crate::preferences_ui::LocalPreferences>()
                    .0
                    .keep_camera_up;
                crate::preferences_ui::set_keep_camera_up(world, on);
            });
            return;
        }
        "view-walk" => {
            commands.queue(|world: &mut World| {
                let on = !world.resource::<crate::walk::Walking>().on;
                crate::walk::set_walking(world, on);
            });
            return;
        }
        "view-section-item" => {
            commands.queue(crate::section_view::toggle);
        }
        _ => {}
    }
}

/// The view ball's camera's projection (P3D.4: the Repair panel's ball uses it too).
pub(crate) fn cube_projection() -> Projection {
    Projection::Orthographic(OrthographicProjection {
        scaling_mode: ScalingMode::WindowSize,
        scale: 1.0 / (CUBE_PX * CUBE_SUPERSAMPLE),
        near: 0.0,
        far: 100.0,
        viewport_origin: Vec2::new(
            CUBE_CENTER.x / CUBE_WIDGET.x,
            1.0 - CUBE_CENTER.y / CUBE_WIDGET.y,
        ),
        ..OrthographicProjection::default_3d()
    })
}

/// The view a click at `widget_pos` (px from the widget's top-left) on a ball seen from `view`
/// turns to: a knob's standard view or a corner's trimetric one (P3D.4: the Repair panel's
/// ball).
pub(crate) fn view_at_spot(view: &ViewState, widget_pos: Vec2) -> Option<ViewState> {
    match spot_at(view, widget_pos)? {
        BallSpot::Knob(face) => Some(view.oriented(face.view())),
        BallSpot::Corner(c) => {
            let (azimuth, elevation) = corner_view(c);
            Some(ViewState {
                azimuth,
                elevation,
                roll: 0.0,
                ..*view
            })
        }
        BallSpot::Glass => None,
    }
}

/// `view` turned like the ball: about its centre, a ball unit [`CUBE_PX`] pixels.
fn cube_view(view: &ViewState) -> ViewState {
    ViewState {
        focus: Vec3::ZERO,
        scale: 1.0 / CUBE_PX,
        perspective: false,
        ..*view
    }
}

fn sync_cube_camera(view: Res<ViewportView>, mut q: Query<&mut Transform, With<CubeCamera>>) {
    let v = view.view;
    for mut t in &mut q {
        let new_t = Transform::from_translation(v.back() * 50.0).with_rotation(v.rotation());
        if *t != new_t {
            *t = new_t;
        }
    }
}

/// The knob, dot or glass under a point of the widget (px from its top-left): of the knobs
/// and dots within reach, the nearest the viewer.
fn spot_at(view: &ViewState, widget_pos: Vec2) -> Option<BallSpot> {
    let v = cube_view(view);
    let at = widget_pos - CUBE_CENTER;
    let back = v.back();
    let knobs = CubeFace::ALL.map(|f| (BallSpot::Knob(f), f.normal(), KNOB_R));
    let dots = corners().map(|c| (BallSpot::Corner(c), c.as_vec3().normalize(), DOT_R * 1.8));
    knobs
        .into_iter()
        .chain(dots)
        .filter(|(_, p, r)| v.project(*p).distance(at) <= (r + HIT_SLOP) * CUBE_PX)
        .max_by(|a, b| a.1.dot(back).total_cmp(&b.1.dot(back)))
        .map(|(spot, ..)| spot)
        .or_else(|| (at.length() <= CUBE_PX).then_some(BallSpot::Glass))
}

fn widget_origin(
    q: &Query<(&ComputedNode, &bevy::ui::UiGlobalTransform), With<CubeImageNode>>,
) -> Option<Vec2> {
    let (node, t) = q.iter().next()?;
    let s = node.inverse_scale_factor();
    let center = t.translation * s;
    // The hover node's center is the ball's.
    Some(center - CUBE_CENTER)
}

fn cube_hover(
    view: Res<ViewportView>,
    drag: Res<crate::viewport::ViewportDrag>,
    ball_drag: Res<BallDrag>,
    q_node: Query<(&ComputedNode, &bevy::ui::UiGlobalTransform), With<CubeImageNode>>,
    q_hovered: Query<&Hovered, With<CubeImageNode>>,
    mut hover: ResMut<BallHover>,
) {
    let spot = if ball_drag.active {
        Some(BallSpot::Glass)
    } else if q_hovered.iter().any(|h| h.get()) {
        widget_origin(&q_node).and_then(|o| spot_at(&view.view, drag.pointer() - o))
    } else {
        None
    };
    if hover.0 != spot {
        hover.0 = spot;
    }
}

/// What a click on `spot` does, for the caption beside the ball.
fn caption(spot: BallSpot) -> String {
    match spot {
        BallSpot::Knob(f) => tr(f.caption_key()),
        // "Top front right" and the like: one text each, as the words' order differs.
        BallSpot::Corner(c) => tr(match (c.z > 0, c.y < 0, c.x > 0) {
            (true, true, true) => "view-ball-top-front-right",
            (true, true, false) => "view-ball-top-front-left",
            (true, false, true) => "view-ball-top-back-right",
            (true, false, false) => "view-ball-top-back-left",
            (false, true, true) => "view-ball-bottom-front-right",
            (false, true, false) => "view-ball-bottom-front-left",
            (false, false, true) => "view-ball-bottom-back-right",
            (false, false, false) => "view-ball-bottom-back-left",
        }),
        BallSpot::Glass => tr("view-ball-drag-to-turn"),
    }
}

fn show_caption(
    hover: Res<BallHover>,
    mut q: Query<(&mut Node, &Children), With<BallCaption>>,
    mut q_text: Query<&mut Text>,
) {
    if !hover.is_changed() {
        return;
    }
    for (mut node, children) in &mut q {
        let display = if hover.0.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let Some(spot) = hover.0 else {
            continue;
        };
        for c in children.iter() {
            if let Ok(mut text) = q_text.get_mut(c) {
                let want = caption(spot);
                if text.0 != want {
                    text.0 = want;
                }
            }
        }
    }
}

fn draw_cube(
    mut ring: Gizmos<CubeArcGizmos>,
    mut rolls: Gizmos<RollGizmos>,
    view: Res<ViewportView>,
    theme: Res<Theme>,
    active: Res<cadrs_ui::ActiveTheme>,
    q_roll: Query<(&RollArrow, &Hovered)>,
) {
    let color = theme.view_cube_arc();
    // Gaps for the nudge arrows and the roll arrows' arcs.
    let gap = (9.5 / (RING_R * CUBE_PX)).to_degrees();
    let mut gaps: Vec<(f32, f32)> = [0.0_f32, 90.0, 180.0, 270.0]
        .into_iter()
        .map(|a| (a - gap, a + gap))
        .collect();
    gaps.extend(ROLL_ARCS.map(|(from, to, _)| (from.min(to), from.max(to))));
    draw_ball_ring(&mut ring, &view.view, color, &gaps);
    for (from, to, roll) in ROLL_ARCS {
        let hovered = q_roll.iter().any(|(r, h)| r.0 == roll && h.get());
        let c = if hovered {
            active.primary
        } else {
            by_scheme(
                Color::srgb_u8(0x8f, 0x9c, 0xaa),
                Color::srgb_u8(0x8a, 0x92, 0x9c),
            )
        };
        draw_roll_arrow(&mut rolls, &view.view, from, to, c);
    }
}

/// A point of the ring round the ball, `deg` counter-clockwise from screen right, in `view`'s
/// screen plane in front of the ball.
fn ring_point(view: &ViewState, deg: f32) -> Vec3 {
    let (r, u, b) = (view.right(), view.up(), view.back());
    let a = deg.to_radians();
    (r * a.cos() + u * a.sin()) * RING_R + b * 5.0
}

/// The ring round the ball, drawn in `view`'s screen plane (the main ball's, with `gaps` in
/// degrees for its arrows; the Repair view's and the flat view's, whole).
pub(crate) fn draw_ball_ring<G: GizmoConfigGroup>(
    ring: &mut Gizmos<G>,
    view: &ViewState,
    color: Color,
    gaps: &[(f32, f32)],
) {
    const N: usize = 120;
    let step = 360.0 / N as f32;
    for i in 0..N {
        let (a0, a1) = (i as f32 * step, (i + 1) as f32 * step);
        let mid = (a0 + a1) / 2.0;
        if gaps.iter().any(|(lo, hi)| {
            let m = if *lo < 0.0 && mid > 180.0 {
                mid - 360.0
            } else {
                mid
            };
            m > *lo && m < *hi
        }) {
            continue;
        }
        ring.line(ring_point(view, a0), ring_point(view, a1), color);
    }
}

/// A roll arrow: the ring's arc from `from` to `to` (degrees), its head at `to`.
fn draw_roll_arrow<G: GizmoConfigGroup>(
    g: &mut Gizmos<G>,
    view: &ViewState,
    from: f32,
    to: f32,
    color: Color,
) {
    const N: usize = 10;
    for i in 0..N {
        let a0 = from + (to - from) * i as f32 / N as f32;
        let a1 = from + (to - from) * (i + 1) as f32 / N as f32;
        g.line(ring_point(view, a0), ring_point(view, a1), color);
    }
    let b = view.back() * 5.0;
    let end = ring_point(view, to);
    let tangent = (end - ring_point(view, to - (to - from) * 0.1)).normalize();
    let radial = (end - b).normalize();
    let base = end - tangent * 0.2;
    g.line(end + tangent * 0.03, base + radial * 0.12, color);
    g.line(end + tangent * 0.03, base - radial * 0.12, color);
}

fn place_cube_labels(
    view: Res<ViewportView>,
    mut q_axis: Query<(&AxisLabel, &ComputedNode, &mut Node, &mut TextColor)>,
) {
    for (axis, node_c, mut node, mut color) in &mut q_axis {
        let size = node_c.size() * node_c.inverse_scale_factor();
        let (p, a) = axis_label_spot(&view.view, axis.0, size);
        if (color.0.alpha() - a).abs() > 1e-3 {
            color.0.set_alpha(a);
        }
        let (l, t) = (Val::Px(p.x), Val::Px(p.y));
        if node.left != l || node.top != t {
            node.left = l;
            node.top = t;
        }
    }
}

/// Where an axis letter goes for `view` (its label's top-left in the ball widget, for a label
/// of `size`: centred on its knob) and its opacity: shown while the knob is on the near side
/// (the main ball's and the flat view's).
pub(crate) fn axis_label_spot(view: &ViewState, axis: Vec3, size: Vec2) -> (Vec2, f32) {
    let v = cube_view(view);
    let along = axis.dot(v.back());
    let a = ((along + 0.1) / 0.2).clamp(0.0, 1.0);
    let p = CUBE_CENTER + v.project(axis);
    (p - size / 2.0, a)
}

fn on_cube_click(
    click: On<PointerClick>,
    q: Query<(), With<CubeImageNode>>,
    q_node: Query<(&ComputedNode, &bevy::ui::UiGlobalTransform), With<CubeImageNode>>,
    mut drag: ResMut<BallDrag>,
    mut view: ResMut<ViewportView>,
) {
    if click.button != PointerButton::Primary || !q.contains(click.entity) {
        return;
    }
    // The end of a drag.
    if std::mem::take(&mut drag.moved) > 3.0 {
        return;
    }
    let Some(origin) = widget_origin(&q_node) else {
        return;
    };
    match spot_at(&view.view, click.pointer.position - origin) {
        Some(BallSpot::Knob(face)) => {
            let to = view.target().oriented(face.view());
            view.animate_to(to);
        }
        Some(BallSpot::Corner(c)) => {
            let (azimuth, elevation) = corner_view(c);
            let to = ViewState {
                azimuth,
                elevation,
                roll: 0.0,
                ..view.target()
            };
            view.animate_to(to);
        }
        Some(BallSpot::Glass) | None => {}
    }
}

fn on_ball_drag_start(
    ev: On<PointerDragStart>,
    q: Query<(), With<CubeImageNode>>,
    mut drag: ResMut<BallDrag>,
) {
    if ev.button == PointerButton::Primary && q.contains(ev.entity) {
        *drag = BallDrag {
            active: true,
            moved: 0.0,
        };
    }
}

/// Dragging the ball turns the view as a right-drag in the view does: the ball rolls under
/// the pointer.
fn on_ball_drag(
    ev: On<PointerDrag>,
    q: Query<(), With<CubeImageNode>>,
    mut drag: ResMut<BallDrag>,
    mut view: ResMut<ViewportView>,
) {
    if ev.button != PointerButton::Primary || !q.contains(ev.entity) {
        return;
    }
    drag.moved += ev.delta.length();
    view.animation = None;
    view.view.orbit(ev.delta);
}

fn on_ball_drag_end(
    ev: On<PointerDragEnd>,
    q: Query<(), With<CubeImageNode>>,
    mut drag: ResMut<BallDrag>,
) {
    if q.contains(ev.entity) {
        drag.active = false;
    }
}

/// A nudge arrow rotates 15°; Shift+click 90°, Ctrl+click 5° (`shortcuts.md`).
fn on_arrow(
    ev: On<Activate>,
    q: Query<&CubeArrow>,
    keys: Res<ButtonInput<KeyCode>>,
    mut view: ResMut<ViewportView>,
) {
    if let Ok(a) = q.get(ev.entity) {
        let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        let k = crate::viewport::arrow_step(shift, ctrl) / 15.0;
        view.turn_by(a.0 * k, a.1 * k);
    }
}

/// A roll arrow turns the view about its direction by 15° (Shift 90°, Ctrl 5°, as the nudge
/// arrows).
fn on_roll(
    click: On<PointerClick>,
    q: Query<&RollArrow>,
    keys: Res<ButtonInput<KeyCode>>,
    mut view: ResMut<ViewportView>,
) {
    let Ok(r) = q.get(click.entity) else {
        return;
    };
    if click.button != PointerButton::Primary {
        return;
    }
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let k = crate::viewport::arrow_step(shift, ctrl) / 15.0;
    let mut to = view.target();
    to.rotate_model_about(to.back(), (r.0 * k).to_radians());
    view.animate_to(to);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where a point of the ball is in the widget, seen from `view`.
    fn at(view: &ViewState, p: Vec3) -> Vec2 {
        CUBE_CENTER + cube_view(view).project(p)
    }

    #[test]
    fn clicking_a_knob_picks_its_view() {
        let front = ViewState::standard(StandardView::Front);
        assert_eq!(
            spot_at(&front, CUBE_CENTER),
            Some(BallSpot::Knob(CubeFace::Front))
        );
        let top = ViewState::standard(StandardView::Top);
        assert_eq!(
            spot_at(&top, CUBE_CENTER),
            Some(BallSpot::Knob(CubeFace::Top))
        );
        // In the default view the Top, Right and Front knobs face the viewer.
        let d = ViewState::default();
        for f in [CubeFace::Top, CubeFace::Right, CubeFace::Front] {
            assert_eq!(spot_at(&d, at(&d, f.normal())), Some(BallSpot::Knob(f)));
        }
        // A knob in front hides the one behind it: in the Front view the Back knob is behind
        // the Front one.
        assert_eq!(
            view_at_spot(&front, CUBE_CENTER).map(|v| (v.azimuth, v.elevation)),
            Some(StandardView::Front.angles())
        );
        // Between knobs is the glass; beyond the ball, nothing.
        assert_eq!(
            spot_at(&d, CUBE_CENTER + Vec2::new(-0.3, 0.6) * CUBE_PX),
            Some(BallSpot::Glass)
        );
        assert_eq!(spot_at(&d, CUBE_CENTER + Vec2::new(66.0, 0.0)), None);
        assert_eq!(
            view_at_spot(&d, CUBE_CENTER + Vec2::new(-0.3, 0.6) * CUBE_PX),
            None
        );
    }

    #[test]
    fn axis_letters_sit_on_their_knobs() {
        let d = ViewState::default();
        let size = Vec2::new(8.0, 12.0);
        let (p, a) = axis_label_spot(&d, Vec3::Z, size);
        assert!((p + size / 2.0).distance(at(&d, Vec3::Z)) < 1e-3);
        assert_eq!(a, 1.0);
        // The Y knob is on the far side of the default view: no letter.
        assert_eq!(axis_label_spot(&d, Vec3::Y, size).1, 0.0);
    }

    #[test]
    fn axonometric_axis_scales() {
        // P3F.4 (P6.3): the lengths the unit X, Y and Z axes project to.
        let scales = |v: ViewState| {
            [Vec3::X, Vec3::Y, Vec3::Z].map(|a| v.project_vector(a).length() * v.scale)
        };
        // Isometric (view direction (1, 1, 1)/√3): all three √(2/3) = 0.81650.
        let iso = scales(ViewState::standard(StandardView::Isometric));
        for k in iso {
            assert!((k - (2.0f32 / 3.0).sqrt()).abs() < 1e-5, "{iso:?}");
        }
        // Dimetric: exactly two equal (X and Y), Z different.
        let di = scales(ViewState {
            azimuth: DIMETRIC.0,
            elevation: DIMETRIC.1,
            ..ViewState::default()
        });
        assert!((di[0] - di[1]).abs() < 1e-5, "{di:?}");
        assert!((di[0] - di[2]).abs() > 0.05, "{di:?}");
        // Trimetric (Onshape's default view, 30° round and 30° up): three different lengths,
        // (1 − cos²30·sin²30)^½ = 0.9014, (1 − cos⁴30)^½ = 0.6614 and cos 30 = 0.8660.
        let tri = scales(ViewState::standard(StandardView::Default));
        for (k, want) in tri.iter().zip([0.901_388, 0.661_438, 0.866_025]) {
            assert!((k - want).abs() < 1e-5, "{tri:?}");
        }
        assert!(
            (tri[0] - tri[1]).abs() > 0.01
                && (tri[1] - tri[2]).abs() > 0.01
                && (tri[0] - tri[2]).abs() > 0.01,
            "{tri:?}"
        );
        // Orthographic projections keep the sum of squares at 2 (the axes are orthonormal).
        for k in [iso, di, tri] {
            assert!(
                (k.iter().map(|x| x * x).sum::<f32>() - 2.0).abs() < 1e-4,
                "{k:?}"
            );
        }
    }

    #[test]
    fn corners_turn_to_trimetric_views() {
        // The default view looks from the front-right-top corner.
        assert_eq!(
            corner_view(IVec3::new(1, -1, 1)),
            StandardView::Default.angles()
        );
        // Every corner's view looks from that corner (its view direction is in its octant) and
        // no two axes are foreshortened alike (trimetric).
        for c in corners() {
            let (azimuth, elevation) = corner_view(c);
            let v = ViewState {
                azimuth,
                elevation,
                ..ViewState::default()
            };
            let b = v.back();
            assert!(
                b.x * c.x as f32 > 0.0 && b.y * c.y as f32 > 0.0 && b.z * c.z as f32 > 0.0,
                "{c}: {b}"
            );
            let k = [Vec3::X, Vec3::Y, Vec3::Z].map(|a| v.project_vector(a).length());
            assert!(
                (k[0] - k[1]).abs() > 0.05
                    && (k[1] - k[2]).abs() > 0.05
                    && (k[0] - k[2]).abs() > 0.05,
                "{k:?}"
            );
        }
        assert_eq!(corners().count(), 8);
        // In the Front view the front-right-top corner's dot is up and to the right, the
        // front-left-bottom one down and to the left.
        let front = ViewState::standard(StandardView::Front);
        let k = 1.0 / 3f32.sqrt() * CUBE_PX;
        assert_eq!(
            spot_at(&front, CUBE_CENTER + Vec2::new(k, -k)),
            Some(BallSpot::Corner(IVec3::new(1, -1, 1)))
        );
        assert_eq!(
            spot_at(&front, CUBE_CENTER + Vec2::new(-k, k)),
            Some(BallSpot::Corner(IVec3::new(-1, -1, -1)))
        );
    }
}
