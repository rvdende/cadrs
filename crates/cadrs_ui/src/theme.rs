//! Theme tokens (colors, spacing, sizes, fonts), modeled on gpui-component's `ThemeColor` and
//! measured from the Onshape reference captures in `reference/onshape/screens/`.
//!
//! Measurements (Chrome at 1148×1059, device scale 1):
//! - Top bar `#eaeaea`, with a `#e0e0e0` bottom border. Bottom tab bar `#d7d7d7`.
//! - Primary ("Create", "Share", "Create public document") `#1655ad`.
//! - Secondary ("Cancel") `#dadada`. Text `#232323`–`#333333`; muted text `#4a4a4a`–`#6b6b6b`;
//!   the dialog ✕ `#959595`.
//! - Borders: menu `#d7d7d7`, input `#cfcfcf`, list header `#d5d5d5`, list rows `#f1f1f1`,
//!   dialog separators `#e4e4e4`. Focused input border `#7ba5cb`. Selection `#3a65d8`.
//! - Modal backdrop: black at 50% (white turns `#7f7f7f`).
//! - List hover / section header `#f1f4f9`. Active filter: blue bar `#2750a2`, text `#1655ad`.
//! - Toast `#ddf1fc` with a `#c3d6e0` border. Tooltips `#d3dae4` with `#333` text.
//! - Feature dialog: ✓ `#009600` (disabled `#cfe8cf`), ✕ `#c42b3c`, invalid title `#a3202e`;
//!   selection field waiting `#def1ff`; checkbox checked `#28549d`; feature being edited in the
//!   list `#b1ddf8`.
//! - Info box `#ddf1f8`. Planes: fill `#e8ecf5`, edge `#bbc9d4`, label `#4a6fb0`.
//! - Document shell: icon rail `#fafafa` (36 px), panel borders `#e0e0e0`, tools `#333333`
//!   (disabled `#999999`), inactive tabs `#eaeaea`, tree guide `#dbdbdb`, rollback bar `#bababa`.
//! - Sizes: top bar 36 px, toolbar 34 px, tab bar 29 px, list rows 38 px, menu items 27 px,
//!   Create button 32 px tall, dialog buttons 30 px, text inputs 30 px. UI text 13 px, sidebar
//!   14 px, dialog title 16 px semibold.

use std::time::Duration;

use bevy::prelude::*;
use bevy::text::{FontSize, FontSource, FontWeight};

/// The font family used everywhere. The TTFs are embedded by [`crate::fonts`].
pub const FONT_FAMILY: &str = "Inter";

fn hex(s: &str) -> Color {
    Color::Srgba(Srgba::hex(s).expect("valid hex color"))
}

/// What a color paints, which decides its dark counterpart ([`crate::color_scheme`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorRole {
    /// Backgrounds of panels, rows and buttons.
    Fill,
    /// Backgrounds of surfaces above the page: dialogs, floating panels.
    Raised,
    /// Backgrounds of surfaces above everything: menus, popups, toasts.
    Floating,
    /// Text and icons.
    Ink,
    /// Borders, outlines and separators.
    Line,
}

/// All design tokens. Insert a modified copy as a resource to restyle the app.
#[derive(Resource, Clone, Debug)]
pub struct Theme {
    // Surfaces
    pub background: Color,
    pub title_bar: Color,
    pub tab_bar: Color,
    pub sidebar: Color,
    pub viewport_background: Color,
    pub popover: Color,
    /// Menus and popups, over dialogs and the 3D view alike.
    pub floating: Color,
    pub overlay: Color,
    pub shadow: Color,

    // Text
    pub foreground: Color,
    pub muted_foreground: Color,
    pub subtle_foreground: Color,
    pub disabled_foreground: Color,
    pub link: Color,

    // Borders
    pub border: Color,
    pub border_strong: Color,
    pub separator: Color,
    pub row_separator: Color,
    pub focus_ring: Color,

