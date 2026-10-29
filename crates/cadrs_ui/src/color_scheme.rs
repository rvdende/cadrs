//! Light and dark color schemes.
//!
//! The interface is always built from [`Theme::light`] (the `Theme` resource), and so are the
//! colors code writes later: hover states, selected rows, fixed swatches. While the dark scheme
//! is on, [`apply_scheme`] maps every UI color it finds to the dark one once the frame is laid
//! out (before it is drawn), and puts the light color back when the scheme turns light again. A color is mapped
//! by what it paints ([`ColorRole`]: a fill, a raised surface, text and icons, a line):
//!
//! - A [`Theme`] token becomes its counterpart in [`Theme::dark`].
//! - Any other color keeps its hue and has its lightness moved along the curve the theme's own
//!   greys define for that role: light fills and lines turn dark, dark text turns light. Strong
//!   colors (a part's swatch, an accent) are left as they are; white text stays white.
//!
//! So nothing that draws the interface needs to know the scheme. What isn't a UI color (the 3D
//! view's background, its planes and sketches) reads [`ActiveTheme`], the theme of the scheme in
//! force, or picks its own pair with [`by_scheme`].
//!
//! The scheme is the user's [`SchemePreference`]: light, dark, or the system's, which is read
//! from the window (Windows, macOS) or the desktop's settings portal (Linux) and again whenever
//! the window gets the focus back.

use bevy::color::Oklcha;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::text::TextCursorStyle;
use bevy::window::{WindowFocused, WindowThemeChanged};

use crate::segmented::{Segmented, SegmentedSelect, SegmentedState};
use crate::style::Selected;
use crate::theme::{ColorRole, Theme};

pub struct ColorSchemePlugin;

impl Plugin for ColorSchemePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Appearance>()
            .init_resource::<ActiveTheme>()
            .init_resource::<SchemeMap>()
            .init_resource::<SystemSchemeProbe>()
            .add_systems(PreUpdate, (detect_system_scheme, resolve_scheme).chain())
            .add_systems(Update, sync_toggles)
            .add_observer(on_toggle)
            .add_systems(
                PostUpdate,
                (
                    apply_scheme::<BackgroundColor>,
                    apply_scheme::<BorderColor>,
                    apply_scheme::<Outline>,
                    apply_scheme::<TextColor>,
                    apply_scheme::<ImageNode>,
                    apply_scheme::<TextCursorStyle>,
                    apply_scheme::<BoxShadow>,
                    finish_scheme,
                )
                    .chain()
                    // After layout, so after every system that writes a light color before
                    // it: unordered, such a write and its mapping alternated, and the color
                    // flickered (a number field's underline in Preferences). Colors are only
                    // read when the frame is extracted.
                    .after(bevy::ui::UiSystems::PostLayout),
            );
    }
}

/// A color scheme in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum ColorScheme {
    #[default]
    Light,
    Dark,
}

/// The scheme the user asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum SchemePreference {
    /// Follow the system's light or dark setting.
    #[default]
    System,
    Light,
    Dark,
}

impl SchemePreference {
    /// The scheme this preference resolves to when the system's is `system` (light when the
    /// system doesn't say).
    pub fn resolve(self, system: Option<ColorScheme>) -> ColorScheme {
        match self {
            Self::System => system.unwrap_or_default(),
            Self::Light => ColorScheme::Light,
            Self::Dark => ColorScheme::Dark,
        }
    }
}

/// The preferred scheme, the system's, and the one in force.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct Appearance {
    pub preference: SchemePreference,
    /// The system's scheme, when known.
    pub system: Option<ColorScheme>,
    /// Whether to ask the system at all. Off for scripted and headless runs, which must look
    /// the same on every machine.
    pub follow_system: bool,
    scheme: ColorScheme,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            preference: SchemePreference::System,
            system: None,
            follow_system: true,
            scheme: ColorScheme::Light,
        }
    }
}

impl Appearance {
    /// An appearance that never asks the system (a scripted run).
    pub fn without_system() -> Self {
        Self {
            follow_system: false,
            ..default()
        }
    }

    /// The scheme in force.
    pub fn scheme(&self) -> ColorScheme {
        self.scheme
    }

    pub fn is_dark(&self) -> bool {
        self.scheme == ColorScheme::Dark
    }
}

static DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the dark scheme is in force (it changes in `PreUpdate`, never mid-frame).
pub fn dark_in_force() -> bool {
    DARK.load(std::sync::atomic::Ordering::Relaxed)
}

/// `light` or `dark` by the scheme in force: for what is drawn outside the UI (gizmos, 3D
/// materials) by code that has no [`ActiveTheme`] at hand. A UI node's colors are mapped by
/// [`apply_scheme`] instead, so they are always given in light.
pub fn by_scheme(light: Color, dark: Color) -> Color {
    if dark_in_force() { dark } else { light }
}

