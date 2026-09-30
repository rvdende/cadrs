//! Small line pictograms for editor toolbars (alignment, lists, underline and strikethrough, table
//! rows and columns), drawn with UI nodes where the icon set has no icon. Each is 16 × 16 px,
//! like an icon, and ignores the pointer so the button under it gets the click.
//!
//! ```ignore
//! commands.spawn(Button::new("note-align-left").ghost().square().build(&theme))
//!     .with_children(|b| glyph(b, Glyph::AlignLeft, theme.foreground));
//! ```

use bevy::prelude::*;
use bevy::text::FontWeight;

use crate::theme::Theme;

/// A pictogram.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Glyph {
    AlignLeft,
    AlignCenter,
    AlignRight,
    Bullets,
    Numbered,
    /// "U" with a line under it.
    Underline,
    /// "S" struck through.
    Strike,
    RowAbove,
    RowBelow,
    ColumnLeft,
    ColumnRight,
    DeleteRow,
    DeleteColumn,
    Merge,
    Unmerge,
}

const SIZE: f32 = 16.0;

fn bar(x: f32, y: f32, w: f32, h: f32, c: Color) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(x),
            top: Val::Px(y),
            width: Val::Px(w),
            height: Val::Px(h),
            ..default()
        },
        BackgroundColor(c),
        Pickable::IGNORE,
    )
}

fn frame(x: f32, y: f32, w: f32, h: f32, c: Color) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(x),
            top: Val::Px(y),
            width: Val::Px(w),
            height: Val::Px(h),
            border: UiRect::all(Val::Px(1.2)),
            ..default()
        },
        BorderColor::all(c),
        Pickable::IGNORE,
    )
}

/// Spawns glyph `g` in `color` under `p`.
pub fn glyph(p: &mut ChildSpawnerCommands, theme: &Theme, g: Glyph, color: Color) {
    let accent = theme.primary;
    p.spawn((
        Node {
            width: Val::Px(SIZE),
            height: Val::Px(SIZE),
            flex_shrink: 0.0,
            ..default()
        },
        Pickable::IGNORE,
    ))
    .with_children(|b| {
        let t = 1.4;
        match g {
            Glyph::AlignLeft | Glyph::AlignCenter | Glyph::AlignRight => {
                for (i, w) in [14.0, 9.0, 14.0, 9.0].into_iter().enumerate() {
                    let x = match g {
                        Glyph::AlignLeft => 1.0,
                        Glyph::AlignCenter => (SIZE - w) / 2.0,
                        _ => SIZE - 1.0 - w,
                    };
                    b.spawn(bar(x, 2.5 + i as f32 * 3.4, w, t, color));
                }
            }
            Glyph::Bullets => {
                for i in 0..3 {
                    let y = 2.5 + i as f32 * 4.5;
                    b.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(1.0),
                            top: Val::Px(y - 0.3),
                            width: Val::Px(2.6),
                            height: Val::Px(2.6),
                            border_radius: BorderRadius::all(Val::Px(1.3)),
                            ..default()
                        },
                        BackgroundColor(color),
                        Pickable::IGNORE,
                    ));
                    b.spawn(bar(5.5, y + 0.2, 9.5, t, color));
                }
            }
            Glyph::Numbered => {
                for (i, n) in ["1", "2", "3"].into_iter().enumerate() {
                    let y = 0.2 + i as f32 * 5.0;
                    b.spawn((
                        theme.text(n, 6.5, FontWeight::BOLD, color),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.5),
                            top: Val::Px(y),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                    b.spawn(bar(5.5, y + 2.4, 9.5, t, color));
                }
            }
            Glyph::Underline | Glyph::Strike => {
                let letter = if g == Glyph::Underline { "U" } else { "S" };
                b.spawn((
                    theme.text(letter, 13.0, FontWeight::MEDIUM, color),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(3.5),
                        top: Val::Px(-0.5),
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
                if g == Glyph::Underline {
                    b.spawn(bar(3.0, 14.2, 10.0, 1.3, color));
                } else {
                    b.spawn(bar(1.5, 7.6, 13.0, 1.3, color));
                }
            }
            Glyph::RowAbove | Glyph::RowBelow | Glyph::DeleteRow => {
                // A 2 × 2 grid with the new (or removed) row highlighted.
                let (y_new, y_old) = if g == Glyph::RowBelow { (9.0, 2.0) } else { (2.0, 9.0) };
                b.spawn(frame(1.0, y_old, 14.0, 6.0, color));
                b.spawn(bar(8.0, y_old, 1.2, 6.0, color));
                let c = if g == Glyph::DeleteRow { Color::srgb_u8(0xd9, 0x3b, 0x30) } else { accent };
                b.spawn(frame(1.0, y_new, 14.0, 6.0, c));
                b.spawn(bar(8.0, y_new, 1.2, 6.0, c));
            }
            Glyph::ColumnLeft | Glyph::ColumnRight | Glyph::DeleteColumn => {
                let (x_new, x_old) = if g == Glyph::ColumnRight { (9.0, 2.0) } else { (2.0, 9.0) };
                b.spawn(frame(x_old, 1.0, 6.0, 14.0, color));
                b.spawn(bar(x_old, 8.0, 6.0, 1.2, color));
                let c = if g == Glyph::DeleteColumn { Color::srgb_u8(0xd9, 0x3b, 0x30) } else { accent };
                b.spawn(frame(x_new, 1.0, 6.0, 14.0, c));
                b.spawn(bar(x_new, 8.0, 6.0, 1.2, c));
            }
            Glyph::Merge => {
                b.spawn(frame(1.0, 3.0, 14.0, 10.0, color));
                // Two arrows meeting in the middle.
                b.spawn(bar(3.5, 7.4, 3.5, 1.2, accent));
                b.spawn(bar(9.0, 7.4, 3.5, 1.2, accent));
                b.spawn(bar(7.0, 5.5, 1.2, 5.0, accent));
                b.spawn(bar(7.8, 5.5, 1.2, 5.0, accent));
            }
            Glyph::Unmerge => {
                b.spawn(frame(1.0, 3.0, 14.0, 10.0, color));
                b.spawn(bar(7.4, 3.0, 1.2, 10.0, accent));
            }
        }
    });
}