    // Buttons
    pub primary: Color,
    pub primary_hover: Color,
    /// A primary button whose menu is open.
    pub primary_open: Color,
    pub primary_open_border: Color,
    pub primary_active: Color,
    pub primary_disabled: Color,
    pub primary_foreground: Color,
    /// A disabled primary button's label: white to the eye in light, but its own token so the
    /// dark theme can dim it.
    pub primary_disabled_foreground: Color,
    pub secondary: Color,
    pub secondary_hover: Color,
    pub secondary_active: Color,
    pub secondary_disabled: Color,
    pub ghost_hover: Color,
    pub ghost_active: Color,

    // Lists and menus
    pub list_hover: Color,
    pub list_active: Color,
    pub list_selected: Color,
    pub list_selected_bar: Color,
    pub menu_hover: Color,
    /// The current point of a history timeline.
    pub timeline_selected: Color,

    // Inputs
    pub input_background: Color,
    pub input_disabled_background: Color,
    pub input_border: Color,
    pub input_border_focus: Color,
    pub caret: Color,
    pub selection: Color,
    pub selection_foreground: Color,

    // Status
    pub info_background: Color,
    pub info_border: Color,
    pub warning_background: Color,
    pub danger: Color,
    pub success: Color,
    pub tooltip_background: Color,
    pub tooltip_foreground: Color,
    /// Toasts: Onshape's light-blue message pill.
    pub toast_background: Color,
    pub toast_border: Color,
    pub toast_foreground: Color,
    pub toast_icon: Color,
    /// The warning banner (`screens/15`).
    pub warning_banner_background: Color,
    pub warning_banner_border: Color,
    pub warning_icon: Color,

    // Feature dialogs and form controls
    /// A feature dialog's title while the feature is invalid, and invalid features in the list.
    pub feature_error: Color,
    /// The feature being edited in the feature list.
    pub feature_editing: Color,
    /// The ✓ (accept) button, and its disabled look.
    pub accept: Color,
    pub accept_disabled: Color,
    /// The ✕ (cancel) glyph.
    pub cancel: Color,
    pub checkbox_border: Color,
    pub checkbox_checked: Color,
    /// A checkbox that doesn't apply (its fill and border).
    pub checkbox_unavailable: Color,
    /// A selection field waiting for input.
    pub selection_field_active: Color,
    pub selection_field_active_border: Color,

    // Document shell
    pub rail_background: Color,
    pub panel_border: Color,
    pub tool_foreground: Color,
    pub tool_disabled_foreground: Color,
    pub toolbar_separator: Color,
    pub kbd_border: Color,
    pub search_background: Color,
    pub tab_inactive: Color,
    pub tab_hover: Color,
    pub tab_underline: Color,
    pub feature_icon: Color,
    pub tree_guide: Color,
    pub rollback_bar: Color,

    // Viewport
    pub plane_fill: Color,
    pub plane_edge: Color,
    pub plane_label: Color,
    /// Hovered (pre-selected) geometry: planes get an orange outline.
    pub highlight: Color,
    /// Selected geometry.
    pub selection_3d: Color,
    pub axis_x: Color,
    pub axis_y: Color,
    pub axis_z: Color,

    // Typography (logical px)
    pub font_xs: f32,
    pub font_sm: f32,
    pub font_base: f32,
    pub font_md: f32,
    pub font_lg: f32,
    pub font_xl: f32,

    // Spacing scale (logical px): 0, 2, 4, 6, 8, 12, 16, 24, 32
    pub space: [f32; 9],
    pub radius_sm: f32,
    pub radius: f32,
    pub radius_lg: f32,

    // Component sizes (logical px)
    pub button_height_sm: f32,
    pub button_height: f32,
    pub button_height_lg: f32,
    pub input_height: f32,
    pub list_row_height: f32,
    pub menu_item_height: f32,
    pub icon_size: f32,
    pub top_bar_height: f32,
    pub toolbar_height: f32,
    pub tab_bar_height: f32,
    pub sidebar_width: f32,