/// The theme of the scheme in force, for colors that aren't UI nodes (the 3D view's background
/// and overlays). The UI itself is built from the `Theme` resource.
#[derive(Resource, Debug, Clone, Default, Deref)]
pub struct ActiveTheme(pub Theme);

/// How light-scheme colors become dark ones.
#[derive(Resource, Debug, Clone)]
pub struct SchemeMap {
    tokens: HashMap<(ColorRole, [u8; 3]), Srgba>,
    /// Per role, (light lightness, dark lightness) of the theme's greys, by light lightness.
    curves: HashMap<ColorRole, Vec<(f32, f32)>>,
    /// How much lighter than the page a raised and a floating surface are (OKLCH lightness):
    /// what's drawn on them is lifted as much, so a hover stays lighter than its menu and text
    /// keeps its contrast.
    lift: [f32; 2],
}

impl Default for SchemeMap {
    fn default() -> Self {
        Self::new(&Theme::light(), &Theme::dark())
    }
}

/// Chroma below which a color counts as grey (OKLCH).
const GREY: f32 = 0.025;
/// Chroma above which a fill or line counts as a strong color and isn't changed.
const STRONG: f32 = 0.08;
/// Lightness below which an opaque grey fill or line counts as ink.
const INKY: f32 = 0.4;

fn key(c: Srgba) -> [u8; 3] {
    let [r, g, b, _] = c.to_u8_array();
    [r, g, b]
}

impl SchemeMap {
    /// Pairs the tokens of `light` with those of `dark`.
    pub fn new(light: &Theme, dark: &Theme) -> Self {
        let mut tokens = HashMap::default();
        let mut curves: HashMap<ColorRole, Vec<(f32, f32)>> = HashMap::default();
        for ((role, l), (_, d)) in light.colors().into_iter().zip(dark.colors()) {
            let (l, d) = (l.to_srgba(), d.to_srgba());
            if tokens.contains_key(&(role, key(l))) {
                continue;
            }
            tokens.insert((role, key(l)), d);
            let (lo, dk) = (Oklcha::from(l), Oklcha::from(d));
            if lo.chroma < GREY && dk.chroma < GREY {
                curves
                    .entry(role)
                    .or_default()
                    .push((lo.lightness, dk.lightness));
            }
        }
        for points in curves.values_mut() {
            points.sort_by(|a, b| a.0.total_cmp(&b.0));
            // Keep it monotonic (lighter in, darker out): drop points that would bend it back.
            let mut kept: Vec<(f32, f32)> = Vec::with_capacity(points.len());
            for &p in points.iter() {
                if kept
                    .last()
                    .is_none_or(|last| p.0 > last.0 + 1e-4 && p.1 <= last.1)
                {
                    kept.push(p);
                }
            }
            *points = kept;
        }
        let l = |c: Color| Oklcha::from(c.to_srgba()).lightness;
        let lift = [
            l(dark.popover) - l(dark.background),
            l(dark.floating) - l(dark.background),
        ];
        Self {
            tokens,
            curves,
            lift,
        }
    }

    /// The dark-scheme color for `color` painted in `role`. Alpha is kept, except that a
    /// barely see-through white panel (85% and up: a legend over the 3D view) is all but
    /// opaque in dark, where what shows through would be light lines on a dark veil. (A fade
    /// passes that band in a moment.)
    pub fn dark(&self, color: Color, role: ColorRole) -> Color {
        let out = self.dark_keeping_alpha(color, role);
        let c = color.to_srgba();
        let frosted = role != ColorRole::Ink
            && role != ColorRole::Line
            && (0.85..0.99).contains(&c.alpha)
            && Oklcha::from(c).lightness > 0.95;
        if frosted { out.with_alpha(0.97) } else { out }
    }

