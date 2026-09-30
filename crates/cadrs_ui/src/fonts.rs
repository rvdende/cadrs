//! Embeds the Inter font (SIL OFL 1.1, see `assets/fonts/OFL.txt`) and registers it, with
//! cadrs's own small symbols font for hole callouts.
//!
//! The faces are embedded rather than loaded through the asset server so text renders on the
//! very first frame, which keeps headless screenshots deterministic. Inter Regular also replaces
//! Bevy's default font, so any text spawned without an explicit font still uses Inter.

use bevy::asset::AssetId;
use bevy::prelude::*;

const FACES: [&[u8]; 10] = [
    include_bytes!("../../../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../../assets/fonts/Inter-Bold.ttf"),
    // Bevy blends glyph coverage in linear space, so text draws lighter than in a browser;
    // the heavier faces let bold text match the references. ink.
    include_bytes!("../../../assets/fonts/Inter-ExtraBold.ttf"),
    include_bytes!("../../../assets/fonts/Inter-Black.ttf"),
    // Latin subset of Inter Italic (from Fontsource), for placeholders.
    include_bytes!("../../../assets/fonts/Inter-Italic.ttf"),
    // Heavy italics (Inter 4.1), for the Text dialog's bold + italic preview: without them
    // italic falls back to the regular-weight italic face and drops the bold.
    include_bytes!("../../../assets/fonts/Inter-ExtraBoldItalic.ttf"),
    include_bytes!("../../../assets/fonts/Inter-BlackItalic.ttf"),
    // cadrs's own hole-callout symbols Inter lacks (⌴ counterbore, ⌵ countersink, ↧ depth;
    // `tools/make_symbols_font.py`, P3.6): the text shaper falls back to them.
    include_bytes!("../../../assets/fonts/cadrs-symbols.ttf"),
];

pub struct FontsPlugin;

impl Plugin for FontsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, register_fonts);
    }
}

/// Keeps the embedded font handles alive.
#[derive(Resource)]
pub struct InterFonts(pub Vec<Handle<Font>>);

fn register_fonts(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let handles = FACES
        .iter()
        .map(|bytes| fonts.add(Font::from_bytes(bytes.to_vec())))
        .collect();
    let _ = fonts.insert(AssetId::default(), Font::from_bytes(FACES[0].to_vec()));
    commands.insert_resource(InterFonts(handles));
}
