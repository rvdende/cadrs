//! A colour editor: the body of the Edit appearance dialog (PS9.2), for any dialog that picks a
//! colour. Built from [`crate::color_picker`]'s swatches and mixer:
//!
//! - **Colors**: preset swatches, eight to a row;
//! - **Custom colors** (when given): the saved colours, then **+** to save the current one
//!   ([`ColorEditorSaveCustom`]); each is a [`ContextMenuTarget`](crate::menu::ContextMenuTarget)
//!   ([`CustomColorSwatch`]) so the app can offer Update and Delete;
//! - **Mixer**: the saturation/value square and hue strip;
//! - the current colour, its **Hex** code and **R G B** values;
//! - **Opacity** (optional): a slider and its percentage;
//! - **Default** (optional): [`ColorEditorDefault`]; the app decides the colour it goes back to.
//!
//! Every pick sets the editor's [`ColorEditorState`] and triggers [`ColorEditorChange`] on the
//! editor; setting the state from outside moves every control. Set [`ColorEditorCustom`] to
//! change the custom colours shown. Names start with the editor's prefix: `<prefix>-editor`,
//! `<prefix>-swatch-<i>`, `<prefix>-custom-row`, `<prefix>-custom-<i>`, `<prefix>-add-custom`,
//! `<prefix>-mixer`, `<prefix>-current`, `<prefix>-hex`, `<prefix>-r|g|b`, `<prefix>-opacity`,
//! `<prefix>-opacity-value`, `<prefix>-reset`.

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Activate;

use crate::button::{Button, ButtonSize, IconButton};
use crate::color_picker::{
    ColorMixer, ColorMixerChange, ColorMixerState, ColorSwatch, SwatchColor, SwatchSelected,
};
use crate::dialog_fields::{
    NumberField, NumberFieldCommit, NumberFieldState, Slider, SliderChange, SliderState,
};
use crate::menu::{ContextMenuTarget, LastPointerButton};
use crate::theme::Theme;

pub struct ColorEditorPlugin;

impl Plugin for ColorEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_swatch)
            .add_observer(on_mixer)
            .add_observer(on_field)
            .add_observer(on_opacity)
            .add_observer(on_button)
            .add_systems(
                PostUpdate,
                (sync_custom, sync_controls)
                    .chain()
                    .before(bevy::ui::UiSystems::Prepare),
            );
    }
}

/// The editor's colour (sRGB and opacity, 0–255). Set it to move every control.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorEditorState {
    pub rgba: [u8; 4],
}

/// The custom colours the editor shows; set it to change the row.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct ColorEditorCustom(pub Vec<[u8; 3]>);

/// A colour was picked in the editor (a swatch, the mixer, a typed value, the opacity).
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct ColorEditorChange {
    pub entity: Entity,
    pub rgba: [u8; 4],
}

/// **Default** was pressed in the editor.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct ColorEditorDefault {
    pub entity: Entity,
}

/// **+** was pressed: save this colour among the custom ones.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct ColorEditorSaveCustom {
    pub entity: Entity,
    pub rgb: [u8; 3],
}

/// A custom colour's swatch: its index in the list, and its editor.
#[derive(Component, Debug, Clone, Copy)]
pub struct CustomColorSwatch {
    pub index: usize,
    pub editor: Entity,
}

/// Which control a part of an editor is, and its editor.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
struct Piece {
    editor: Entity,
    kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Preset,
    Custom,
    CustomRow,
    AddCustom,
    Mixer,
    Current,
    Hex,
    Channel(usize),
    Opacity,
    OpacityText,
    Default,
}

/// Builder for a colour editor.
pub struct ColorEditor {
    prefix: String,
    presets: Vec<[u8; 3]>,
    custom: Option<Vec<[u8; 3]>>,
    rgba: [u8; 4],
    opacity: bool,
    default: bool,
    width: f32,
}