    // Behavior
    pub tooltip_delay: Duration,
    pub caret_blink_period: Duration,
    pub fade_duration: Duration,
}

impl Default for Theme {
    fn default() -> Self {
        Self::light()
    }
}

impl Theme {
    /// The Onshape-like light theme.
    pub fn light() -> Self {
        Self {
            background: hex("#ffffff"),
            title_bar: hex("#eaeaea"),
            tab_bar: hex("#d7d7d7"),
            sidebar: hex("#ffffff"),
            viewport_background: hex("#ffffff"),
            popover: hex("#ffffff"),
            floating: hex("#ffffff"),
            // UI blends in linear space: 0.79 alpha dims white to about #7f7f7f, like the browser's 50%.
            overlay: Color::srgba(0.0, 0.0, 0.0, 0.79),
            shadow: Color::srgba(0.0, 0.0, 0.0, 0.18),

            foreground: hex("#212121"),
            muted_foreground: hex("#5e5e5e"),
            subtle_foreground: hex("#959595"),
            disabled_foreground: hex("#b4b4b4"),
            link: hex("#1655ad"),

            border: hex("#d7d7d7"),
            border_strong: hex("#c4c4c4"),
            separator: hex("#e4e4e4"),
            row_separator: hex("#f1f1f1"),
            focus_ring: hex("#7ba5cb"),

            primary: hex("#1655ad"),
            primary_hover: hex("#2d63c0"),
            primary_open: hex("#5b8fd6"),
            primary_open_border: hex("#9fc0ee"),
            primary_active: hex("#0f4491"),
            primary_disabled: hex("#9bb6dc"),
            primary_foreground: hex("#ffffff"),
            primary_disabled_foreground: hex("#f8fafd"),
            secondary: hex("#dadada"),
            secondary_hover: hex("#cecece"),
            secondary_active: hex("#bfbfbf"),
            secondary_disabled: hex("#ececec"),
            ghost_hover: hex("#e9e9e9"),
            ghost_active: hex("#dcdcdc"),

            list_hover: hex("#f1f4f9"),
            list_active: hex("#e3eaf5"),
            list_selected: hex("#dce8f8"),
            list_selected_bar: hex("#2750a2"),
            menu_hover: hex("#eef3fb"),
            timeline_selected: hex("#cfe3f7"),

            input_background: hex("#ffffff"),
            input_disabled_background: hex("#eeeeee"),
            input_border: hex("#cfcfcf"),
            input_border_focus: hex("#3d7bd9"),
            caret: hex("#2b2b2b"),
            selection: hex("#3b78e0"),
            selection_foreground: hex("#ffffff"),

            info_background: hex("#ddf1f8"),
            info_border: hex("#b9e0ee"),
            warning_background: hex("#fff3c4"),
            danger: hex("#d0342c"),
            success: hex("#2e9e3e"),
            tooltip_background: hex("#d3dae4"),
            tooltip_foreground: hex("#333333"),
            toast_background: hex("#ddf1fc"),
            toast_border: hex("#c3d6e0"),
            toast_foreground: hex("#0f2d44"),
            toast_icon: hex("#6b8faf"),
            warning_banner_background: hex("#fdfae6"),
            warning_banner_border: hex("#e0dbbb"),
            warning_icon: hex("#ecb330"),

            feature_error: hex("#a3202e"),
            feature_editing: hex("#b1ddf8"),
            accept: hex("#009600"),
            accept_disabled: hex("#cfe8cf"),
            cancel: hex("#c42b3c"),
            checkbox_border: hex("#a6a6a6"),
            checkbox_checked: hex("#28549d"),
            checkbox_unavailable: hex("#a8a8a8"),
            selection_field_active: hex("#def1ff"),
            selection_field_active_border: hex("#7a8a97"),

            rail_background: hex("#fafafa"),
            panel_border: hex("#e0e0e0"),
            tool_foreground: hex("#333333"),
            tool_disabled_foreground: hex("#999999"),
            toolbar_separator: hex("#d9d9d9"),
            kbd_border: hex("#9a9a9a"),
            search_background: hex("#eeeeee"),
            tab_inactive: hex("#eaeaea"),
            tab_hover: hex("#f4f4f4"),
            tab_underline: hex("#2b5aa8"),
            feature_icon: hex("#8e8e8e"),
            tree_guide: hex("#dbdbdb"),
            rollback_bar: hex("#bababa"),

            plane_fill: hex("#e8ecf5"),
            plane_edge: hex("#93a8c4"),
            plane_label: hex("#3f5a8c"),
            highlight: hex("#f0a860"),
            selection_3d: hex("#f07c00"),
            axis_x: hex("#e0312b"),
            axis_y: hex("#3fa535"),
            axis_z: hex("#2b35c8"),

            font_xs: 11.0,
            font_sm: 12.0,
            font_base: 13.0,
            font_md: 14.0,
            font_lg: 16.0,
            font_xl: 20.0,

            space: [0.0, 2.0, 4.0, 6.0, 8.0, 12.0, 16.0, 24.0, 32.0],
            radius_sm: 2.0,
            radius: 3.0,
            radius_lg: 4.0,

            button_height_sm: 24.0,
            button_height: 30.0,
            button_height_lg: 32.0,
            input_height: 30.0,
            list_row_height: 38.0,
            menu_item_height: 27.0,
            icon_size: 16.0,
            top_bar_height: 36.0,
            toolbar_height: 38.0,
            tab_bar_height: 29.0,
            sidebar_width: 202.0,

            tooltip_delay: Duration::from_millis(500),
            caret_blink_period: Duration::from_secs(1),
            fade_duration: Duration::from_millis(150),
        }
    }

