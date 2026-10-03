//! The view cube in the viewport's top right corner: a small cube that turns with the view,
//! with Top/Front/Right/… labels on its faces, an X/Y/Z axis triad (red, green, blue) and
//! nudge arrows around it. Clicking a face animates the view to that standard view; clicking a
//! **corner** (P3.9, P6.4: the square patch where three faces meet, highlighted on hover) turns
//! it to the trimetric view from that corner; the arrows rotate the view by 15°.
//!
//! The cube is a tiny 3D scene on render layer 1, drawn by its own camera into an image that a
//! UI node shows, so it composes with the UI like any other widget.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, ScalingMode};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::text::FontWeight;
use bevy::ui_widgets::Activate;
use cadrs_ui::Theme;

use crate::camera::{StandardView, ViewState, ray_square};
use crate::viewport::ViewportView;
use crate::AppState;

pub struct ViewCubePlugin;

impl Plugin for ViewCubePlugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<CubeGizmos>()
            .init_gizmo_group::<CubeArcGizmos>()
            .init_resource::<CubeHover>()
            .init_resource::<CornerHover>()
            .add_systems(Startup, setup_cube)
            .add_systems(
                Update,
                (sync_cube_camera, cube_hover, draw_cube, fade_face_labels)
                    .chain()
                    .after(crate::viewport::apply_view_to_camera)
                    .run_if(in_state(AppState::Document)),
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
            .add_observer(on_arrow);
    }
}

/// Size of the view cube widget (logical px), as in the reference capture.
pub const CUBE_WIDGET: Vec2 = Vec2::new(172.0, 164.0);

/// The screen rect the view cube widget covers in a viewport (top right). Sketch glyphs and
/// labels are not drawn over it.
pub fn cube_rect(viewport: Rect) -> Rect {
    Rect::new(
        viewport.max.x - CUBE_WIDGET.x,
        viewport.min.y,
        viewport.max.x,
        viewport.min.y + CUBE_WIDGET.y,
    )
}
/// Where the cube's center sits in the widget.
const CUBE_CENTER: Vec2 = Vec2::new(86.0, 82.0);
/// Pixels per cube unit (the cube is 2 units wide).
const CUBE_PX: f32 = 26.0;
/// The cube image is rendered at twice the widget size for crisp edges.
pub(crate) const CUBE_SUPERSAMPLE: f32 = 2.0;
pub(crate) const CUBE_LAYER: usize = 1;
/// The main cube's rotate arrows: drawn in the main view's screen plane, so only the main cube's
/// camera sees them (the Repair cube draws its own on [`REPAIR_CUBE_ARC_LAYER`]).
pub(crate) const CUBE_ARC_LAYER: usize = 3;
/// The Repair view cube's rotate arrows ([`crate::repair`]).
pub(crate) const REPAIR_CUBE_ARC_LAYER: usize = 4;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct CubeGizmos;

/// The thick curved rotate arrows around the cube.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct CubeArcGizmos;

/// Where the axis triad starts: the cube's back-bottom-left corner.
/// Where the axis triad starts: just off the cube's front-bottom-left corner, so Z runs up the
/// cube's left edge and X along its bottom, as in the reference.
const TRIAD_ORIGIN: Vec3 = Vec3::new(-1.06, -1.06, -1.06);

#[derive(Resource)]
pub struct ViewCubeImage(pub Handle<Image>);

#[derive(Component)]
struct CubeCamera;