    fn dark_keeping_alpha(&self, color: Color, role: ColorRole) -> Color {
        let c = color.to_srgba();
        if c.alpha == 0.0 {
            return color;
        }
        if let Some(d) = self.tokens.get(&(role, key(c))) {
            return Color::Srgba(d.with_alpha(c.alpha));
        }
        let o = Oklcha::from(c);
        let Some(curve) = self.curves.get(&role).filter(|c| c.len() >= 2) else {
            return color;
        };
        // A translucent black rule (a swatch's rim, a divider) would vanish on dark: a faint
        // light one instead.
        if role == ColorRole::Line && o.chroma < GREY && o.lightness < INKY && c.alpha < 0.99 {
            return Color::Srgba(Srgba::WHITE.with_alpha(c.alpha * 0.6));
        }
        let lightness = match role {
            ColorRole::Fill | ColorRole::Raised | ColorRole::Floating | ColorRole::Line
                if o.chroma < GREY && o.lightness < INKY && c.alpha > 0.99 =>
            {
                // Near-black drawn as a fill or line is ink (a marker, a dot, a rule): it turns
                // light like text. Translucent black (a shadow, a backdrop) stays.
                match self.curves.get(&ColorRole::Ink) {
                    Some(ink) if ink.len() >= 2 => along(ink, o.lightness),
                    _ => return color,
                }
            }
            ColorRole::Fill | ColorRole::Raised | ColorRole::Floating | ColorRole::Line => {
                if o.chroma > STRONG {
                    return color;
                }
                // Mid and dark fills (a badge, a shadow) already read on dark: blend into
                // themselves below the curve's darkest light grey.
                let start = curve[0].0;
                let floor = (start - 0.3).max(0.0);
                if o.lightness >= start {
                    along(curve, o.lightness)
                } else if o.lightness <= floor {
                    return color;
                } else {
                    let t = (o.lightness - floor) / (start - floor);
                    o.lightness + (curve[0].1 - o.lightness) * t
                }
            }
            ColorRole::Ink => {
                if o.lightness > 0.6 && o.chroma > STRONG {
                    return color;
                }
                // Light text is on a strong fill already (white on a primary button): fade out
                // the curve towards white.
                let end = curve[curve.len() - 1].0;
                let top = 0.97;
                if o.lightness <= end {
                    along(curve, o.lightness)
                } else if o.lightness >= top {
                    return color;
                } else {
                    let t = (o.lightness - end) / (top - end);
                    let at_end = curve[curve.len() - 1].1;
                    at_end + (o.lightness - at_end) * t
                }
            }
        };
        let out = Oklcha { lightness, ..o };
        Color::Srgba(Srgba::from(out).with_alpha(c.alpha))
    }

    /// Pairs a light color the app draws in `role` (one that isn't a [`Theme`] token: a
    /// feature's own tint) with its dark counterpart, as [`Theme::dark`] does for the tokens.
    /// On raised and floating surfaces it is lifted like the page's colors.
    pub fn pair(&mut self, role: ColorRole, light: Color, dark: Color) {
        self.tokens
            .insert((role, key(light.to_srgba())), dark.to_srgba());
    }

    /// The dark-scheme color for `color` painted in `role` (a fill, ink or a line) on `surface`:
    /// the surface's own token, else the page's color lifted to the surface.
    pub fn dark_on(&self, color: Color, role: ColorRole, surface: Surface) -> Color {
        let (own, lift) = match surface {
            Surface::Page => return self.dark(color, role),
            Surface::Raised => (ColorRole::Raised, self.lift[0]),
            Surface::Floating => (ColorRole::Floating, self.lift[1]),
        };
        let c = color.to_srgba();
        if role == ColorRole::Fill
            && let Some(d) = self.tokens.get(&(own, key(c)))
        {
            return Color::Srgba(d.with_alpha(c.alpha));
        }
        let page = self.dark(color, role);
        if key(page.to_srgba()) == key(c) {
            // Kept as it is (a strong color, white text): not lifted either.
            return color;
        }
        let o = Oklcha::from(page.to_srgba());
        let lifted = Oklcha {
            lightness: (o.lightness + lift).min(0.97).max(o.lightness),
            ..o
        };
        Color::Srgba(Srgba::from(lifted))
    }

    /// `color` in the given scheme.
    pub fn resolve(&self, color: Color, role: ColorRole, scheme: ColorScheme) -> Color {
        match scheme {
            ColorScheme::Light => color,
            ColorScheme::Dark => self.dark(color, role),
        }
    }
}

/// Piecewise-linear interpolation, flat past the ends.
fn along(curve: &[(f32, f32)], x: f32) -> f32 {
    let (first, last) = (curve[0], curve[curve.len() - 1]);
    if x <= first.0 {
        return first.1;
    }
    if x >= last.0 {
        return last.1;
    }
    let i = curve.partition_point(|p| p.0 <= x);
    let (a, b) = (curve[i - 1], curve[i]);
    a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0)
}

// ---------------------------------------------------------------------------------------------
// The system's scheme

/// The scheme the desktop reports, read off the main thread (Linux asks the settings portal,
/// which can take a moment).
#[derive(Resource, Default)]
struct SystemSchemeProbe {
    pending: Option<std::sync::Mutex<std::sync::mpsc::Receiver<Option<ColorScheme>>>>,
    asked: bool,
}

fn from_window(theme: bevy::window::WindowTheme) -> ColorScheme {
    match theme {
        bevy::window::WindowTheme::Light => ColorScheme::Light,
        bevy::window::WindowTheme::Dark => ColorScheme::Dark,
    }
}

