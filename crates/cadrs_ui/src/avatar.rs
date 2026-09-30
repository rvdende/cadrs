//! A small avatar with the user's initials, modeled on gpui-component's `Avatar` fallback.

use bevy::prelude::*;
use bevy::text::FontWeight;

use crate::theme::Theme;

/// Builder for an avatar.
pub struct Avatar {
    name: String,
    user: String,
    size: f32,
}

impl Avatar {
    /// `name` is the node's `Name`; `user` is the person's display name.
    pub fn new(name: impl Into<String>, user: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            user: user.into(),
            size: 24.0,
        }
    }

    pub fn size(mut self, s: f32) -> Self {
        self.size = s;
        self
    }

    /// Up to two initials from the display name.
    pub fn initials(user: &str) -> String {
        user.split_whitespace()
            .filter_map(|w| w.chars().next())
            .take(2)
            .flat_map(char::to_uppercase)
            .collect()
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        // A stable muted color per user.
        const COLORS: [&str; 6] = [
            "#4f6d8f", "#6b5b95", "#3f7f6e", "#8a5a44", "#5b7a3a", "#7a4f6b",
        ];
        let hash = self
            .user
            .bytes()
            .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
        let bg = Color::Srgba(Srgba::hex(COLORS[hash as usize % COLORS.len()]).unwrap());
        (
            Name::new(self.name),
            Node {
                width: Val::Px(self.size),
                height: Val::Px(self.size),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(Val::Px(theme.radius)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(bg),
            children![(
                theme.text(
                    Self::initials(&self.user),
                    (self.size * 0.42).round(),
                    FontWeight::SEMIBOLD,
                    Color::WHITE
                ),
                Pickable::IGNORE,
            )],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials() {
        assert_eq!(Avatar::initials("Rouan van der Ende"), "RV");
        assert_eq!(Avatar::initials("casper"), "C");
        assert_eq!(Avatar::initials(""), "");
    }
}