/// The UI image showing the cube.
#[derive(Component)]
struct CubeImageNode;

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

    /// The label's reading direction and up direction on the face.
    fn frame(self) -> (Vec3, Vec3) {
        match self {
            CubeFace::Front => (Vec3::X, Vec3::Z),
            CubeFace::Back => (-Vec3::X, Vec3::Z),
            CubeFace::Left => (-Vec3::Y, Vec3::Z),
            CubeFace::Right => (Vec3::Y, Vec3::Z),
            CubeFace::Top => (Vec3::X, Vec3::Y),
            CubeFace::Bottom => (Vec3::X, -Vec3::Y),
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

/// The cube face under the pointer.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct CubeHover(pub Option<CubeFace>);

/// The cube corner under the pointer (P6.4): the signs of its x, y and z.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct CornerHover(pub Option<IVec3>);

/// A corner's highlight: three small squares, one on each face that meets there.
#[derive(Component, Debug, Clone, Copy)]
struct CornerPatch(IVec3);

/// How far a corner's patch reaches along each edge from the corner (the cube is 2 wide).
const CORNER_SIZE: f32 = 0.5;

/// What the pointer is over on the cube.
#[derive(Debug, Clone, Copy, PartialEq)]
enum CubeSpot {
    Face(CubeFace),
    Corner(IVec3),
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

#[derive(Resource)]
struct CubeMaterials {
    normal: Handle<StandardMaterial>,
    hover: Handle<StandardMaterial>,
}

/// A face's label quad.
#[derive(Component, Clone, Copy)]
struct FaceLabel(CubeFace);

/// Label opacity: fully shown when the face is within about 67° of the view direction, gone
/// beyond 75°, where the text would be squashed into noise.
pub fn face_label_alpha(facing: f32) -> f32 {
    ((facing - 0.26) / 0.12).clamp(0.0, 1.0)
}

fn fade_face_labels(
    view: Res<ViewportView>,
    repair: Option<Res<crate::repair::Repair>>,
    (sm, panel): (Option<Res<crate::sheetmetal_table::SmTable>>, Option<Res<crate::appearance::SidePanel>>),
    q: Query<(&FaceLabel, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let back = view.view.back();
    // P3D.4: the Repair panel's cube shares the faces; a label it faces shows too (a face
    // turned away from the main view is hidden there by the cube itself). So does the Sheet
    // metal table's flat view cube (its Top stays labelled when the main view looks from below).
    let mut others: Vec<Vec3> = repair.filter(|r| r.open).map(|r| r.view.back()).into_iter().collect();
    if panel.is_some_and(|p| *p == crate::appearance::SidePanel::SheetMetal)
        && let Some(t) = sm
    {
        others.push(t.view.back());
    }
    for (label, mat) in &q {
        let n = label.0.normal();
        let a = face_label_alpha(others.iter().fold(n.dot(back), |m, o| m.max(n.dot(*o))));
        // Premultiplied: scale every channel.
        let c = Color::LinearRgba(LinearRgba::new(a, a, a, a));
        if let Some(m) = materials.get(&mat.0)
            && m.base_color != c
            && let Some(mut m) = materials.get_mut(&mat.0)
        {
            m.base_color = c;
        }
    }
}

/// Renders the face labels into the atlas the cube faces are textured with.
#[derive(Component)]
struct LabelAtlasCamera;

/// Atlas cell size (px) of one face label: about 2.5x the face's size on screen, so the
/// (foreshortened) labels are sampled down, never up.
const ATLAS_CELL: u32 = 128;

#[derive(Component)]
struct AxisLabel(Vec3);

/// A nudge arrow: rotates the view by (yaw, pitch) degrees.
#[derive(Component, Clone, Copy)]
struct CubeArrow(f32, f32);

fn setup_cube(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut store: ResMut<GizmoConfigStore>,
    theme: Res<Theme>,
) {
    let size = (CUBE_WIDGET * CUBE_SUPERSAMPLE).as_uvec2();
    let mut image = Image::new_target_texture(
        size.x,
        size.y,
        TextureFormat::Rgba8UnormSrgb,
        None,
    );
    image.asset_usage = RenderAssetUsages::default();
    let handle = images.add(image);
    commands.insert_resource(ViewCubeImage(handle.clone()));

    let (config, _) = store.config_mut::<CubeGizmos>();
    config.render_layers = RenderLayers::layer(CUBE_LAYER);
    config.line.width = 2.4;
    let (config, _) = store.config_mut::<CubeArcGizmos>();
    config.render_layers = RenderLayers::layer(CUBE_ARC_LAYER);
    config.line.width = 12.0;

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

    // The face labels: white cells with the name, rendered by a UI camera into an atlas
    // that the faces use as their texture (tinted light blue on hover).
    let n = CubeFace::ALL.len() as u32;
    let mut atlas = Image::new_target_texture(
        ATLAS_CELL * n,
        ATLAS_CELL,
        TextureFormat::Rgba8UnormSrgb,
        None,
    );
    atlas.asset_usage = RenderAssetUsages::default();
    let atlas = images.add(atlas);
    let atlas_cam = commands
        .spawn((
            Name::new("view-cube-label-camera"),
            LabelAtlasCamera,
            Camera2d,
            RenderTarget::Image(atlas.clone().into()),
            Camera {
                order: -3,
                is_active: false,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            Tonemapping::None,
            DebandDither::Disabled,
            RenderLayers::layer(31),
        ))
        .id();
    commands
        .spawn((
            Name::new("view-cube-label-atlas"),
            bevy::ui::UiTargetCamera(atlas_cam),
            Node {
                width: Val::Px((ATLAS_CELL * n) as f32),
                height: Val::Px(ATLAS_CELL as f32),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|row| {
            for face in CubeFace::ALL {
                row.spawn((
                    Node {
                        width: Val::Px(ATLAS_CELL as f32),
                        height: Val::Px(ATLAS_CELL as f32),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_child((
                    theme.text(face.label(), 32.0, FontWeight::SEMIBOLD, theme.foreground),
                    Pickable::IGNORE,
                ));
            }
        });
    let mat = |c: Color, tex: Option<Handle<Image>>| StandardMaterial {
        base_color: c,
        base_color_texture: tex,
        unlit: true,
        ..default()
    };
    let normal = materials.add(mat(theme.view_cube_face(), None));
    let hover = materials.add(mat(theme.view_cube_hover(), None));
    let hover_mat = hover.clone();
    let frame = materials.add(mat(theme.view_cube_frame(), None));
    commands.insert_resource(CubeMaterials {
        normal: normal.clone(),
        hover,
    });
    // Each side: a light grey-blue frame with a white rounded-looking face inset in it.
    let body = meshes.add(Rectangle::new(2.0, 2.0));
    for (i, face) in CubeFace::ALL.into_iter().enumerate() {
        // This face's cell of the label atlas.
        let mut mesh = Mesh::from(Rectangle::new(1.56, 1.56));
        if let Some(bevy::mesh::VertexAttributeValues::Float32x2(uvs)) =
            mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0)
        {
            for uv in uvs.iter_mut() {
                uv[0] = (i as f32 + uv[0]) / n as f32;
            }
        }
        let label_quad = meshes.add(mesh);
        let quad = meshes.add(Rectangle::new(1.56, 1.56));
        // The label: text on transparent, premultiplied by the UI pass; faded per face.
        let label_mat = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(atlas.clone()),
            unlit: true,
            alpha_mode: AlphaMode::Premultiplied,
            ..default()
        });
        let (u, v) = face.frame();
        let n = face.normal();
        let rot = Quat::from_mat3(&Mat3::from_cols(u, v, n));
        commands.spawn((
            Name::new("view-cube-side"),
            Mesh3d(body.clone()),
            MeshMaterial3d(frame.clone()),
            Transform::from_translation(n).with_rotation(rot),
            RenderLayers::layer(CUBE_LAYER),
        ));
        commands.spawn((
            Name::new(format!("view-cube-face-{}", face.label().to_lowercase())),
            face,
            Mesh3d(quad),
            MeshMaterial3d(normal.clone()),
            Transform::from_translation(n * 1.004).with_rotation(rot),
            RenderLayers::layer(CUBE_LAYER),
        ));
        commands.spawn((
            Name::new("view-cube-face-label"),
            FaceLabel(face),
            Mesh3d(label_quad),
            MeshMaterial3d(label_mat),
            Transform::from_translation(n * 1.01).with_rotation(rot),
            RenderLayers::layer(CUBE_LAYER),
        ));
    }
    // P6.4: each corner's hover patch, a square on each of its three faces (over the face and
    // its label, so the whole corner lights up).
    let patch = meshes.add(Rectangle::new(CORNER_SIZE, CORNER_SIZE));
    for x in [-1, 1] {
        for y in [-1, 1] {
            for z in [-1, 1] {
                let c = IVec3::new(x, y, z);
                let cf = c.as_vec3();
                commands
                    .spawn((
                        Name::new(format!("view-cube-corner-{}{}{}", sign(x), sign(y), sign(z))),
                        CornerPatch(c),
                        Transform::default(),
                        Visibility::Hidden,
                        RenderLayers::layer(CUBE_LAYER),
                    ))
                    .with_children(|p| {
                        for face in CubeFace::ALL {
                            let n = face.normal();
                            if n.dot(cf) < 0.5 {
                                continue;
                            }
                            let (u, v) = face.frame();
                            let rot = Quat::from_mat3(&Mat3::from_cols(u, v, n));
                            let at = n * 1.012 + u * (u.dot(cf) * (1.0 - CORNER_SIZE / 2.0)) + v * (v.dot(cf) * (1.0 - CORNER_SIZE / 2.0));
                            p.spawn((
                                Mesh3d(patch.clone()),
                                MeshMaterial3d(hover_mat.clone()),
                                Transform::from_translation(at).with_rotation(rot),
                                RenderLayers::layer(CUBE_LAYER),
                            ));
                        }
                    });
            }
        }
    }
}

/// "p" or "m" for a corner's name ("view-cube-corner-pmp": x+, y−, z+).
fn sign(v: i32) -> &'static str {
    if v > 0 { "p" } else { "m" }
}

#[allow(clippy::type_complexity)]
fn activate_cube_camera<const ON: bool>(
    mut q: Query<&mut Camera, Or<(With<CubeCamera>, With<LabelAtlasCamera>)>>,
) {
    for mut c in &mut q {
        c.is_active = ON;
    }
}

pub(crate) trait CubeColors {
    fn view_cube_frame(&self) -> Color;
    fn view_cube_face(&self) -> Color;
    fn view_cube_hover(&self) -> Color;
    fn view_cube_edge(&self) -> Color;
    fn view_cube_arrow(&self) -> Color;
}

impl CubeColors for Theme {
    fn view_cube_frame(&self) -> Color {
        Color::srgb_u8(0xe6, 0xeb, 0xef)
    }
    fn view_cube_face(&self) -> Color {
        Color::WHITE
    }
    fn view_cube_hover(&self) -> Color {
        Color::srgb_u8(0xcf, 0xe3, 0xf7)
    }
    fn view_cube_edge(&self) -> Color {
        Color::srgb_u8(0xd0, 0xd6, 0xdb)
    }
    fn view_cube_arrow(&self) -> Color {
        Color::srgb_u8(0xe1, 0xe9, 0xef)
    }
}

/// Spawns the view cube widget (inside the viewport area, top right).
pub fn spawn_view_cube(p: &mut ChildSpawnerCommands, theme: &Theme, image: Handle<Image>) {
    p.spawn((
        Name::new("view-cube"),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Px(CUBE_WIDGET.x),
            height: Val::Px(CUBE_WIDGET.y),
            ..default()
        },
        Pickable::IGNORE,
    ))
    .with_children(|w| {
        w.spawn((
            Name::new("view-cube-render"),
            ImageNode::new(image),
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
        // An invisible node over the cube: hovering and clicking it picks a face. Outside it
        // the pointer still reaches the viewport.
        w.spawn((
            Name::new("view-cube-image"),
            CubeImageNode,
            Hovered::default(),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(CUBE_CENTER.x - 46.0),
                top: Val::Px(CUBE_CENTER.y - 46.0),
                width: Val::Px(92.0),
                height: Val::Px(92.0),
                ..default()
            },
        ));
        for (axis, letter, color) in [
            // Label colors measured in `screens/09`: more saturated than the triad lines.
            (Vec3::X, "X", Color::srgb_u8(0xd0, 0x30, 0x30)),
            (Vec3::Y, "Y", Color::srgb_u8(0x3a, 0x9a, 0x3a)),
            (Vec3::Z, "Z", theme.axis_z),
        ] {
            w.spawn((
                Name::new(format!("view-cube-axis-{}", letter.to_lowercase())),
                AxisLabel(axis),
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                theme.text(letter, 12.0, FontWeight::BOLD, color),
                Pickable::IGNORE,
            ));
        }
        let arrow = theme.view_cube_arrow();
        for (name, icon_name, left, top, yaw, pitch) in [
            ("view-cube-up", "caret-up-filled", CUBE_CENTER.x - 8.0, 10.0, 0.0, -15.0),
            (
                "view-cube-down",
                "caret-down-filled",
                CUBE_CENTER.x - 8.0,
                CUBE_CENTER.y + 56.0,
                0.0,
                15.0,
            ),
            ("view-cube-left", "caret-left-filled", 14.0, CUBE_CENTER.y - 8.0, -15.0, 0.0),
            (
                "view-cube-right",
                "caret-right-filled",
                CUBE_CENTER.x + 56.0,
                CUBE_CENTER.y - 8.0,
                15.0,
                0.0,
            ),
        ] {
            w.spawn((
                cadrs_ui::IconButton::new(name, icon_name)
                    .icon_size(16.0)
                    .build(theme),
                CubeArrow(yaw, pitch),
            ))
            .insert((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(left),
                    top: Val::Px(top),
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
        // The view menu (`reference/onshape/view/view-cube-menu4-01.png`).
        w.spawn((
            cadrs_ui::Button::new("render-mode")
                .icon("part")
                .ghost()
                .dropdown_caret()
                .icon_size(16.0)
                .tooltip("View options")
                .build(theme),
            cadrs_ui::prelude::observe(
                |a: On<Activate>, theme: Res<Theme>, mut commands: Commands| {
                    cadrs_ui::open_menu(&mut commands, a.entity, view_menu().build(&theme));
                },
            ),
            cadrs_ui::prelude::observe(on_view_menu),
        ))
        .insert(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(116.0),
            top: Val::Px(136.0),
            height: Val::Px(22.0),
            padding: UiRect::horizontal(Val::Px(3.0)),
            column_gap: Val::Px(2.0),
            align_items: AlignItems::Center,
            ..default()
        });
    });
}

/// The view cube's ▾ menu, grouped like Onshape's; what cadrs does not have yet is disabled,
/// and the render options it always uses are shown checked.
fn view_menu() -> cadrs_ui::Menu {
    use cadrs_ui::MenuItem as I;
    cadrs_ui::Menu::new("view-menu")
        .align_end()
        .min_width(222.0)
        .item_height(23.0)
        .item(I::new("view-isometric", "Isometric").shortcut("Shift+7"))
        .item(I::new("view-dimetric", "Dimetric"))
        .item(I::new("view-trimetric", "Trimetric"))
        .separator()
        .item(I::new("view-graphics", "Graphics preferences…").icon("settings").disabled(true))
        .separator()
        .item(I::new("view-named", "Named views…").disabled(true))
        .item(I::new("view-previous", "Previous view").disabled(true))
        .separator()
        .item(I::new("view-zoom-to-fit", "Zoom to fit").shortcut("F"))
        .item(I::new("view-zoom-window", "Zoom to window").disabled(true))
        .separator()
        .item(I::new("view-perspective", "Perspective view").disabled(true))
        .item(I::new("view-orient-normal", "Orient normal to sketch on edit").disabled(true))
        .separator()
        .item(
            I::new("view-shaded", "Shaded with edges")
                .icon("check")
                .submenu(vec![]),
        )
        .item(
            I::new("view-hidden-edges", "Hidden edges removed")
                .icon("check")
                .submenu(vec![]),
        )
        .item(
            I::new("view-tangent-edges", "Tangent edges visible")
                .icon("check")
                .submenu(vec![]),
        )
        .separator()
        .item(I::new("view-high-quality", "View in high quality").disabled(true))
        .item(I::new("view-boundary", "Highlight boundary edges").disabled(true))
        .separator()
        .item(I::new("view-section", "Section view…").icon("section-view").disabled(true))
}

fn on_view_menu(
    ev: On<cadrs_ui::MenuAction>,
    rect: Res<crate::viewport::ViewportRect>,
    mut view: ResMut<ViewportView>,
    mut commands: Commands,
) {
    let target = view.target();
    let to = match ev.item.as_str() {
        "view-isometric" => crate::viewport::fitted_isometric(rect.0.size()),
        // Dimetric: two axes foreshortened equally; trimetric: Onshape's default view.
        "view-dimetric" => ViewState {
            azimuth: DIMETRIC.0,
            elevation: DIMETRIC.1,
            ..target
        },
        "view-trimetric" => target.oriented(crate::camera::StandardView::Default),
        "view-zoom-to-fit" => {
            commands.queue(crate::viewport::zoom_to_fit);
            return;
        }
        _ => return,
    };
    view.animate_to(to);
}

/// The cube camera's projection (P3D.4: the Repair panel's cube uses it too).
pub(crate) fn cube_projection() -> Projection {
    Projection::Orthographic(OrthographicProjection {
        scaling_mode: ScalingMode::WindowSize,
        scale: 1.0 / (CUBE_PX * CUBE_SUPERSAMPLE),
        near: 0.0,
        far: 100.0,
        viewport_origin: Vec2::new(CUBE_CENTER.x / CUBE_WIDGET.x, 1.0 - CUBE_CENTER.y / CUBE_WIDGET.y),
        ..OrthographicProjection::default_3d()
    })
}

/// The view a click at `widget_pos` (px from the widget's top-left) on a cube seen from `view`
/// turns to: a face's standard view or a corner's trimetric one (P3D.4: the Repair panel's
/// cube).
pub(crate) fn view_at_spot(view: &ViewState, widget_pos: Vec2) -> Option<ViewState> {
    match spot_at(view, widget_pos)? {
        CubeSpot::Face(face) => Some(view.oriented(face.view())),
        CubeSpot::Corner(c) => {
            let (azimuth, elevation) = corner_view(c);
            Some(ViewState { azimuth, elevation, roll: 0.0, ..*view })
        }
    }
}

fn cube_view(view: &ViewState) -> ViewState {
    ViewState {
        focus: Vec3::ZERO,
        scale: 1.0 / CUBE_PX,
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

/// The face under a point of the widget (px from the widget's top-left).
#[cfg(test)]
fn face_at(view: &ViewState, widget_pos: Vec2) -> Option<CubeFace> {
    match spot_at(view, widget_pos)? {
        CubeSpot::Face(f) => Some(f),
        CubeSpot::Corner(_) => None,
    }
}

/// The face or corner under a point of the widget: a hit within [`CORNER_SIZE`] of two edges
/// of a face is on the corner there.
fn spot_at(view: &ViewState, widget_pos: Vec2) -> Option<CubeSpot> {
    let v = cube_view(view);
    let (o, d) = v.ray(widget_pos - CUBE_CENTER);
    let (t, f) = CubeFace::ALL
        .into_iter()
        .filter_map(|f| {
            let (u, w) = f.frame();
            ray_square(o, d, f.normal(), u, w, 1.0).map(|t| (t, f))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))?;
    let p = o + d * t;
    let (u, w) = f.frame();
    let (a, b) = (p.dot(u), p.dot(w));
    let edge = 1.0 - CORNER_SIZE;
    if a.abs() > edge && b.abs() > edge {
        let c = f.normal() + u * a.signum() + w * b.signum();
        return Some(CubeSpot::Corner(c.round().as_ivec3()));
    }
    Some(CubeSpot::Face(f))
}

fn widget_origin(
    q: &Query<(&ComputedNode, &bevy::ui::UiGlobalTransform), With<CubeImageNode>>,
) -> Option<Vec2> {
    let (node, t) = q.iter().next()?;
    let s = node.inverse_scale_factor();
    let center = t.translation * s;
    // The image node's center is the cube center.
    Some(center - CUBE_CENTER)
}

#[allow(clippy::too_many_arguments)]
fn cube_hover(
    view: Res<ViewportView>,
    drag: Res<crate::viewport::ViewportDrag>,
    q_node: Query<(&ComputedNode, &bevy::ui::UiGlobalTransform), With<CubeImageNode>>,
    q_hovered: Query<&Hovered, With<CubeImageNode>>,
    mut hover: ResMut<CubeHover>,
    mut corner_hover: ResMut<CornerHover>,
    materials: Option<Res<CubeMaterials>>,
    mut q_faces: Query<(&CubeFace, &mut MeshMaterial3d<StandardMaterial>)>,
    mut q_corners: Query<(&CornerPatch, &mut Visibility)>,
) {
    let hovered = q_hovered.iter().any(|h| h.get());
    let spot = if hovered {
        widget_origin(&q_node).and_then(|o| spot_at(&view.view, drag.pointer() - o))
    } else {
        None
    };
    let (face, corner) = match spot {
        Some(CubeSpot::Face(f)) => (Some(f), None),
        Some(CubeSpot::Corner(c)) => (None, Some(c)),
        None => (None, None),
    };
    if hover.0 != face {
        hover.0 = face;
    }
    if corner_hover.0 != corner {
        corner_hover.0 = corner;
    }
    for (p, mut vis) in &mut q_corners {
        vis.set_if_neq(if Some(p.0) == corner { Visibility::Inherited } else { Visibility::Hidden });
    }
    let Some(m) = materials else {
        return;
    };
    for (f, mut mat) in &mut q_faces {
        let want = if Some(*f) == hover.0 {
            &m.hover
        } else {
            &m.normal
        };
        if mat.0 != *want {
            mat.0 = want.clone();
        }
    }
}

fn draw_cube(
    mut gizmos: Gizmos<CubeGizmos>,
    mut arcs: Gizmos<CubeArcGizmos>,
    view: Res<ViewportView>,
    theme: Res<Theme>,
) {
    draw_cube_arcs(&mut arcs, &view.view, theme.view_cube_arrow());
    let edge = theme.view_cube_edge();
    let c = |x: f32, y: f32, z: f32| Vec3::new(x, y, z) * 0.97;
    let corners = [
        c(-1.0, -1.0, -1.0),
        c(1.0, -1.0, -1.0),
        c(1.0, 1.0, -1.0),
        c(-1.0, 1.0, -1.0),
        c(-1.0, -1.0, 1.0),
        c(1.0, -1.0, 1.0),
        c(1.0, 1.0, 1.0),
        c(-1.0, 1.0, 1.0),
    ];
    for i in 0..4 {
        gizmos.line(corners[i], corners[(i + 1) % 4], edge);
        gizmos.line(corners[i + 4], corners[(i + 1) % 4 + 4], edge);
        gizmos.line(corners[i], corners[i + 4], edge);
    }
    let o = TRIAD_ORIGIN;
    for (axis, color) in [
        (Vec3::X, theme.axis_x),
        (Vec3::Y, theme.axis_y),
        (Vec3::Z, theme.axis_z),
    ] {
        gizmos.line(o, o + axis * TRIAD_LEN, color);
    }
}

/// The two curved rotate arrows (about 50 px radius) at the cube's top left and top right, drawn
/// in `view`'s screen plane in front of the cube (the main cube's, and the Repair view's own).
pub(crate) fn draw_cube_arcs<G: GizmoConfigGroup>(arcs: &mut Gizmos<G>, view: &ViewState, arc_color: Color) {
    let v = cube_view(view);
    let (r, u, b) = (v.right(), v.up(), v.back());
    let radius = 62.0 / CUBE_PX;
    let at = |deg: f32| {
        let a = deg.to_radians();
        (r * a.cos() + u * a.sin()) * radius + b * 5.0
    };
    for (from, to) in [(108.0_f32, 143.0_f32), (72.0, 37.0)] {
        const N: usize = 12;
        for i in 0..N {
            let a0 = from + (to - from) * i as f32 / N as f32;
            let a1 = from + (to - from) * (i + 1) as f32 / N as f32;
            arcs.line(at(a0), at(a1), arc_color);
        }
        // Arrowhead at the outer, lower end.
        let end = at(to);
        let tangent = (at(to) - at(to - (to - from) * 0.1)).normalize();
        let normal = (end - b * 5.0).normalize();
        let back_pt = end - tangent * 0.28;
        arcs.line(end + tangent * 0.1, back_pt + normal * 0.26, arc_color);
        arcs.line(end + tangent * 0.1, back_pt - normal * 0.26, arc_color);
    }
}

const TRIAD_LEN: f32 = 2.45;

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

/// Where an axis letter of the cube's triad goes for `view` (its label's top-left in the cube
/// widget, for a label of `size`) and its opacity: the main cube's and the flat view's.
pub(crate) fn axis_label_spot(view: &ViewState, axis: Vec3, size: Vec2) -> (Vec2, f32) {
    let v = cube_view(view);
    let tip = TRIAD_ORIGIN + axis * (TRIAD_LEN + 0.35);
    // An axis pointing (nearly) at the viewer has no label, as in Onshape's normal views
    // (`screens/09` shows only X and Y); a tip behind the cube shows faintly, as if seen
    // through it.
    let along = axis.dot(v.back());
    let a = if along.abs() > 0.9 {
        0.0
    } else if along < -0.3 {
        0.45
    } else {
        1.0
    };
    // A tip in front of the cube's face labels moves out along its axis until the label
    // clears the cube's outline, so "Z" never sits on "Top".
    let mut offset = v.project(tip);
    let dir = v.project_vector(axis).normalize_or_zero();
    let clear = CUBE_PX * 1.62 + size.max_element() / 2.0;
    if along > -0.3 && dir.dot(offset) > 0.0 {
        let mut n = 0;
        while offset.length() < clear && n < 40 {
            offset += dir * 2.0;
            n += 1;
        }
    }
    let p = CUBE_CENTER + offset;
    (Vec2::new(p.x - size.x / 2.0, p.y - size.y / 2.0), a)
}

fn on_cube_click(
    click: On<Pointer<Click>>,
    q: Query<(), With<CubeImageNode>>,
    q_node: Query<(&ComputedNode, &bevy::ui::UiGlobalTransform), With<CubeImageNode>>,
    mut view: ResMut<ViewportView>,
) {
    if click.button != PointerButton::Primary || !q.contains(click.entity) {
        return;
    }
    let Some(origin) = widget_origin(&q_node) else {
        return;
    };
    match spot_at(&view.view, click.pointer_location.position - origin) {
        Some(CubeSpot::Face(face)) => {
            let to = view.target().oriented(face.view());
            view.animate_to(to);
        }
        Some(CubeSpot::Corner(c)) => {
            let (azimuth, elevation) = corner_view(c);
            let to = ViewState { azimuth, elevation, roll: 0.0, ..view.target() };
            view.animate_to(to);
        }
        None => {}
    }
}

/// A view cube arrow rotates 15°; Shift+click 90°, Ctrl+click 5° (`shortcuts.md`).
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
        let mut to = view.target();
        to.rotate_by(a.0 * k, a.1 * k);
        view.animate_to(to);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_labels_fade_when_turned_away() {
        assert_eq!(face_label_alpha(1.0), 1.0);
        assert_eq!(face_label_alpha(75f32.to_radians().cos() - 0.01), 0.0);
        assert!(face_label_alpha(0.32) > 0.0 && face_label_alpha(0.32) < 1.0);
    }

    #[test]
    fn clicking_the_cube_center_hits_the_front_face_from_the_front() {
        let v = ViewState::standard(StandardView::Front);
        assert_eq!(face_at(&v, CUBE_CENTER), Some(CubeFace::Front));
        let top = ViewState::standard(StandardView::Top);
        assert_eq!(face_at(&top, CUBE_CENTER), Some(CubeFace::Top));
        // In the default view the top face is above the center and the right face to the right.
        let d = ViewState::default();
        assert_eq!(face_at(&d, CUBE_CENTER + Vec2::new(0.0, -30.0)), Some(CubeFace::Top));
        assert_eq!(face_at(&d, CUBE_CENTER + Vec2::new(24.0, 4.0)), Some(CubeFace::Right));
        assert_eq!(face_at(&d, CUBE_CENTER + Vec2::new(-10.0, 12.0)), Some(CubeFace::Front));
        assert_eq!(face_at(&d, CUBE_CENTER + Vec2::new(80.0, 0.0)), None);
    }

    #[test]
    fn axonometric_axis_scales() {
        // P3F.4 (P6.3): the lengths the unit X, Y and Z axes project to.
        let scales = |v: ViewState| [Vec3::X, Vec3::Y, Vec3::Z].map(|a| v.project_vector(a).length() * v.scale);
        // Isometric (view direction (1, 1, 1)/√3): all three √(2/3) = 0.81650.
        let iso = scales(ViewState::standard(StandardView::Isometric));
        for k in iso {
            assert!((k - (2.0f32 / 3.0).sqrt()).abs() < 1e-5, "{iso:?}");
        }
        // Dimetric: exactly two equal (X and Y), Z different.
        let di = scales(ViewState { azimuth: DIMETRIC.0, elevation: DIMETRIC.1, ..ViewState::default() });
        assert!((di[0] - di[1]).abs() < 1e-5, "{di:?}");
        assert!((di[0] - di[2]).abs() > 0.05, "{di:?}");
        // Trimetric (Onshape's default view, 30° round and 30° up): three different lengths,
        // (1 − cos²30·sin²30)^½ = 0.9014, (1 − cos⁴30)^½ = 0.6614 and cos 30 = 0.8660.
        let tri = scales(ViewState::standard(StandardView::Default));
        for (k, want) in tri.iter().zip([0.901_388, 0.661_438, 0.866_025]) {
            assert!((k - want).abs() < 1e-5, "{tri:?}");
        }
        assert!((tri[0] - tri[1]).abs() > 0.01 && (tri[1] - tri[2]).abs() > 0.01 && (tri[0] - tri[2]).abs() > 0.01, "{tri:?}");
        // Orthographic projections keep the sum of squares at 2 (the axes are orthonormal).
        for k in [iso, di, tri] {
            assert!((k.iter().map(|x| x * x).sum::<f32>() - 2.0).abs() < 1e-4, "{k:?}");
        }
    }

    #[test]
    fn corners_turn_to_trimetric_views() {
        // The default view looks from the front-right-top corner.
        assert_eq!(corner_view(IVec3::new(1, -1, 1)), StandardView::Default.angles());
        // Every corner's view looks from that corner (its view direction is in its octant) and
        // no two axes are foreshortened alike (trimetric).
        for x in [-1, 1] {
            for y in [-1, 1] {
                for z in [-1, 1] {
                    let c = IVec3::new(x, y, z);
                    let (azimuth, elevation) = corner_view(c);
                    let v = ViewState { azimuth, elevation, ..ViewState::default() };
                    let b = v.back();
                    assert!(b.x * x as f32 > 0.0 && b.y * y as f32 > 0.0 && b.z * z as f32 > 0.0, "{c}: {b}");
                    let k = [Vec3::X, Vec3::Y, Vec3::Z].map(|a| v.project_vector(a).length());
                    assert!((k[0] - k[1]).abs() > 0.05 && (k[1] - k[2]).abs() > 0.05 && (k[0] - k[2]).abs() > 0.05, "{k:?}");
                }
            }
        }
        // A click near a corner of the front face in the Front view hits that corner.
        let front = ViewState::standard(StandardView::Front);
        let near = CUBE_CENTER + Vec2::new(0.9, -0.9) * CUBE_PX;
        assert_eq!(spot_at(&front, near), Some(CubeSpot::Corner(IVec3::new(1, -1, 1))));
        assert_eq!(spot_at(&front, CUBE_CENTER), Some(CubeSpot::Face(CubeFace::Front)));
    }
}