fn detect_system_scheme(
    mut appearance: ResMut<Appearance>,
    mut probe: ResMut<SystemSchemeProbe>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut theme_changed: MessageReader<WindowThemeChanged>,
    mut focused: MessageReader<WindowFocused>,
) {
    let refocused = focused.read().any(|f| f.focused);
    let changed = theme_changed.read().last().map(|e| from_window(e.theme));
    if !appearance.follow_system {
        return;
    }
    let mut system = appearance.system;
    if let Some(s) = changed {
        system = Some(s);
    }
    if !probe.asked || refocused {
        probe.asked = true;
        if let Some(t) = windows.single().ok().and_then(|w| w.window_theme) {
            system = Some(from_window(t));
        }
        // A page has no thread for it, and the browser tells the window (`window_theme`).
        #[cfg(not(target_arch = "wasm32"))]
        if probe.pending.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(desktop_scheme());
            });
            probe.pending = Some(std::sync::Mutex::new(rx));
        }
    }
    if let Some(rx) = &probe.pending
        && let Ok(answer) = rx
            .lock()
            .map_err(drop)
            .and_then(|rx| rx.try_recv().map_err(drop))
    {
        probe.pending = None;
        if answer.is_some() {
            system = answer;
        }
    }
    if appearance.system != system {
        appearance.system = system;
    }
}

/// The desktop's preference where the window can't tell (Linux): the freedesktop settings
/// portal, then GNOME's own setting.
#[cfg(target_os = "linux")]
fn desktop_scheme() -> Option<ColorScheme> {
    use std::process::{Command, Stdio};
    let run = |cmd: &str, args: &[&str]| -> Option<String> {
        let out = Command::new(cmd)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    // `color-scheme`: 0 no preference, 1 dark, 2 light.
    if let Some(out) = run(
        "gdbus",
        &[
            "call",
            "--session",
            "--timeout=2",
            "--dest=org.freedesktop.portal.Desktop",
            "--object-path=/org/freedesktop/portal/desktop",
            "--method=org.freedesktop.portal.Settings.ReadOne",
            "org.freedesktop.appearance",
            "color-scheme",
        ],
    ) {
        match out
            .split_whitespace()
            .last()
            .map(|w| w.trim_matches(|c| ">,)".contains(c)))
        {
            Some("1") => return Some(ColorScheme::Dark),
            Some("2") => return Some(ColorScheme::Light),
            _ => {}
        }
    }
    let gtk = run(
        "gsettings",
        &["get", "org.gnome.desktop.interface", "color-scheme"],
    )?;
    if gtk.contains("dark") {
        Some(ColorScheme::Dark)
    } else if gtk.contains("light") {
        Some(ColorScheme::Light)
    } else {
        None
    }
}

// (A page asks the browser instead.)
#[cfg(not(any(target_os = "linux", target_arch = "wasm32")))]
fn desktop_scheme() -> Option<ColorScheme> {
    None
}

fn resolve_scheme(
    mut appearance: ResMut<Appearance>,
    mut active: ResMut<ActiveTheme>,
    mut clear: ResMut<ClearColor>,
) {
    if !appearance.is_changed() {
        return;
    }
    let scheme = appearance.preference.resolve(appearance.system);
    if appearance.scheme != scheme || appearance.is_added() {
        appearance.scheme = scheme;
        DARK.store(
            scheme == ColorScheme::Dark,
            std::sync::atomic::Ordering::Relaxed,
        );
        active.0 = match scheme {
            ColorScheme::Light => Theme::light(),
            ColorScheme::Dark => Theme::dark(),
        };
        clear.0 = active.viewport_background;
    }
}

// ---------------------------------------------------------------------------------------------
// Mapping the UI's colors

/// A UI component whose colors the scheme maps.
pub trait SchemeColors: Component<Mutability = bevy::ecs::component::Mutable> {
    type Value: Clone + PartialEq + Send + Sync + 'static;
    /// Whether this is a node's fill, which [`KeepFill`] keeps.
    const FILL: bool = false;
    fn get(&self) -> Self::Value;
    fn set(&mut self, v: Self::Value);
    /// The value in the dark scheme, for a node at `place`.
    fn dark(v: Self::Value, map: &SchemeMap, place: Place) -> Self::Value;
    /// When `current` is the mapped `dark` value with only its alpha changed (code fading the
    /// color it read back), the light value with that alpha.
    fn refade(
        _light: Self::Value,
        _dark: Self::Value,
        _current: Self::Value,
    ) -> Option<Self::Value> {
        None
    }
}

/// An opaque near-black grey (the colors [`SchemeMap::dark`] treats as ink).
fn inky(c: Color) -> bool {
    let c = c.to_srgba();
    let o = Oklcha::from(c);
    o.chroma < GREY && o.lightness < INKY && c.alpha > 0.99
}

/// `light` with `current`'s alpha, if `current` is `dark` with another alpha.
fn refade(light: Color, dark: Color, current: Color) -> Option<Color> {
    let (d, c) = (dark.to_srgba(), current.to_srgba());
    (d.with_alpha(0.0) == c.with_alpha(0.0)).then(|| light.with_alpha(c.alpha))
}

/// Where a UI node is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    pub surface: Surface,
    /// A small node (at most 16 px each way: a dot, a ring, a knob), whose near-black fill is
    /// ink like a line's.
    pub marker: bool,
}