impl ColorEditor {
    /// `prefix` starts its controls' names.
    pub fn new(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            presets: vec![],
            custom: None,
            rgba: [255, 255, 255, 255],
            opacity: false,
            default: false,
            width: 270.0,
        }
    }

    /// The preset swatches, eight to a row.
    pub fn presets(mut self, colors: impl IntoIterator<Item = [u8; 3]>) -> Self {
        self.presets = colors.into_iter().collect();
        self
    }

    /// Shows the custom colours (and **+**).
    pub fn custom(mut self, colors: Vec<[u8; 3]>) -> Self {
        self.custom = Some(colors);
        self
    }

    pub fn color(mut self, rgba: [u8; 4]) -> Self {
        self.rgba = rgba;
        self
    }

    /// Shows the opacity slider.
    pub fn opacity(mut self) -> Self {
        self.opacity = true;
        self
    }

    /// Shows **Default**.
    pub fn default_button(mut self) -> Self {
        self.default = true;
        self
    }

    /// The mixer's width (px); the editor is as wide.
    pub fn width(mut self, px: f32) -> Self {
        self.width = px;
        self
    }

    pub fn build(self, t: &Theme) -> impl Bundle {
        let t = t.clone();
        let Self {
            prefix,
            presets,
            custom,
            rgba,
            opacity,
            default: reset,
            width,
        } = self;
        (
            Name::new(format!("{prefix}-editor")),
            ColorEditorState { rgba },
            ColorEditorCustom(custom.clone().unwrap_or_default()),
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                width: Val::Px(width),
                ..default()
            },
            Children::spawn(SpawnWith(move |c: &mut ChildSpawner| {
                let editor = c.target_entity();
                let piece = |kind| Piece { editor, kind };
                let rgb = [rgba[0], rgba[1], rgba[2]];
                section(c, &t, "Colors");
                for (r, row) in presets.chunks(8).enumerate() {
                    c.spawn(Node {
                        column_gap: Val::Px(6.0),
                        ..default()
                    })
                    .with_children(|s| {
                        for (k, p) in row.iter().enumerate() {
                            let i = r * 8 + k;
                            s.spawn((
                                ColorSwatch::new(format!("{prefix}-swatch-{i}"), srgb(*p))
                                    .size(24.0)
                                    .selected(*p == rgb)
                                    .tooltip(hex(*p))
                                    .build(&t),
                                piece(Kind::Preset),
                            ));
                        }
                    });
                }
                if let Some(custom) = &custom {
                    section(c, &t, "Custom colors");
                    c.spawn((
                        Name::new(format!("{prefix}-custom-row")),
                        piece(Kind::CustomRow),
                        Node {
                            column_gap: Val::Px(6.0),
                            align_items: AlignItems::Center,
                            min_height: Val::Px(24.0),
                            ..default()
                        },
                    ))
                    .with_children(|r| custom_row(r, &t, &prefix, editor, custom, rgb));
                }
                section(c, &t, "Mixer");
                let [h, s, v] = hsv(rgb);
                c.spawn((
                    ColorMixer::new(format!("{prefix}-mixer"))
                        .size(width, 96.0)
                        .hsv(h, s, v)
                        .build(&t),
                    piece(Kind::Mixer),
                ));
                c.spawn(Node {
                    column_gap: Val::Px(8.0),
                    align_items: AlignItems::Center,
                    margin: UiRect::top(Val::Px(2.0)),
                    ..default()
                })
                .with_children(|r| {
                    r.spawn((
                        Name::new(format!("{prefix}-current")),
                        piece(Kind::Current),
                        Node {
                            width: Val::Px(30.0),
                            height: Val::Px(30.0),
                            flex_shrink: 0.0,
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            ..default()
                        },
                        BackgroundColor(rgba_color(rgba)),
                        crate::color_scheme::KeepFill,
                        BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.25)),
                    ));
                    r.spawn((
                        NumberField::new(format!("{prefix}-hex"), "Hex")
                            .label_width(26.0)
                            .text(hex(rgb))
                            .build(&t),
                        piece(Kind::Hex),
                        // A colour, not a number: the wheel doesn't step it.
                        crate::dialog_fields::NoWheelStep,
                    ))
                    .entry::<Node>()
                    .and_modify(|mut n| n.width = Val::Px(110.0));
                });
                c.spawn(Node {
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|r| {
                    for (k, label) in ["R", "G", "B"].into_iter().enumerate() {
                        r.spawn((
                            NumberField::new(format!("{prefix}-{}", label.to_lowercase()), label)
                                .label_width(12.0)
                                .text(rgb[k].to_string())
                                .build(&t),
                            piece(Kind::Channel(k)),
                        ))
                        .entry::<Node>()
                        .and_modify(|mut n| {
                            n.flex_grow = 1.0;
                            n.flex_basis = Val::Px(0.0);
                        });
                    }
                });
                if opacity {
                    c.spawn(Node {
                        column_gap: Val::Px(8.0),
                        align_items: AlignItems::Center,
                        height: Val::Px(24.0),
                        ..default()
                    })
                    .with_children(|r| {
                        r.spawn((
                            t.text(
                                "Opacity",
                                11.0,
                                FontWeight::NORMAL,
                                Color::srgb_u8(0x6b, 0x6b, 0x6b),
                            ),
                            Node {
                                width: Val::Px(52.0),
                                ..default()
                            },
                        ));
                        r.spawn((
                            Slider::new(format!("{prefix}-opacity"))
                                .value(rgba[3] as f32 / 255.0)
                                .width(160.0)
                                .tooltip("Opacity")
                                .build(&t),
                            piece(Kind::Opacity),
                        ));
                        r.spawn((
                            Name::new(format!("{prefix}-opacity-value")),
                            piece(Kind::OpacityText),
                            t.text(percent(rgba[3]), 12.0, FontWeight::MEDIUM, t.foreground),
                        ));
                    });
                }
                if reset {
                    c.spawn(Node {
                        justify_content: JustifyContent::FlexEnd,
                        ..default()
                    })
                    .with_child((
                        Button::new(format!("{prefix}-reset"))
                            .label("Default")
                            .size(ButtonSize::Small)
                            .build(&t),
                        piece(Kind::Default),
                    ));
                }
            })),
        )
    }
}