/// A bold thumbtack (a pinned reference, ER5.1; icon-rs has none yet), drawn with UI nodes in a
/// `size` × `size` box: the cap, the body and the collar, and the needle, tilted 40° with the
/// needle to the lower left. Spawn it as a child: `p.spawn(thumbtack(14.0, color))`.
pub fn thumbtack(size: f32, color: Color) -> impl Bundle {
    let k = size / 14.0;
    let px = move |v: f32| Val::Px(v * k);
    let part = move |w: f32, h: f32, r: f32| {
        (
            Node { width: px(w), height: px(h), flex_shrink: 0.0, border_radius: BorderRadius::all(px(r)), ..default() },
            BackgroundColor(color),
            Pickable::IGNORE,
        )
    };
    (
        Node { width: Val::Px(size), height: Val::Px(size), flex_shrink: 0.0, justify_content: JustifyContent::Center, ..default() },
        Pickable::IGNORE,
        children![(
            Node { width: px(9.0), height: px(14.0), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() },
            bevy::ui::UiTransform { rotation: Rot2::degrees(40.0), ..default() },
            Pickable::IGNORE,
            children![part(6.5, 2.4, 1.0), part(4.2, 4.2, 0.6), part(9.0, 2.4, 1.0), part(1.7, 5.0, 0.8)],
        )],
    )
}

/// A bold down arrow (a stem and a filled head) in a `size` × `size` box: the "update further
/// down the chain" badge (ER4.2), which icon-rs's thin `arrow-down` doesn't carry at badge size.
pub fn down_arrow(size: f32, color: Color) -> impl Bundle {
    let k = size / 10.0;
    (
        Node { width: Val::Px(size), height: Val::Px(size), flex_shrink: 0.0, flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() },
        Pickable::IGNORE,
        children![
            (Node { width: Val::Px(2.0 * k), height: Val::Px(4.5 * k), flex_shrink: 0.0, ..default() }, BackgroundColor(color), Pickable::IGNORE),
            (
                Node { margin: UiRect::top(Val::Px(-2.5 * k)), flex_shrink: 0.0, ..default() },
                Pickable::IGNORE,
                children![(crate::icon::icon("caret-down-filled", 8.0 * k, color), crate::icon::SolidTint, Pickable::IGNORE)],
            ),
        ],
    )
}