/// The surface a UI node is drawn on, the nearest of it and its ancestors that is raised or
/// floating: its colors are mapped for that surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Page,
    /// A dialog, a floating panel: any node with a `GlobalZIndex` below the menus', or marked
    /// [`RaisedSurface`].
    Raised,
    /// Menus, popups and toasts: `GlobalZIndex` of [`crate::z::MENU`] and above.
    Floating,
}

impl SchemeColors for BackgroundColor {
    type Value = Color;
    const FILL: bool = true;
    fn refade(light: Color, dark: Color, current: Color) -> Option<Color> {
        refade(light, dark, current)
    }
    fn get(&self) -> Color {
        self.0
    }
    fn set(&mut self, v: Color) {
        self.0 = v;
    }
    fn dark(v: Color, map: &SchemeMap, place: Place) -> Color {
        // A dark chip (a HUD bar with white text) stays dark; only a marker's near-black fill
        // is ink.
        if !place.marker && inky(v) {
            return v;
        }
        map.dark_on(v, ColorRole::Fill, place.surface)
    }
}

impl SchemeColors for BorderColor {
    type Value = [Color; 4];
    fn get(&self) -> [Color; 4] {
        [self.top, self.right, self.bottom, self.left]
    }
    fn set(&mut self, [top, right, bottom, left]: [Color; 4]) {
        *self = BorderColor {
            top,
            right,
            bottom,
            left,
        };
    }
    fn dark(v: [Color; 4], map: &SchemeMap, place: Place) -> [Color; 4] {
        v.map(|c| map.dark_on(c, ColorRole::Line, place.surface))
    }
}

impl SchemeColors for Outline {
    type Value = Color;
    fn refade(light: Color, dark: Color, current: Color) -> Option<Color> {
        refade(light, dark, current)
    }
    fn get(&self) -> Color {
        self.color
    }
    fn set(&mut self, v: Color) {
        self.color = v;
    }
    fn dark(v: Color, map: &SchemeMap, place: Place) -> Color {
        map.dark_on(v, ColorRole::Line, place.surface)
    }
}

impl SchemeColors for TextColor {
    type Value = Color;
    fn refade(light: Color, dark: Color, current: Color) -> Option<Color> {
        refade(light, dark, current)
    }
    fn get(&self) -> Color {
        self.0
    }
    fn set(&mut self, v: Color) {
        self.0 = v;
    }
    fn dark(v: Color, map: &SchemeMap, place: Place) -> Color {
        map.dark_on(v, ColorRole::Ink, place.surface)
    }
}

impl SchemeColors for ImageNode {
    type Value = Color;
    fn refade(light: Color, dark: Color, current: Color) -> Option<Color> {
        refade(light, dark, current)
    }
    fn get(&self) -> Color {
        self.color
    }
    fn set(&mut self, v: Color) {
        self.color = v;
    }
    fn dark(v: Color, map: &SchemeMap, place: Place) -> Color {
        map.dark_on(v, ColorRole::Ink, place.surface)
    }
}

impl SchemeColors for TextCursorStyle {
    type Value = (Color, Option<Color>);
    fn get(&self) -> Self::Value {
        (self.color, self.selected_text_color)
    }
    fn set(&mut self, (color, selected): Self::Value) {
        self.color = color;
        self.selected_text_color = selected;
    }
    fn dark((color, selected): Self::Value, map: &SchemeMap, place: Place) -> Self::Value {
        (map.dark_on(color, ColorRole::Ink, place.surface), selected)
    }
}

impl SchemeColors for BoxShadow {
    type Value = Vec<Color>;
    fn get(&self) -> Vec<Color> {
        self.0.iter().map(|s| s.color).collect()
    }
    fn set(&mut self, v: Vec<Color>) {
        for (s, c) in self.0.iter_mut().zip(v) {
            s.color = c;
        }
    }
    /// A shadow on a dark page needs to be deeper to show.
    fn dark(v: Vec<Color>, _: &SchemeMap, _: Place) -> Vec<Color> {
        v.into_iter()
            .map(|c| c.with_alpha((c.alpha() * 2.5).min(0.6)))
            .collect()
    }
}

/// The light color a component was given and the dark one the scheme wrote over it.
#[derive(Component, Debug, Clone)]
pub struct Mapped<T: SchemeColors> {
    light: T::Value,
    dark: T::Value,
}