fn section(p: &mut ChildSpawner, t: &Theme, label: &str) {
    p.spawn((
        t.text(
            label.to_string(),
            11.0,
            FontWeight::MEDIUM,
            t.muted_foreground,
        ),
        Node {
            margin: UiRect::top(Val::Px(4.0)),
            ..default()
        },
    ));
}

fn custom_row(
    r: &mut ChildSpawner,
    t: &Theme,
    prefix: &str,
    editor: Entity,
    custom: &[[u8; 3]],
    rgb: [u8; 3],
) {
    for (i, c) in custom.iter().enumerate() {
        r.spawn((
            ColorSwatch::new(format!("{prefix}-custom-{i}"), srgb(*c))
                .size(20.0)
                .selected(*c == rgb)
                .tooltip(hex(*c))
                .build(t),
            Piece {
                editor,
                kind: Kind::Custom,
            },
            CustomColorSwatch { index: i, editor },
            ContextMenuTarget,
        ));
    }
    r.spawn((
        IconButton::new(format!("{prefix}-add-custom"), "plus")
            .tooltip("Save this color")
            .build(t),
        Piece {
            editor,
            kind: Kind::AddCustom,
        },
    ));
}

fn srgb(c: [u8; 3]) -> Color {
    Color::srgb_u8(c[0], c[1], c[2])
}

fn rgba_color(c: [u8; 4]) -> Color {
    Color::srgba_u8(c[0], c[1], c[2], c[3])
}

/// "#9BC1D8".
pub fn hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// Reads "#9bc1d8", "9BC1D8" or the short "#abc".
pub fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let h = text.trim().trim_start_matches('#');
    let d: Vec<u8> = h
        .chars()
        .map(|c| c.to_digit(16).map(|d| d as u8))
        .collect::<Option<_>>()?;
    match d.len() {
        6 => Some([d[0] * 16 + d[1], d[2] * 16 + d[3], d[4] * 16 + d[5]]),
        3 => Some([d[0] * 17, d[1] * 17, d[2] * 17]),
        _ => None,
    }
}

fn percent(alpha: u8) -> String {
    format!("{}%", (alpha as f32 / 255.0 * 100.0).round() as u8)
}

/// Hue (degrees), saturation and value (0–1).
fn hsv(c: [u8; 3]) -> [f32; 3] {
    let h = Hsva::from(Srgba::rgb_u8(c[0], c[1], c[2]));
    [h.hue, h.saturation, h.value]
}