    /// The dark theme: the light theme's tokens on charcoal surfaces. The interface is always
    /// built from [`Theme::light`]; [`crate::color_scheme`] maps what it draws to these values
    /// while the dark scheme is on, so sizes, fonts and behavior are the light theme's.
    pub fn dark() -> Self {
        Self {
            background: hex("#1f2023"),
            title_bar: hex("#2a2b2f"),
            tab_bar: hex("#18191b"),
            sidebar: hex("#1f2023"),
            viewport_background: hex("#393c42"),
            popover: hex("#2c2e33"),
            floating: hex("#44474e"),
            overlay: Color::srgba(0.0, 0.0, 0.0, 0.79),
            shadow: Color::srgba(0.0, 0.0, 0.0, 0.45),

            foreground: hex("#dcdee1"),
            muted_foreground: hex("#a9abb0"),
            subtle_foreground: hex("#8b8e95"),
            disabled_foreground: hex("#7a7d84"),
            link: hex("#7aaaf0"),

            border: hex("#3d3f44"),
            border_strong: hex("#4c4e54"),
            separator: hex("#35373b"),
            row_separator: hex("#28292d"),
            focus_ring: hex("#5c8fd6"),

            primary: hex("#2f6fd0"),
            primary_hover: hex("#3e7ddc"),
            primary_open: hex("#4f88dc"),
            primary_open_border: hex("#7aa5e8"),
            primary_active: hex("#2861b8"),
            primary_disabled: hex("#2c4469"),
            primary_foreground: hex("#ffffff"),
            primary_disabled_foreground: hex("#8c9bb4"),
            secondary: hex("#3a3c41"),
            secondary_hover: hex("#45474d"),
            secondary_active: hex("#50535a"),
            secondary_disabled: hex("#2c2d31"),
            ghost_hover: hex("#34363b"),
            ghost_active: hex("#3e4046"),

            list_hover: hex("#2a2e36"),
            list_active: hex("#2f3747"),
            list_selected: hex("#263a57"),
            list_selected_bar: hex("#5b8fe0"),
            menu_hover: hex("#2d3544"),
            timeline_selected: hex("#263a57"),

            input_background: hex("#18191c"),
            input_disabled_background: hex("#26272b"),
            input_border: hex("#46484e"),
            input_border_focus: hex("#4f86da"),
            caret: hex("#e6e6e6"),
            selection: hex("#3b78e0"),
            selection_foreground: hex("#ffffff"),

            info_background: hex("#1b3340"),
            info_border: hex("#2a5266"),
            warning_background: hex("#3d3517"),
            danger: hex("#f0605a"),
            success: hex("#4cc15c"),
            tooltip_background: hex("#4d515a"),
            tooltip_foreground: hex("#ececec"),
            toast_background: hex("#1c3545"),
            toast_border: hex("#2d5068"),
            toast_foreground: hex("#d8ecfa"),
            toast_icon: hex("#7fb0d6"),
            warning_banner_background: hex("#36311d"),
            warning_banner_border: hex("#5a5232"),
            warning_icon: hex("#f0bd45"),

            feature_error: hex("#ff9a9e"),
            feature_editing: hex("#22476a"),
            accept: hex("#3dc23d"),
            accept_disabled: hex("#2b4a2b"),
            cancel: hex("#ff6070"),
            checkbox_border: hex("#6c6f75"),
            checkbox_checked: hex("#4a80d2"),
            checkbox_unavailable: hex("#45474d"),
            selection_field_active: hex("#1d3a54"),
            selection_field_active_border: hex("#6d8ca8"),

            rail_background: hex("#232427"),
            panel_border: hex("#35373b"),
            tool_foreground: hex("#d5d6d9"),
            tool_disabled_foreground: hex("#686b72"),
            toolbar_separator: hex("#404247"),
            kbd_border: hex("#6c6f75"),
            search_background: hex("#2a2b2f"),
            tab_inactive: hex("#2a2b2f"),
            tab_hover: hex("#313338"),
            tab_underline: hex("#5b8fe0"),
            feature_icon: hex("#9c9fa5"),
            tree_guide: hex("#404247"),
            rollback_bar: hex("#6d7076"),

            plane_fill: hex("#3b4456"),
            plane_edge: hex("#7189ab"),
            plane_label: hex("#cbd9f2"),
            highlight: hex("#f0a860"),
            selection_3d: hex("#ff8c1a"),
            axis_x: hex("#ff5a52"),
            axis_y: hex("#5fd255"),
            axis_z: hex("#8fa2ff"),
            ..Self::light()
        }
    }