/// The light color of `c`, as the interface gave it, for code that keeps a color it reads
/// back (to restore it later): with the dark scheme on, `c` holds the mapped color.
pub fn authored<T: SchemeColors>(c: &T, mapped: Option<&Mapped<T>>) -> T::Value {
    match mapped {
        Some(m) if c.get() == m.dark => m.light.clone(),
        _ => c.get(),
    }
}

/// Marks a UI node whose colors are already the scheme's own (the sketch's labels, drawn in the
/// sketch's colors for the scheme): [`apply_scheme`] leaves it alone.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct KeepColors;

/// Marks a UI node whose fill is data, not decoration (a color swatch, a material's chip, a
/// label's dot): its fill is shown as it is in both schemes. Its border and text are mapped.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct KeepFill;

/// Marks a UI node as a raised surface (a dialog's panel), like one with a `GlobalZIndex`.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct RaisedSurface;

/// The scheme the UI's colors were last mapped for.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MappedScheme(ColorScheme);

/// What the scheme maps: UI nodes and their text spans (not world-space `Text2d`, like a
/// drawing sheet's text, which sits on paper).
type UiColor = Or<(With<Node>, With<TextSpan>)>;
/// Left alone: [`KeepColors`], a [`KeepFill`] node's fill, and full-colour icons, which the icon
/// atlas draws for the scheme itself (their tint only fades them; mapping it as ink fought
/// `icon::sync_icon_layers` over the colour every frame, and every write laid the UI out again).
type Keep = (Has<KeepColors>, Has<KeepFill>, Has<crate::icon::FullColour>);
type Item<'a, T> = (
    Entity,
    Mut<'a, T>,
    Option<Mut<'a, Mapped<T>>>,
    (bool, bool, bool),
);

/// Maps the colors of `T` to the scheme in force: the ones changed since the last frame, or all
/// of them when the scheme changes.
#[allow(clippy::type_complexity)]
pub fn apply_scheme<T: SchemeColors>(
    appearance: Res<Appearance>,
    mapped_for: Option<Res<MappedScheme>>,
    map: Res<SchemeMap>,
    mut q: ParamSet<(
        Query<(Entity, &mut T, Option<&mut Mapped<T>>, Keep), (Changed<T>, UiColor)>,
        Query<(Entity, &mut T, Option<&mut Mapped<T>>, Keep), UiColor>,
    )>,
    surfaces: Query<(
        Option<&GlobalZIndex>,
        Has<RaisedSurface>,
        Has<crate::dialog::DialogRoot>,
        Option<&ChildOf>,
    )>,
    nodes: Query<&Node>,
    mut commands: Commands,
) {
    let scheme = appearance.scheme();
    let switched = mapped_for.is_none_or(|m| m.0 != scheme);
    let surface = |mut e: Entity| {
        loop {
            match surfaces.get(e) {
                Ok((Some(z), ..)) if z.0 >= crate::z::MENU => return Surface::Floating,
                Ok((Some(_), ..) | (_, true, ..) | (_, _, true, _)) => return Surface::Raised,
                Ok((.., Some(parent))) => e = parent.parent(),
                _ => return Surface::Page,
            }
        }
    };
    let marker = |e: Entity| {
        let small = |v: Val| matches!(v, Val::Px(px) if px <= 16.0);
        nodes
            .get(e)
            .is_ok_and(|n| small(n.width) && small(n.height))
    };
    let place = |e: Entity| Place {
        surface: surface(e),
        marker: marker(e),
    };
    let mut visit = |(e, mut c, mapped, (keep, keep_fill, full_colour)): Item<T>| {
        if keep || full_colour || (T::FILL && keep_fill) {
            return;
        }
        let current = c.get();
        // Our own write from the last pass: nothing new.
        let light = match &mapped {
            Some(m) if current == m.dark => {
                if !switched {
                    return;
                }
                m.light.clone()
            }
            Some(m) => T::refade(m.light.clone(), m.dark.clone(), current.clone())
                .unwrap_or(current.clone()),
            None => current.clone(),
        };
        match scheme {
            ColorScheme::Light => {
                if mapped.is_some() {
                    if current != light {
                        c.set(light);
                    }
                    commands.entity(e).remove::<Mapped<T>>();
                }
            }
            ColorScheme::Dark => {
                // Code that writes its light color every frame: the same dark one.
                let dark = match &mapped {
                    Some(m) if m.light == light && !switched => m.dark.clone(),
                    _ => T::dark(light.clone(), &map, place(e)),
                };
                if current != dark {
                    c.set(dark.clone());
                }
                match mapped {
                    Some(mut m) => *m = Mapped { light, dark },
                    None => {
                        commands.entity(e).try_insert(Mapped::<T> { light, dark });
                    }
                }
            }
        }
    };
    if switched {
        q.p1().iter_mut().for_each(&mut visit);
    } else if scheme == ColorScheme::Dark {
        q.p0().iter_mut().for_each(&mut visit);
    }
}