fn from_hsv(h: f32, s: f32, v: f32) -> [u8; 3] {
    let c = Srgba::from(Hsva::hsv(
        h.rem_euclid(360.0),
        s.clamp(0.0, 1.0),
        v.clamp(0.0, 1.0),
    ));
    [c.red, c.green, c.blue].map(|x| (x * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// Sets the editor's colour from a control and tells the app.
fn pick(
    commands: &mut Commands,
    q: &mut Query<&mut ColorEditorState>,
    editor: Entity,
    f: impl FnOnce(&mut [u8; 4]),
) {
    let Ok(mut st) = q.get_mut(editor) else {
        return;
    };
    let mut rgba = st.rgba;
    f(&mut rgba);
    st.rgba = rgba;
    commands.trigger(ColorEditorChange {
        entity: editor,
        rgba,
    });
}

fn on_swatch(
    a: On<Activate>,
    q: Query<(&Piece, &SwatchColor)>,
    mut states: Query<&mut ColorEditorState>,
    button: Res<LastPointerButton>,
    mut commands: Commands,
) {
    let Ok((p, c)) = q.get(a.entity) else { return };
    if !matches!(p.kind, Kind::Preset | Kind::Custom) {
        return;
    }
    // A right-click on a custom colour opens its menu; it doesn't pick the colour.
    if button.0 == bevy::picking::pointer::PointerButton::Secondary {
        return;
    }
    let s = c.0.to_srgba().to_u8_array();
    pick(&mut commands, &mut states, p.editor, |rgba| {
        rgba[..3].copy_from_slice(&s[..3])
    });
}

fn on_mixer(
    ev: On<ColorMixerChange>,
    q: Query<&Piece>,
    mut states: Query<&mut ColorEditorState>,
    mut commands: Commands,
) {
    let Ok(p) = q.get(ev.entity) else { return };
    let rgb = from_hsv(ev.hue, ev.saturation, ev.value);
    pick(&mut commands, &mut states, p.editor, |rgba| {
        rgba[..3].copy_from_slice(&rgb)
    });
}

fn on_field(
    ev: On<NumberFieldCommit>,
    q: Query<&Piece>,
    mut states: Query<&mut ColorEditorState>,
    mut focus: ResMut<bevy::input_focus::InputFocus>,
    mut commands: Commands,
) {
    let Ok(p) = q.get(ev.entity).copied() else {
        return;
    };
    // Enter ends the edit: the field lets go of the focus and its caret.
    if ev.enter {
        focus.clear();
    }
    let value = match p.kind {
        Kind::Hex => parse_hex(&ev.text).map(|c| (0..3, c)),
        Kind::Channel(k) => ev.text.trim().parse::<f64>().ok().map(|v| {
            let mut c = [0; 3];
            c[k] = v.round().clamp(0.0, 255.0) as u8;
            (k..k + 1, c)
        }),
        _ => return,
    };
    match value {
        Some((range, c)) => pick(&mut commands, &mut states, p.editor, |rgba| {
            range.for_each(|i| rgba[i] = c[i])
        }),
        // Invalid text goes back to the current value.
        None => {
            if let Ok(mut st) = states.get_mut(p.editor) {
                st.set_changed();
            }
        }
    }
}

fn on_opacity(
    ev: On<SliderChange>,
    q: Query<&Piece>,
    mut states: Query<&mut ColorEditorState>,
    mut commands: Commands,
) {
    let Ok(p) = q.get(ev.entity) else { return };
    if p.kind == Kind::Opacity {
        let a = (ev.value * 255.0).round() as u8;
        pick(&mut commands, &mut states, p.editor, |rgba| rgba[3] = a);
    }
}

fn on_button(
    a: On<Activate>,
    q: Query<&Piece>,
    states: Query<&ColorEditorState>,
    mut commands: Commands,
) {
    let Ok(p) = q.get(a.entity) else { return };
    match p.kind {
        Kind::AddCustom => {
            if let Ok(s) = states.get(p.editor) {
                commands.trigger(ColorEditorSaveCustom {
                    entity: p.editor,
                    rgb: [s.rgba[0], s.rgba[1], s.rgba[2]],
                });
            }
        }
        Kind::Default => commands.trigger(ColorEditorDefault { entity: p.editor }),
        _ => {}
    }
}

/// Rebuilds the custom colours' row when the list changes.
fn sync_custom(
    editors: Query<
        (Entity, &Name, &ColorEditorCustom, &ColorEditorState),
        Changed<ColorEditorCustom>,
    >,
    rows: Query<(Entity, &Piece)>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    for (editor, name, custom, st) in &editors {
        let Some((row, _)) = rows
            .iter()
            .find(|(_, p)| p.editor == editor && p.kind == Kind::CustomRow)
        else {
            continue;
        };
        let prefix = name.as_str().trim_end_matches("-editor").to_string();
        let (t, list, rgb) = (
            theme.clone(),
            custom.0.clone(),
            [st.rgba[0], st.rgba[1], st.rgba[2]],
        );
        commands.entity(row).despawn_children();
        commands.queue(move |world: &mut World| {
            if let Ok(mut e) = world.get_entity_mut(row) {
                e.with_children(|r| custom_row(r, &t, &prefix, editor, &list, rgb));
            }
        });
    }
}

/// Moves every control to the editor's colour when it changes.
#[allow(clippy::type_complexity)]
fn sync_controls(
    editors: Query<(Entity, &ColorEditorState), Changed<ColorEditorState>>,
    mut fields: Query<(&Piece, &mut NumberFieldState)>,
    mut mixers: Query<(&Piece, &mut ColorMixerState)>,
    mut sliders: Query<(&Piece, &mut SliderState)>,
    mut current: Query<(&Piece, &mut BackgroundColor)>,
    mut texts: Query<(&Piece, &mut Text)>,
    mut swatches: Query<(&Piece, &SwatchColor, &mut SwatchSelected)>,
) {
    for (editor, st) in &editors {
        let c = st.rgba;
        let rgb = [c[0], c[1], c[2]];
        for (p, mut f) in &mut fields {
            if p.editor != editor {
                continue;
            }
            let text = match p.kind {
                Kind::Hex => hex(rgb),
                Kind::Channel(k) => rgb[k].to_string(),
                _ => continue,
            };
            if f.text != text {
                f.text = text;
            } else {
                // Rewrite the shown text (an invalid entry goes back).
                f.set_changed();
            }
        }
        let [h, sat, v] = hsv(rgb);
        for (p, mut m) in &mut mixers {
            if p.editor != editor {
                continue;
            }
            // Keep the hue while the colour is grey (it has none of its own).
            let hue = if sat == 0.0 || v == 0.0 { m.hue } else { h };
            let close = (m.hue - hue).abs() < 0.6
                && (m.saturation - sat).abs() < 0.003
                && (m.value - v).abs() < 0.003;
            if !close {
                *m = ColorMixerState {
                    hue,
                    saturation: sat,
                    value: v,
                };
            }
        }
        for (p, mut s) in &mut sliders {
            let want = c[3] as f32 / 255.0;
            if p.editor == editor && p.kind == Kind::Opacity && (s.value - want).abs() > 0.003 {
                s.value = want;
            }
        }
        for (p, mut bg) in &mut current {
            if p.editor == editor && p.kind == Kind::Current {
                bg.0 = rgba_color(c);
            }
        }
        for (p, mut t) in &mut texts {
            if p.editor == editor && p.kind == Kind::OpacityText {
                t.0 = percent(c[3]);
            }
        }
        // The ring follows the colour.
        for (p, sc, mut sel) in &mut swatches {
            if p.editor != editor {
                continue;
            }
            let want = sc.0.to_srgba().to_u8_array()[..3] == rgb;
            if sel.0 != want {
                sel.0 = want;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_and_hsv_round_trip() {
        assert_eq!(hex([155, 193, 216]), "#9BC1D8");
        assert_eq!(parse_hex("#9bc1d8"), Some([155, 193, 216]));
        assert_eq!(parse_hex("abc"), Some([170, 187, 204]));
        assert_eq!(parse_hex("#12345"), None);
        for c in [
            [155, 193, 216],
            [255, 0, 0],
            [0, 0, 0],
            [128, 128, 128],
            [230, 126, 34],
        ] {
            let [h, s, v] = hsv(c);
            assert_eq!(from_hsv(h, s, v), c, "{c:?}");
        }
    }
}
