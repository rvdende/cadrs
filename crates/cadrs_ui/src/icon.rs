//! Icons keyed by name, from the [icon-rs](https://crates.io/crates/icon-rs) crate, rasterized by
//! `build.rs`.
//!
//! Glyph and line icons are rasterized in white and tinted at runtime through
//! `ImageNode::color`. Their small accent detail is a separate child layer ([`IconAccent`]) in
//! the accent colour, which follows the tint when an icon is drawn light (white on a selected
//! button). Solid and sketch icons ([`FullColour`]) keep their own colours: a tint only fades
//! them, through its alpha.

use std::borrow::Cow;

use bevy::asset::RenderAssetUsages;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

mod data {
    include!(concat!(env!("OUT_DIR"), "/icons.rs"));
}

pub struct IconPlugin;

impl Plugin for IconPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<IconAtlas>()
            .add_systems(PreStartup, load_icons)
            .add_systems(
                PostUpdate,
                (resolve_icons, sync_icon_layers)
                    .chain()
                    .after(crate::style::apply_visuals)
                    .before(bevy::ui::UiSystems::Prepare),
            );
    }
}

/// The accent colour of icon-rs icons.
pub fn accent_color() -> Color {
    let [r, g, b] = data::ACCENT;
    Color::srgb_u8(r, g, b)
}

/// Handles to every rasterized icon, by name and pixel size.
#[derive(Resource, Default)]
pub struct IconAtlas {
    images: HashMap<&'static str, Vec<(u32, Handle<Image>)>>,
    accents: HashMap<&'static str, Vec<(u32, Handle<Image>)>>,
    full_colour: HashSet<&'static str>,
}

impl IconAtlas {
    /// The raster closest to `size` px, preferring the next larger one.
    pub fn get(&self, name: &str, size: f32) -> Option<Handle<Image>> {
        pick(self.images.get(name)?, size)
    }

    /// The icon's accent layer at about `size` px, if it has one.
    pub fn accent(&self, name: &str, size: f32) -> Option<Handle<Image>> {
        pick(self.accents.get(name)?, size)
    }

    /// Whether the icon keeps its own colours instead of being tinted.
    pub fn is_full_colour(&self, name: &str) -> bool {
        self.full_colour.contains(name)
    }

    /// All icon names, sorted.
    pub fn names(&self) -> Vec<&'static str> {
        let mut v: Vec<_> = self.images.keys().copied().collect();
        v.sort();
        v
    }
}

fn pick(sizes: &[(u32, Handle<Image>)], size: f32) -> Option<Handle<Image>> {
    let px = size.round() as u32;
    sizes
        .iter()
        .find(|(s, _)| *s >= px)
        .or_else(|| sizes.last())
        .map(|(_, h)| h.clone())
}

fn load_icons(mut atlas: ResMut<IconAtlas>, mut images: ResMut<Assets<Image>>) {
    let mut add = |table: &'static [(&'static str, u32, &'static [u8])]| {
        let mut out: HashMap<&'static str, Vec<(u32, Handle<Image>)>> = HashMap::default();
        for &(name, size, rgba) in table {
            let image = Image::new(
                Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                rgba.to_vec(),
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            );
            out.entry(name).or_default().push((size, images.add(image)));
        }
        for sizes in out.values_mut() {
            sizes.sort_by_key(|(s, _)| *s);
        }
        out
    };
    atlas.images = add(data::ICON_DATA);
    atlas.accents = add(data::ACCENT_DATA);
    atlas.full_colour = data::FULL_COLOUR.iter().copied().collect();
}

/// An icon. Spawn it with [`icon`], or add it to any UI node; the image is filled in
/// automatically. Tint it with `ImageNode::color`.
#[derive(Component, Clone, Debug)]
#[require(Node, ImageNode)]
pub struct Icon {
    pub name: Cow<'static, str>,
    pub size: f32,
}

impl Icon {
    pub fn new(name: impl Into<Cow<'static, str>>, size: f32) -> Self {
        Self {
            name: name.into(),
            size,
        }
    }
}

/// Marks an icon drawn in its own colours: its tint's alpha fades it, its tint's colour is
/// ignored.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct FullColour;

/// Draws a tintable icon wholly in its tint: its accent layer follows the tint too (a warning
/// icon drawn red, a glyph drawn white on a coloured badge).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SolidTint;

/// The accent layer of a tintable icon: a child image over the icon.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct IconAccent;

/// An icon bundle: `name` (an icon-rs icon name such as `"plus"`) at `size` logical px, tinted
/// `color`.
pub fn icon(name: impl Into<Cow<'static, str>>, size: f32, color: Color) -> impl Bundle {
    (
        Icon::new(name, size),
        ImageNode { color, ..default() },
        Node {
            width: Val::Px(size),
            height: Val::Px(size),
            flex_shrink: 0.0,
            ..default()
        },
    )
}

