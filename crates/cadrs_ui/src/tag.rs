//! A small tag (pill) with a short label, modeled on gpui-component's `Tag`: a muted rounded
//! background, small medium-weight text, an optional outline. Used on list rows for a short
//! marker, e.g. the variable that suppresses a feature (`#withHole`).

use bevy::prelude::*;
use bevy::text::FontWeight;

use crate::theme::Theme;

/// Builder for a tag.
pub struct Tag {
    name: String,
    label: String,
    color: Option<Color>,
    outline: bool,
}

impl Tag {
    /// `name` is the node's `Name`.
    pub fn new(name: impl Into<String>, label: impl Into<String>) -> Self {
        Self { name: name.into(), label: label.into(), color: None, outline: false }
    }

    /// The text (and outline) colour; the theme's muted foreground by default.
    pub fn color(mut self, c: Color) -> Self {
        self.color = Some(c);
        self
    }

    /// A 1 px border in the text colour instead of a filled background.
    pub fn outline(mut self) -> Self {
        self.outline = true;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let fg = self.color.unwrap_or(theme.muted_foreground);
        (
            Name::new(self.name),
            Node {
                height: Val::Px(16.0),
                padding: UiRect::horizontal(Val::Px(5.0)),
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(if self.outline { 1.0 } else { 0.0 })),
                border_radius: BorderRadius::all(Val::Px(theme.radius_sm)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(if self.outline { Color::NONE } else { theme.secondary }),
            BorderColor::all(fg),
            Pickable::IGNORE,
            children![(theme.text(self.label, theme.font_xs, FontWeight::MEDIUM, fg), Pickable::IGNORE)],
        )
    }
}
