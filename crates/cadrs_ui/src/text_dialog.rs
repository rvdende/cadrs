//! The sketch text dialog (S16.1, Onshape's "Text" dialog,
//! `reference/onshape/t5/textrectangle-dialog-01.png`): a [`FeatureDialog`] titled "Text" with a
//! font dropdown and toggle buttons for bold, italic and the two flips on one row, a grey preview
//! box showing the text in its style, the text field (autofocused) and a "N/250 characters"
//! counter.
//!
//! The state lives in [`TextDialogState`] on the dialog; the app reads it every frame for its
//! live preview in the sketch and on ✓ ([`crate::FeatureDialogAccept`]) or Enter
//! ([`crate::TextSubmit`], bubbling up to the dialog).
//!
//! Names: the dialog `<name>`, its parts `<name>-font` (the select), `<name>-bold`,
//! `<name>-italic`, `<name>-flip-h`, `<name>-flip-v`, `<name>-preview`, `<name>-input` (and
//! `<name>-input-field`) and `<name>-count`.

use std::borrow::Cow;

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, FontStyle, FontWeight};
use bevy::ui::UiTransform;
use bevy::ui_widgets::Activate;

use crate::button::IconButton;
use crate::dialog_fields::{Select, SelectChange};
use crate::feature_dialog::FeatureDialog;
use crate::input::{TextInput, TextInputField};
use crate::style::Selected;
use crate::theme::Theme;

pub struct TextDialogPlugin;

impl Plugin for TextDialogPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_toggle)
            .add_observer(on_font)
            .add_systems(
                PostUpdate,
                (resolve_parts, read_text, sync_parts)
                    .chain()
                    .before(bevy::ui::UiSystems::Prepare),
            );
    }
}

/// Onshape's limit on a text box's characters.
pub const MAX_TEXT_CHARS: usize = 250;

/// What the dialog holds.
#[derive(Component, Debug, Clone, PartialEq, Eq, Default)]
pub struct TextDialogState {
    pub text: String,
    /// Index into the font list.
    pub font: usize,
    /// The fonts' names (the preview uses the UI's Inter at the matching weight).
    pub fonts: Vec<String>,
    /// The weight of each font (bold makes it heavier).
    pub weights: Vec<u16>,
    pub bold: bool,
    pub italic: bool,
    /// Flipped left to right.
    pub flip_h: bool,
    /// Flipped upside down.
    pub flip_v: bool,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    Bold,
    Italic,
    FlipH,
    FlipV,
    Preview,
    PreviewText,
    Count,
    Field,
    Font,
}

/// Which dialog a part belongs to.
#[derive(Component, Debug, Clone, Copy)]
struct Of(Entity, Part);

/// Builder for the text dialog.
pub struct TextDialog {
    name: Cow<'static, str>,
    state: TextDialogState,
}

impl TextDialog {
    /// `fonts`: (name, CSS-like weight) of each font on offer.
    pub fn new(name: impl Into<Cow<'static, str>>, fonts: &[(&str, u16)]) -> Self {
        Self {
            name: name.into(),
            state: TextDialogState {
                fonts: fonts.iter().map(|f| f.0.to_string()).collect(),
                weights: fonts.iter().map(|f| f.1).collect(),
                ..TextDialogState::default()
            },
        }
    }