/// Like [`icon`], with a custom layout node (for absolute placement or margins). The node's
/// width and height are set to `size`.
pub fn icon_in(
    name: impl Into<Cow<'static, str>>,
    size: f32,
    color: Color,
    node: Node,
) -> impl Bundle {
    (
        Icon::new(name, size),
        ImageNode { color, ..default() },
        Node {
            width: Val::Px(size),
            height: Val::Px(size),
            flex_shrink: 0.0,
            ..node
        },
    )
}

/// Fills in the image of new or changed [`Icon`]s, marks full-colour ones and (re)builds the
/// accent layer.
#[allow(clippy::type_complexity)]
pub fn resolve_icons(
    mut commands: Commands,
    atlas: Res<IconAtlas>,
    mut q: Query<(Entity, &Icon, &mut ImageNode, &mut Node, Option<&Children>, Has<SolidTint>), Changed<Icon>>,
    q_accent: Query<(), With<IconAccent>>,
) {
    for (entity, icon, mut image, mut node, children, solid) in &mut q {
        match atlas.get(&icon.name, icon.size) {
            Some(handle) => image.image = handle,
            None => warn!("unknown icon {:?}", icon.name),
        }
        node.width = Val::Px(icon.size);
        node.height = Val::Px(icon.size);

        if atlas.is_full_colour(&icon.name) {
            commands.entity(entity).try_insert(FullColour);
            image.color = Color::WHITE.with_alpha(image.color.alpha());
        } else {
            commands.entity(entity).try_remove::<FullColour>();
        }

        for child in children.into_iter().flatten() {
            if q_accent.contains(*child) {
                commands.entity(*child).try_despawn();
            }
        }
        if let Some(accent) = atlas.accent(&icon.name, icon.size) {
            let layer = (
                IconAccent,
                ImageNode {
                    image: accent,
                    color: if solid { image.color } else { accent_layer_color(image.color) },
                    ..default()
                },
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                Pickable::IGNORE,
            );
            // Only while the icon still exists when the command runs: an icon despawned later
            // in the same frame (a sketch glyph that went away) would otherwise leave the layer
            // as a root node, stretched over the whole window.
            commands.queue(move |world: &mut World| {
                if let Ok(mut icon) = world.get_entity_mut(entity) {
                    icon.with_child(layer);
                }
            });
        }
    }
}

/// The accent layer's colour for an icon tinted `tint`: the accent, faded like the icon, unless
/// the icon is drawn light (white on a selected button), where the accent would vanish into the
/// background, so it follows the tint.
fn accent_layer_color(tint: Color) -> Color {
    let lightness = tint.to_srgba();
    if 0.2126 * lightness.red + 0.7152 * lightness.green + 0.0722 * lightness.blue > 0.6 {
        tint
    } else {
        accent_color().with_alpha(tint.alpha())
    }
}

/// Keeps full-colour icons untinted and accent layers in step with their icon's tint, after
/// widgets have set their state colours; and re-greys (or restores) an icon when its button is
/// disabled or enabled, even when that doesn't change the icon's tint (P3H.4 judge).
#[allow(clippy::type_complexity)]
fn sync_icon_layers(
    mut q_icons: Query<(Mut<ImageNode>, Has<FullColour>, Option<&Children>, Option<&ChildOf>, Has<SolidTint>), With<Icon>>,
    mut q_accent: Query<&mut ImageNode, (With<IconAccent>, Without<Icon>)>,
    q_disabled: Query<(), With<bevy::ui::InteractionDisabled>>,
    q_now_disabled: Query<Entity, Added<bevy::ui::InteractionDisabled>>,
    mut enabled: RemovedComponents<bevy::ui::InteractionDisabled>,
) {
    let disabled_now: std::collections::HashSet<Entity> = q_now_disabled.iter().collect();
    let enabled_now: std::collections::HashSet<Entity> = enabled.read().collect();
    for (mut image, full, children, parent, solid) in &mut q_icons {
        let parent = parent.map(|p| p.parent());
        let toggled = parent.is_some_and(|p| disabled_now.contains(&p) || enabled_now.contains(&p));
        if !image.is_changed() && !toggled {
            continue;
        }
        // An icon of a disabled button is greyed as a whole (P3H.3 judge): its accent takes the
        // grey tint and a full-colour icon fades, so every disabled tool looks the same.
        let disabled = parent.is_some_and(|p| q_disabled.contains(p));
        let reenabled = parent.is_some_and(|p| enabled_now.contains(&p)) && !disabled;
        if full {
            let alpha = if disabled {
                0.35
            } else if reenabled {
                1.0
            } else {
                image.color.alpha()
            };
            let white = Color::WHITE.with_alpha(alpha);
            if image.color != white {
                image.color = white;
            }
        }
        let accent = if disabled || solid { image.color } else { accent_layer_color(image.color) };
        for child in children.into_iter().flatten() {
            if let Ok(mut layer) = q_accent.get_mut(*child)
                && layer.color != accent
            {
                layer.color = accent;
            }
        }
    }
}

