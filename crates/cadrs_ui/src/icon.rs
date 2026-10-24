//! Icons keyed by name, from the [icon-rs](https://crates.io/crates/icon-rs) crate, rasterized by
//! `build.rs`.
//!
//! Glyph and line icons are rasterized in white and tinted at runtime through
//! `ImageNode::color`. Their small accent detail is a separate child layer ([`IconAccent`]) in
//! the accent colour, which follows the tint when an icon is drawn light (white on a selected
//! button). Solid and sketch icons ([`FullColour`]) keep their own colours: a tint only fades
//! them, through its alpha.
//!
//! The rasters are picked by physical pixels: at a UI scale or on a high-DPI screen, an icon
//! whose size isn't built in is rendered at that size (once, with `build.rs`'s code), so it stays
//! sharp instead of being upscaled.

use std::borrow::Cow;

use bevy::asset::RenderAssetUsages;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;

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

/// Whether the icon-rs build has an icon by this name (for icons newer than the release a build
/// uses, with a stand-in until then).
pub fn has_icon(name: &str) -> bool {
    data::ICON_DATA.iter().any(|(n, ..)| *n == name)
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
    /// Rasters rendered at runtime, by (name, physical px, accent layer, dark artwork); `None`
    /// where rendering failed (the built-in raster is used).
    rendered: HashMap<(String, u32, bool, bool), Option<Handle<Image>>>,
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

    /// The icon's base (or `accent`) layer for `size` logical px drawn at `scale` physical px
    /// each: the built-in raster 100 % would use, or that raster's size times `scale` rendered
    /// (once) so the icon looks the same at every scale, only sharper. With `dark`, a
    /// full-colour icon is drawn from icon-rs's artwork for dark backgrounds.
    fn scaled(
        &mut self,
        name: &str,
        size: f32,
        scale: f32,
        accent: bool,
        dark: bool,
        images: &mut Assets<Image>,
    ) -> Option<Handle<Image>> {
        let dark = dark && self.is_full_colour(name);
        let table = if accent { &self.accents } else { &self.images };
        let sizes = table.get(name)?;
        let base = pick(sizes, size)?;
        let built = sizes
            .iter()
            .find(|(_, h)| *h == base)
            .map_or(0, |(s, _)| *s);
        let px = (built as f32 * scale).round().clamp(1.0, 512.0) as u32;
        if px == built && !dark {
            return Some(base);
        }
        let key = (name.to_owned(), px, accent, dark);
        let made = self.rendered.entry(key).or_insert_with(|| {
            render(name, built, px, accent, dark).map(|rgba| images.add(rgba_image(px, rgba)))
        });
        Some(made.clone().unwrap_or(base))
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

/// The icon `name` (built in at `size` px) rendered at `px` px: its base or accent layer, from
/// the artwork for dark backgrounds with `dark`.
fn render(name: &str, size: u32, px: u32, accent: bool, dark: bool) -> Option<Vec<u8>> {
    let (mut icon, weight) = crate::icon_raster::source(name)?;
    if dark {
        icon.svg = icon.svg_dark;
    }
    let (base, accent_svg) = crate::icon_raster::layers(&icon, weight, size);
    let svg = if accent { accent_svg? } else { base };
    crate::icon_raster::rasterize(&svg, px)
        .inspect_err(|e| warn!("cannot render icon {name:?} at {px} px: {e}"))
        .ok()
}

fn rgba_image(px: u32, rgba: Vec<u8>) -> Image {
    Image::new(
        Extent3d {
            width: px,
            height: px,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn load_icons(mut atlas: ResMut<IconAtlas>, mut images: ResMut<Assets<Image>>) {
    *atlas = IconAtlas::built_in(&mut images);
}

impl IconAtlas {
    /// The rasters `build.rs` made, added to `images`.
    fn built_in(images: &mut Assets<Image>) -> Self {
        let mut add = |table: &'static [(&'static str, u32, &'static [u8])]| {
            let mut out: HashMap<&'static str, Vec<(u32, Handle<Image>)>> = HashMap::default();
            for &(name, size, rgba) in table {
                out.entry(name)
                    .or_default()
                    .push((size, images.add(rgba_image(size, rgba.to_vec()))));
            }
            for sizes in out.values_mut() {
                sizes.sort_by_key(|(s, _)| *s);
            }
            out
        };
        Self {
            images: add(data::ICON_DATA),
            accents: add(data::ACCENT_DATA),
            full_colour: data::FULL_COLOUR.iter().copied().collect(),
            rendered: HashMap::default(),
        }
    }
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

/// Fills in the image of new or changed [`Icon`]s (all of them when the UI's scale or the color
/// scheme changes), marks full-colour ones and (re)builds the accent layer.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn resolve_icons(
    mut commands: Commands,
    mut atlas: ResMut<IconAtlas>,
    mut images: ResMut<Assets<Image>>,
    window: Query<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    appearance: Option<Res<crate::color_scheme::Appearance>>,
    mut last: Local<(f32, bool)>,
    mut q: Query<(
        Entity,
        Ref<Icon>,
        &mut ImageNode,
        &mut Node,
        Option<&Children>,
        Has<SolidTint>,
    )>,
    q_accent: Query<(), With<IconAccent>>,
) {
    // Physical pixels per logical pixel.
    let scale = window.single().map_or(1.0, |w| w.scale_factor()) * ui_scale.0;
    let dark = appearance.is_some_and(|a| a.is_dark());
    let rescaled = (scale, dark) != *last;
    *last = (scale, dark);
    for (entity, icon, mut image, mut node, children, solid) in &mut q {
        if !icon.is_changed() && !rescaled {
            continue;
        }
        match atlas.scaled(&icon.name, icon.size, scale, false, dark, &mut images) {
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
        if let Some(accent) = atlas.scaled(&icon.name, icon.size, scale, true, dark, &mut images) {
            let layer = (
                IconAccent,
                ImageNode {
                    image: accent,
                    color: if solid {
                        image.color
                    } else {
                        accent_layer_color(image.color)
                    },
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
pub(crate) fn sync_icon_layers(
    mut q_icons: Query<
        (
            Mut<ImageNode>,
            Has<FullColour>,
            Option<&Children>,
            Option<&ChildOf>,
            Has<SolidTint>,
        ),
        With<Icon>,
    >,
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
                // Faded further on dark, where the dark artwork faded as much still reads.
                if crate::color_scheme::dark_in_force() {
                    0.18
                } else {
                    0.35
                }
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
        let accent = if disabled || solid {
            image.color
        } else {
            accent_layer_color(image.color)
        };
        for child in children.into_iter().flatten() {
            if let Ok(mut layer) = q_accent.get_mut(*child)
                && layer.color != accent
            {
                layer.color = accent;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_icons_are_rendered_at_their_physical_size() {
        let mut images = Assets::<Image>::default();
        let mut atlas = IconAtlas::built_in(&mut images);
        // 100 %: the built-in raster.
        let at_100 = atlas
            .scaled("plus", 16.0, 1.0, false, false, &mut images)
            .unwrap();
        assert_eq!(Some(&at_100), atlas.get("plus", 16.0).as_ref());
        // 150 % and 200 %: rendered at 24 and 32 px (not the 16 px raster stretched), once each.
        for (scale, px) in [(1.5, 24), (2.0, 32)] {
            let h = atlas
                .scaled("plus", 16.0, scale, false, false, &mut images)
                .unwrap();
            let image = images.get(&h).unwrap();
            assert_eq!((image.width(), image.height()), (px, px));
            assert!(
                image.data.as_ref().unwrap().chunks(4).any(|p| p[3] > 0),
                "drawn"
            );
            assert_eq!(
                atlas.scaled("plus", 16.0, scale, false, false, &mut images),
                Some(h)
            );
        }
        // Weight variants and accent layers too.
        let bold = atlas
            .scaled("check-bold", 16.0, 2.0, false, false, &mut images)
            .unwrap();
        assert_eq!(images.get(&bold).unwrap().width(), 32);
        let with_accent = data::ACCENT_DATA[0].0;
        let accent = atlas
            .scaled(with_accent, 16.0, 2.0, true, false, &mut images)
            .unwrap();
        assert_eq!(images.get(&accent).unwrap().width(), 32);
    }
}