    /// The starting text and style (Edit text opens with the entity's).
    pub fn state(mut self, text: impl Into<String>, font: usize, bold: bool, italic: bool, flip_h: bool, flip_v: bool) -> Self {
        self.state.text = text.into();
        self.state.font = font;
        self.state.bold = bold;
        self.state.italic = italic;
        self.state.flip_h = flip_h;
        self.state.flip_v = flip_v;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let name = self.name.to_string();
        let st = self.state.clone();
        let dialog = FeatureDialog::new(self.name.clone())
            .title("Text")
            .width(250.0)
            .body(move |p: &mut ChildSpawner| {
                let root = p.target_entity();
                // The row: font, B, I, flip left–right, flip upside down.
                p.spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(2.0),
                    padding: UiRect::new(Val::Px(4.0), Val::Px(2.0), Val::Px(2.0), Val::Px(4.0)),
                    ..default()
                })
                .with_children(|r| {
                    let mut select = Select::new(format!("{name}-font")).width(Val::Px(118.0));
                    for f in &st.fonts {
                        select = select.option(f.clone(), true);
                    }
                    r.spawn((select.selected(st.font).build(&t), Of(root, Part::Font)));
                    let toggles = [
                        (Part::Bold, "bold", "bold", "Bold", st.bold),
                        (Part::Italic, "italic", "italic", "Italic", st.italic),
                        (Part::FlipH, "flip-h", "flip-horizontal", "Flip left to right", st.flip_h),
                        (Part::FlipV, "flip-v", "mirror", "Flip upside down", st.flip_v),
                    ];
                    for (part, suffix, icon_name, tip, on) in toggles {
                        r.spawn((
                            IconButton::new(format!("{name}-{suffix}"), icon_name)
                                .icon_size(16.0)
                                .selected(on)
                                .tooltip(tip)
                                .build(&t),
                            Of(root, part),
                        ));
                    }
                });
                // The preview: the text in its style on grey.
                p.spawn((
                    Name::new(format!("{name}-preview")),
                    Of(root, Part::Preview),
                    Node {
                        height: Val::Px(40.0),
                        margin: UiRect::new(Val::Px(4.0), Val::Px(4.0), Val::Px(2.0), Val::Px(4.0)),
                        padding: UiRect::horizontal(Val::Px(6.0)),
                        align_items: AlignItems::Center,
                        overflow: Overflow::clip(),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb_u8(0xee, 0xee, 0xee)),
                    BorderColor::all(Color::srgb_u8(0xdd, 0xdd, 0xdd)),
                ))
                .with_children(|pv| {
                    pv.spawn((
                        Of(root, Part::PreviewText),
                        t.text(st.text.clone(), 20.0, FontWeight::NORMAL, Color::srgb_u8(0x6b, 0x6b, 0x6b)),
                        UiTransform::default(),
                        Pickable::IGNORE,
                    ));
                });
                p.spawn(Node {
                    padding: UiRect::horizontal(Val::Px(4.0)),
                    ..default()
                })
                .with_children(|r| {
                    r.spawn((
                        TextInput::new(format!("{name}-input"))
                            .value(st.text.clone())
                            .height(30.0)
                            .max_characters(MAX_TEXT_CHARS)
                            .autofocus()
                            .build(&t),
                        Of(root, Part::Field),
                    ));
                });
                p.spawn((
                    Name::new(format!("{name}-count")),
                    Of(root, Part::Count),
                    t.text(
                        format!("{}/{MAX_TEXT_CHARS} characters", st.text.chars().count()),
                        t.font_sm,
                        FontWeight::NORMAL,
                        Color::srgb_u8(0x6b, 0x6b, 0x6b),
                    ),
                    Node {
                        margin: UiRect::new(Val::Px(5.0), Val::ZERO, Val::Px(5.0), Val::Px(6.0)),
                        ..default()
                    },
                ));
            })
            .build(theme);
        (dialog, self.state)
    }
}

/// Parts are spawned inside the dialog's body: point them at the dialog itself (the entity with
/// the state).
fn resolve_parts(
    mut q: Query<&mut Of, Added<Of>>,
    q_parent: Query<&ChildOf>,
    q_state: Query<(), With<TextDialogState>>,
) {
    for mut o in &mut q {
        let mut cur = o.0;
        for _ in 0..8 {
            if q_state.contains(cur) {
                if o.0 != cur {
                    o.0 = cur;
                }
                break;
            }
            let Ok(p) = q_parent.get(cur) else { break };
            cur = p.parent();
        }
    }
}