fn finish_scheme(
    appearance: Res<Appearance>,
    mapped_for: Option<ResMut<MappedScheme>>,
    mut commands: Commands,
) {
    let scheme = appearance.scheme();
    match mapped_for {
        Some(mut m) if m.0 != scheme => m.0 = scheme,
        Some(_) => {}
        None => commands.insert_resource(MappedScheme(scheme)),
    }
}

// ---------------------------------------------------------------------------------------------
// The toggle

/// The scheme toggle of a top bar.
#[derive(Component, Debug, Clone, Copy)]
pub struct SchemeToggle;

const CHOICES: [SchemePreference; 3] = [
    SchemePreference::Light,
    SchemePreference::Dark,
    SchemePreference::System,
];

/// The light / dark / system toggle for a top bar: three icon segments named
/// `color-scheme-0` (light), `-1` (dark) and `-2` (the system's), the [`Appearance`]
/// preference's one on.
pub fn scheme_toggle(theme: &Theme) -> impl Bundle {
    (
        Segmented::new("color-scheme")
            .option("", Some("theme-light-medium"), "Light")
            .option("", Some("theme-dark"), "Dark")
            .option("", Some("theme-system"), "Match the system")
            .selected(2)
            .size(22.0, 14.0)
            .build(theme),
        SchemeToggle,
    )
}

fn on_toggle(
    ev: On<SegmentedSelect>,
    toggles: Query<(), With<SchemeToggle>>,
    mut appearance: ResMut<Appearance>,
) {
    if toggles.contains(ev.entity)
        && let Some(&choice) = CHOICES.get(ev.index)
        && appearance.preference != choice
    {
        appearance.preference = choice;
    }
}