    /// The theme's colors with the roles they're drawn in, for pairing the light and dark
    /// themes ([`crate::color_scheme`]). Where two tokens share a light color in one role, the
    /// earlier one decides its dark color.
    pub fn colors(&self) -> Vec<(ColorRole, Color)> {
        use ColorRole::{Fill, Floating, Ink, Line, Raised};
        vec![
            (Fill, self.background),
            (Fill, self.title_bar),
            (Fill, self.tab_bar),
            (Fill, self.sidebar),
            (Floating, self.floating),
            (Floating, self.tooltip_background),
            // Status fills keep their own dark colors on every surface (a warning toast).
            (Floating, self.toast_background),
            (Floating, self.warning_banner_background),
            (Floating, self.info_background),
            (Floating, self.warning_background),
            (Raised, self.toast_background),
            (Raised, self.warning_banner_background),
            (Raised, self.info_background),
            (Raised, self.warning_background),
            (Raised, self.popover),
            (Raised, self.input_background),
            (Raised, self.tooltip_background),
            (Fill, self.rail_background),
            (Fill, self.tab_inactive),
            (Fill, self.tab_hover),
            (Fill, self.search_background),
            (Fill, self.input_disabled_background),
            (Fill, self.secondary),
            (Fill, self.secondary_hover),
            (Fill, self.secondary_active),
            (Fill, self.secondary_disabled),
            (Fill, self.ghost_hover),
            (Fill, self.ghost_active),
            (Fill, self.list_hover),
            (Fill, self.list_active),
            (Fill, self.list_selected),
            (Fill, self.list_selected_bar),
            (Fill, self.menu_hover),
            (Fill, self.timeline_selected),
            (Fill, self.primary),
            (Fill, self.primary_hover),
            (Fill, self.primary_open),
            (Fill, self.primary_active),
            (Fill, self.primary_disabled),
            (Fill, self.selection),
            (Fill, self.info_background),
            (Fill, self.warning_background),
            (Fill, self.toast_background),
            (Fill, self.warning_banner_background),
            (Fill, self.feature_editing),
            (Fill, self.accept),
            (Fill, self.accept_disabled),
            (Fill, self.checkbox_checked),
            (Fill, self.checkbox_unavailable),
            (Fill, self.selection_field_active),
            (Fill, self.tab_underline),
            (Fill, self.tree_guide),
            (Fill, self.rollback_bar),
            (Fill, self.separator),
            (Fill, self.panel_border),
            (Fill, self.toolbar_separator),
            (Fill, self.border),
            (Fill, self.row_separator),
            (Fill, self.plane_fill),
            (Ink, self.foreground),
            (Ink, self.tool_foreground),
            (Ink, self.muted_foreground),
            (Ink, self.subtle_foreground),
            (Ink, self.disabled_foreground),
            (Ink, self.tool_disabled_foreground),
            (Ink, self.feature_icon),
            (Ink, self.link),
            (Ink, self.primary),
            (Ink, self.primary_foreground),
            (Ink, self.primary_disabled_foreground),
            (Ink, self.selection_foreground),
            (Ink, self.tooltip_foreground),
            (Ink, self.toast_foreground),
            (Ink, self.toast_icon),
            (Ink, self.warning_icon),
            (Ink, self.danger),
            (Ink, self.success),
            (Ink, self.feature_error),
            (Ink, self.accept),
            (Ink, self.cancel),
            (Ink, self.caret),
            (Ink, self.checkbox_checked),
            (Ink, self.list_selected_bar),
            (Ink, self.plane_label),
            (Line, self.border),
            (Line, self.border_strong),
            (Line, self.separator),
            (Line, self.row_separator),
            (Line, self.panel_border),
            (Line, self.toolbar_separator),
            (Line, self.input_border),
            (Line, self.input_border_focus),
            (Line, self.focus_ring),
            (Line, self.kbd_border),
            (Line, self.checkbox_border),
            (Line, self.info_border),
            (Line, self.toast_border),
            (Line, self.warning_banner_border),
            (Line, self.selection_field_active_border),
            (Line, self.primary_open_border),
            (Line, self.tree_guide),
            (Line, self.primary),
            (Line, self.primary_hover),
            (Line, self.primary_active),
            (Line, self.primary_disabled),
            (Line, self.checkbox_checked),
            (Line, self.checkbox_unavailable),
            (Line, self.list_selected_bar),
            (Line, self.tab_underline),
            (Line, self.plane_edge),
        ]
    }

    /// A [`TextFont`] using Inter at `size` px and `weight`.
    pub fn font(&self, size: f32, weight: FontWeight) -> TextFont {
        TextFont {
            font: FontSource::Family(FONT_FAMILY.into()),
            font_size: FontSize::Px(size),
            weight,
            ..default()
        }
    }

    /// Regular UI text: 13 px Inter.
    pub fn body_font(&self) -> TextFont {
        self.font(self.font_base, FontWeight::NORMAL)
    }

    /// A text bundle: `Text` + Inter font + color.
    pub fn text(
        &self,
        text: impl Into<String>,
        size: f32,
        weight: FontWeight,
        color: Color,
    ) -> impl Bundle {
        (
            Text::new(text),
            self.font(size, weight),
            TextColor(color),
            TextLayout::no_wrap(),
        )
    }

    /// Body text (13 px, regular, foreground color).
    pub fn label(&self, text: impl Into<String>) -> impl Bundle {
        self.text(text, self.font_base, FontWeight::NORMAL, self.foreground)
    }
}