fn on_toggle(
    a: On<Activate>,
    q_of: Query<&Of>,
    q_fields: Query<(Entity, &ChildOf), With<TextInputField>>,
    mut focus: ResMut<InputFocus>,
    mut q: Query<&mut TextDialogState>,
) {
    let Ok(o) = q_of.get(a.entity) else { return };
    // Typing (and Enter) stay with the text field.
    if let Some((field, _)) = q_fields
        .iter()
        .find(|(_, p)| q_of.get(p.parent()).is_ok_and(|x| x.0 == o.0 && x.1 == Part::Field))
    {
        focus.set(field, FocusCause::Pressed);
    }
    let Ok(mut s) = q.get_mut(o.0) else { return };
    match o.1 {
        Part::Bold => s.bold = !s.bold,
        Part::Italic => s.italic = !s.italic,
        Part::FlipH => s.flip_h = !s.flip_h,
        Part::FlipV => s.flip_v = !s.flip_v,
        _ => {}
    }
}

fn on_font(ev: On<SelectChange>, q_of: Query<&Of>, mut q: Query<&mut TextDialogState>) {
    let Ok(o) = q_of.get(ev.entity) else { return };
    if o.1 != Part::Font {
        return;
    }
    if let Ok(mut s) = q.get_mut(o.0)
        && s.font != ev.index
    {
        s.font = ev.index;
    }
}

/// The text field's value goes into the state.
#[allow(clippy::type_complexity)]
fn read_text(
    q_fields: Query<(&EditableText, &ChildOf), (With<TextInputField>, Changed<EditableText>)>,
    q_of: Query<&Of>,
    mut q: Query<&mut TextDialogState>,
) {
    for (text, parent) in &q_fields {
        // The field's frame is the dialog's input part.
        let Ok(o) = q_of.get(parent.parent()) else { continue };
        if o.1 != Part::Field {
            continue;
        }
        let v = text.value().to_string();
        if let Ok(mut s) = q.get_mut(o.0)
            && s.text != v
        {
            s.text = v;
        }
    }
}

/// Toggle buttons, preview and counter follow the state.
#[allow(clippy::type_complexity)]
fn sync_parts(
    q: Query<(Entity, &TextDialogState), Changed<TextDialogState>>,
    q_parts: Query<(Entity, &Of, Has<Selected>)>,
    mut q_text: Query<(&mut Text, &mut TextFont, &mut UiTransform)>,
    mut commands: Commands,
) {
    for (dialog, s) in &q {
        for (e, o, selected) in &q_parts {
            if o.0 != dialog {
                continue;
            }
            let on = match o.1 {
                Part::Bold => Some(s.bold),
                Part::Italic => Some(s.italic),
                Part::FlipH => Some(s.flip_h),
                Part::FlipV => Some(s.flip_v),
                _ => None,
            };
            if let Some(on) = on {
                if on && !selected {
                    commands.entity(e).insert(Selected);
                } else if !on && selected {
                    commands.entity(e).remove::<Selected>();
                }
                continue;
            }
            let Ok((mut text, mut font, mut tr)) = q_text.get_mut(e) else {
                continue;
            };
            match o.1 {
                Part::PreviewText => {
                    let shown = s.text.lines().next().unwrap_or("").to_string();
                    if text.0 != shown {
                        text.0 = shown;
                    }
                    let base = s.weights.get(s.font).copied().unwrap_or(400);
                    let w = if s.bold { (base + 400).min(900) } else { base };
                    font.weight = FontWeight(w);
                    font.style = if s.italic { FontStyle::Italic } else { FontStyle::Normal };
                    let scale = Vec2::new(
                        if s.flip_h { -1.0 } else { 1.0 },
                        if s.flip_v { -1.0 } else { 1.0 },
                    );
                    if tr.scale != scale {
                        tr.scale = scale;
                    }
                }
                Part::Count => {
                    let want = format!("{}/{MAX_TEXT_CHARS} characters", s.text.chars().count());
                    if text.0 != want {
                        text.0 = want;
                    }
                }
                _ => {}
            }
        }
    }
}