/// Shows the preference on every toggle (it can change elsewhere: the saved preferences, a
/// script).
fn sync_toggles(
    appearance: Res<Appearance>,
    mut toggles: Query<(&mut SegmentedState, &Children), With<SchemeToggle>>,
    added: Query<(), Added<SchemeToggle>>,
    selected: Query<Has<Selected>>,
    mut commands: Commands,
) {
    if !appearance.is_changed() && added.is_empty() {
        return;
    }
    let want = CHOICES
        .iter()
        .position(|&c| c == appearance.preference)
        .unwrap_or(2);
    for (mut state, children) in &mut toggles {
        if state.selected != want {
            state.selected = want;
        }
        for (i, c) in children.iter().enumerate() {
            match (selected.get(c), i == want) {
                (Ok(false), true) => {
                    commands.entity(c).insert(Selected);
                }
                (Ok(true), false) => {
                    commands.entity(c).remove::<Selected>();
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(c: Color) -> f32 {
        Oklcha::from(c.to_srgba()).lightness
    }

    #[test]
    fn tokens_map_to_their_dark_counterparts() {
        let (light, dark) = (Theme::light(), Theme::dark());
        let map = SchemeMap::default();
        assert_eq!(map.dark(light.background, ColorRole::Fill), dark.background);
        assert_eq!(map.dark(light.popover, ColorRole::Raised), dark.popover);
        assert_eq!(map.dark(light.foreground, ColorRole::Ink), dark.foreground);
        assert_eq!(map.dark(light.border, ColorRole::Line), dark.border);
        // A primary button's label takes its own dark color (dark text on the bright cyan).
        assert_eq!(
            map.dark(light.primary_foreground, ColorRole::Ink),
            dark.primary_foreground
        );
    }

    #[test]
    fn other_greys_follow_the_curve() {
        let map = SchemeMap::default();
        let dark = Theme::dark();
        // A light grey fill the theme doesn't name lands among the dark surfaces.
        let fill = map.dark(Color::srgb_u8(0xf4, 0xf4, 0xf4), ColorRole::Fill);
        assert!(l(fill) < 0.4, "{fill:?}");
        assert!(l(fill) >= l(dark.background) - 0.01);
        // Near-black text turns light, mid grey text stays mid.
        assert!(l(map.dark(Color::srgb_u8(0x44, 0x44, 0x44), ColorRole::Ink)) > 0.75);
        let mid = map.dark(Color::srgb_u8(0x99, 0x99, 0x99), ColorRole::Ink);
        assert!((0.4..0.7).contains(&l(mid)), "{mid:?}");
        // A light grey line turns into a dark one.
        assert!(l(map.dark(Color::srgb_u8(0xe2, 0xe2, 0xe2), ColorRole::Line)) < 0.45);
        // Opaque near-black fills and lines are ink; a backdrop stays dark.
        let light = Theme::light();
        assert!(l(map.dark(light.foreground, ColorRole::Fill)) > 0.75);
        assert!(l(map.dark(Color::BLACK, ColorRole::Line)) > 0.75);
        assert_eq!(map.dark(light.overlay, ColorRole::Fill), light.overlay);
    }

    #[test]
    fn strong_colors_and_alpha_are_kept() {
        let map = SchemeMap::default();
        let orange = Color::srgb(0.91, 0.66, 0.22);
        assert_eq!(map.dark(orange, ColorRole::Fill), orange);
        assert_eq!(map.dark(Color::NONE, ColorRole::Fill), Color::NONE);
        let veil = map.dark(Color::srgba(1.0, 1.0, 1.0, 0.4), ColorRole::Fill);
        assert_eq!(veil.alpha(), 0.4);
        assert_eq!(
            map.dark(Color::WHITE, ColorRole::Ink).to_srgba(),
            Srgba::WHITE
        );
    }

    #[test]
    fn raised_and_floating_surfaces_lift_what_is_on_them() {
        let (light, dark) = (Theme::light(), Theme::dark());
        let map = SchemeMap::default();
        // A menu's own fill is its token; a hovered row on it is lighter than the menu.
        let menu = map.dark_on(light.popover, ColorRole::Fill, Surface::Floating);
        assert_eq!(menu, dark.floating);
        let hover = map.dark_on(light.menu_hover, ColorRole::Fill, Surface::Floating);
        assert!(l(hover) > l(menu) + 0.02, "{hover:?} on {menu:?}");
        let row = map.dark_on(light.list_hover, ColorRole::Fill, Surface::Raised);
        assert!(l(row) > l(dark.popover) + 0.02, "{row:?}");
        // Disabled text on a menu keeps about the contrast it has on the page.
        let page = l(dark.disabled_foreground) - l(dark.background);
        let on_menu = l(map.dark_on(light.disabled_foreground, ColorRole::Ink, Surface::Floating))
            - l(dark.floating);
        assert!((on_menu - page).abs() < 0.02, "{on_menu} vs {page}");
        // White text on a primary button in a menu stays white.
        assert_eq!(
            map.dark_on(Color::WHITE, ColorRole::Ink, Surface::Floating)
                .to_srgba()
                .to_u8_array(),
            [255; 4]
        );
        // A swatch's translucent black rim becomes a faint light one.
        let rim = map.dark(Color::srgba(0.0, 0.0, 0.0, 0.25), ColorRole::Line);
        assert!(l(rim) > 0.9 && rim.alpha() < 0.25);
    }

    #[test]
    fn the_preference_resolves() {
        assert_eq!(SchemePreference::System.resolve(None), ColorScheme::Light);
        assert_eq!(
            SchemePreference::System.resolve(Some(ColorScheme::Dark)),
            ColorScheme::Dark
        );
        assert_eq!(
            SchemePreference::Light.resolve(Some(ColorScheme::Dark)),
            ColorScheme::Light
        );
    }

    #[test]
    fn switching_maps_and_restores() {
        let mut app = App::new();
        app.insert_resource(Appearance::default())
            .init_resource::<SchemeMap>()
            .init_resource::<ActiveTheme>()
            .init_resource::<ClearColor>()
            .add_systems(PreUpdate, resolve_scheme)
            .add_systems(
                PostUpdate,
                (apply_scheme::<BackgroundColor>, finish_scheme).chain(),
            );
        let light = Theme::light();
        let e = app
            .world_mut()
            .spawn((Node::default(), BackgroundColor(light.background)))
            .id();
        app.update();
        let bg = |app: &App| app.world().get::<BackgroundColor>(e).unwrap().0;
        assert_eq!(bg(&app), light.background);
        app.world_mut().resource_mut::<Appearance>().preference = SchemePreference::Dark;
        app.update();
        assert_eq!(bg(&app), Theme::dark().background);
        // A fade of the color read back is a fade of the light one.
        let faded = Theme::dark().background.with_alpha(0.5);
        app.world_mut().get_mut::<BackgroundColor>(e).unwrap().0 = faded;
        app.update();
        assert_eq!(bg(&app), faded);
        // A light color written while dark is mapped too.
        app.world_mut().get_mut::<BackgroundColor>(e).unwrap().0 = light.list_hover;
        app.update();
        assert_eq!(bg(&app), Theme::dark().list_hover);
        app.update();
        assert_eq!(bg(&app), Theme::dark().list_hover);
        app.world_mut().resource_mut::<Appearance>().preference = SchemePreference::Light;
        app.update();
        assert_eq!(bg(&app), light.list_hover);
        assert_eq!(
            app.world().resource::<ClearColor>().0,
            light.viewport_background
        );
    }
}
