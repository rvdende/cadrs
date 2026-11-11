//! Tabs between documents and between running cadrs: **Copy** in a tab's menu, **Paste** (the
//! tab bar's menu, its **+** menu, or Ctrl+V), and a tab as a `.cadrs` file (the Export
//! dialog's CADRS format, **+ ▸ Import…**). The model is [`cadrs_core::tab_package`].
//!
//! - **Copy** puts the tab on the system clipboard as text (its `.cadrs` package: the features,
//!   the tabs it references and the files it uses), so another cadrs reads it. Without a system
//!   clipboard (a machine with no display) the copy is still kept here.
//! - **Paste** adds it right of the active tab and opens it: one undo step. The menus name it
//!   "Paste <tab> from <document>".
//! - **Features**: **Copy** in a feature's or a feature folder's menu, or Ctrl+C with features
//!   selected in the feature list, puts them on the clipboard the same way (a folder comes
//!   along when all its features do); **Paste** in a feature's menu, or Ctrl+V in a Part
//!   Studio, adds them at its rollback bar and selects them: one undo step.
//! - **Ctrl+V** pastes what is on the clipboard unless something was copied in this cadrs since
//!   (sketch entities, assembly instances, schematic items: [`TabClipboard::local_copy`]);
//!   those paste as before then. In a browser the clipboard is read in the background, so
//!   what was copied in another cadrs is pasted when the read comes back.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy::clipboard::{Clipboard, ClipboardError, ClipboardRead};
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use cadrs_core::tab_package::Kind;
use cadrs_core::{ElementId, ElementKind, FeatureId, native, tab_package};
use cadrs_ui::ShortcutKey;
use cadrs_ui::i18n::tr_with;

use crate::viewport::{Pick, Selection};
use crate::{ActiveDocument, AppClock, AppState, DocumentStore, UserProfile};

pub struct TabClipboardPlugin;

impl Plugin for TabClipboardPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TabClipboard>().add_systems(
            Update,
            (
                (shortcut.in_set(TabPaste), copy_shortcut).run_if(in_state(AppState::Document)),
                note_local_copy,
                finish_read,
            ),
        );
    }
}

/// The Ctrl+V of tabs: the other paste shortcuts run after it and leave a key press it took
/// ([`TabClipboard::claimed`]) alone.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct TabPaste;

/// A tab (or features of a Part Studio) on the clipboard.
#[derive(Clone)]
pub struct CopiedTab {
    /// Tells one copy from the next.
    key: u64,
    kind: Kind,
    /// The tab's name (the feature's, or "3 features") and its document's.
    pub tab: String,
    pub document: String,
    package: Arc<native::Package>,
}

impl CopiedTab {
    /// Features of a Part Studio, not a tab.
    pub fn is_features(&self) -> bool {
        self.kind == Kind::Features
    }
}

/// The tab clipboard: what the system clipboard was last seen to hold.
#[derive(Resource, Default)]
pub struct TabClipboard {
    known: Option<CopiedTab>,
    /// The copy that something copied in this cadrs afterwards took Ctrl+V from.
    superseded: Option<u64>,
    /// Set by a copy of something else in this cadrs (see the module docs).
    pub local_copy: bool,
    /// This frame's Ctrl+V pasted from here: the other paste shortcuts leave it alone.
    pub claimed: bool,
    /// A read under way (a browser's), and whether what it holds is pasted when it comes.
    reading: Option<(ClipboardRead, bool)>,
}

fn key_of(text: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

fn parse(text: &str) -> Option<CopiedTab> {
    let (kind, bytes) = tab_package::from_text(text)?;
    let package = native::unpack(&bytes)
        .map_err(|e| warn!("the tab on the clipboard: {e}"))
        .ok()?;
    let doc = &package.file.document;
    let tab = match kind {
        Kind::Tab => {
            let roots = tab_package::roots(doc);
            roots
                .first()
                .or(doc.elements.first().as_ref())?
                .name
                .clone()
        }
        Kind::Features => match tab_package::package_features(doc) {
            [] => return None,
            [one] => one.name.clone(),
            many => tr_with(
                "shell-doc-features-count",
                &[("count", &many.len().to_string())],
            ),
        },
    };
    Some(CopiedTab {
        key: key_of(text),
        kind,
        tab,
        document: doc.name.clone(),
        package: Arc::new(package),
    })
}

impl TabClipboard {
    fn see(&mut self, text: &str) {
        if tab_package::kind_of(text).is_none() {
            self.known = None;
        } else if self.known.as_ref().is_none_or(|k| k.key != key_of(text)) {
            self.known = parse(text);
        }
    }

    /// What the clipboard holds now, if it is a tab or features.
    pub fn peek(&mut self, clipboard: Option<&mut Clipboard>) -> Option<CopiedTab> {
        if self.reading.is_none()
            && let Some(clipboard) = clipboard
        {
            let mut read = clipboard.fetch_text();
            match read.poll_result() {
                Some(Ok(text)) => self.see(&text),
                // No system clipboard: the copy made here is all there is.
                Some(Err(ClipboardError::ClipboardNotSupported)) => {}
                Some(Err(_)) => self.known = None,
                None => self.reading = Some((read, false)),
            }
        }
        self.known.clone()
    }

    /// What Ctrl+V pastes now (see the module docs).
    fn for_shortcut(&mut self, clipboard: Option<&mut Clipboard>) -> Option<CopiedTab> {
        let tab = self.peek(clipboard);
        if let Some((_, paste)) = self.reading.as_mut() {
            *paste = true;
        }
        tab.filter(|t| self.superseded != Some(t.key))
    }
}

/// What the clipboard holds, for a menu's "Paste <tab> from <document>".
pub fn peek(world: &mut World) -> Option<CopiedTab> {
    world.resource_scope(|world, mut tabs: Mut<TabClipboard>| {
        tabs.peek(world.get_resource_mut::<Clipboard>().as_deref_mut())
    })
}

/// A menu's Paste item's text for `tab`.
pub fn paste_label(tab: &CopiedTab) -> String {
    tr_with(
        "shell-doc-paste-tab-from",
        &[
            ("name", tab.tab.as_str()),
            ("document", tab.document.as_str()),
        ],
    )
}

/// The `.cadrs` package of the tab `tab` of the open document, or of only `features` of it;
/// with the tab's name.
fn pack(
    world: &World,
    tab: ElementId,
    features: Option<&[FeatureId]>,
) -> Result<(String, Vec<u8>), String> {
    let doc = world
        .get_resource::<ActiveDocument>()
        .ok_or("No document is open")?;
    let package = match features {
        Some(f) => tab_package::features_document(&doc.doc, tab, f),
        None => tab_package::tab_document(&doc.doc, tab),
    }
    .ok_or("There is nothing to copy")?;
    let name = package.elements[0].name.clone();
    let meta = doc.meta.clone().unwrap_or_else(|| {
        cadrs_core::DocumentMeta::new(
            &world.resource::<UserProfile>().id,
            world.resource::<AppClock>().now(),
        )
    });
    let bytes = native::pack(&package, &meta, &world.resource::<DocumentStore>().0, false)?;
    Ok((name, bytes))
}

/// **Copy** (a tab's menu): the tab goes to the clipboard.
pub fn copy(world: &mut World, tab: ElementId) {
    if crate::collab::lacks(world, |r| r.copy, "copy") {
        return;
    }
    match pack(world, tab, None) {
        Ok((_, bytes)) => put(world, tab_package::to_text(Kind::Tab, &bytes)),
        Err(e) => crate::linked::error_toast(world, e),
    }
}

/// **Copy** (a feature's menu, Ctrl+C): `features` of the active Part Studio go to the
/// clipboard, with the folders that are wholly among them.
pub fn copy_features(world: &mut World, features: &[FeatureId]) {
    if crate::collab::lacks(world, |r| r.copy, "copy") {
        return;
    }
    let Some(tab) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active)
    else {
        return;
    };
    match pack(world, tab, Some(features)) {
        Ok((_, bytes)) => put(world, tab_package::to_text(Kind::Features, &bytes)),
        Err(e) => crate::linked::error_toast(world, e),
    }
}

/// **Copy** (a feature folder's menu): the folder with its features.
pub fn copy_folder(world: &mut World, folder: FeatureId) {
    let features = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| {
            let folders = d.active_element()?.folders();
            Some(folders.iter().find(|f| f.id == folder)?.features.clone())
        })
        .unwrap_or_default();
    copy_features(world, &features);
}

/// Puts a package's text on the clipboard and says what was copied.
fn put(world: &mut World, text: String) {
    let copied = parse(&text);
    let name = copied.as_ref().map(|c| c.tab.clone()).unwrap_or_default();
    if let Some(mut clip) = world.get_resource_mut::<Clipboard>()
        && let Err(e) = clip.set_text(text)
        && !matches!(e, ClipboardError::ClipboardNotSupported)
    {
        warn!("clipboard: {e:?}");
    }
    let mut tabs = world.resource_mut::<TabClipboard>();
    tabs.known = copied;
    tabs.superseded = None;
    tabs.local_copy = false;
    crate::linked::toast(world, tr_with("shell-doc-tab-copied", &[("name", &name)]));
}

/// **Paste**: the tab on the clipboard, right of the active tab, opened; or the features on
/// it, into the active Part Studio.
pub fn paste(world: &mut World) {
    match peek(world) {
        Some(tab) => paste_copied(world, &tab),
        // (A browser's read: it is pasted when it comes.)
        None => {
            if let Some((_, paste)) = world.resource_mut::<TabClipboard>().reading.as_mut() {
                *paste = true;
            }
        }
    }
}

fn paste_copied(world: &mut World, copied: &CopiedTab) {
    match copied.kind {
        Kind::Tab => {
            paste_package(world, &copied.package);
        }
        Kind::Features => paste_features(world, &copied.package),
    }
}

/// Adds a package's features to the active Part Studio (one undo step) and selects them.
fn paste_features(world: &mut World, package: &native::Package) {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc.active else {
        return;
    };
    let command = match tab_package::paste_features(&doc.doc, element, &package.file.document) {
        Ok(c) => c,
        Err(e) => return crate::linked::error_toast(world, e),
    };
    // The files the features read (imports, pictures): saved with this document from now on.
    for (_, bytes) in &package.assets {
        cadrs_core::blobs::insert(bytes.clone());
    }
    match doc.execute(&command) {
        Ok(()) => {
            world.resource_mut::<Selection>().0 = command
                .features()
                .iter()
                .map(|f| Pick::Feature(f.id))
                .collect();
        }
        Err(e) => crate::linked::error_toast(world, e.to_string()),
    }
}

/// Adds a package's tabs to the open document (one undo step) and opens the first. Returns how
/// many tabs came in.
fn paste_package(world: &mut World, package: &native::Package) -> usize {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return 0;
    };
    let Some(paste) = tab_package::paste(&doc.doc, &package.file.document, doc.active) else {
        return 0;
    };
    // The files its features read (imports, pictures): saved with this document from now on.
    for (_, bytes) in &package.assets {
        cadrs_core::blobs::insert(bytes.clone());
    }
    match doc.execute(&paste.command) {
        Ok(()) => {
            doc.set_active(paste.open);
            paste.command.elements.len()
        }
        Err(e) => {
            crate::linked::error_toast(world, e.to_string());
            0
        }
    }
}

/// **+ ▸ Import…** of a `.cadrs` file: its tabs as new tabs of the open document.
pub fn import_file(world: &mut World, path: &Path) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match native::read(path) {
        Ok(package) => {
            if paste_package(world, &package) > 0 {
                crate::linked::toast(world, tr_with("io-imported-name", &[("name", &name)]));
            }
        }
        Err(e) => crate::linked::error_toast(world, tr_with("io-import-failed", &[("error", &e)])),
    }
}

/// The Export dialog's CADRS format: the tab `tab` as `<base>.cadrs` in `dir` (the next free
/// name there).
pub fn export_file(
    world: &World,
    tab: ElementId,
    dir: &Path,
    base: &str,
) -> Result<PathBuf, String> {
    let (name, bytes) = pack(world, tab, None)?;
    let base = base.trim();
    let base = if base.to_ascii_lowercase().ends_with(".cadrs") {
        &base[..base.len() - 6]
    } else {
        base
    };
    let base = if base.is_empty() { name.as_str() } else { base };
    let path =
        cadrs_core::export::unique_path_ext(dir, &cadrs_core::export::sanitize(base), "cadrs");
    native::write_new(&path, &bytes)?;
    Ok(path)
}

/// Ctrl held alone (or Cmd), and whether a text field has the keyboard.
fn ctrl_alone(keys: &ButtonInput<KeyCode>) -> bool {
    keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]) && !keys.any_pressed([
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
    ])
}

/// Ctrl+V: pastes the tab or the features on the clipboard (see the module docs).
#[allow(clippy::too_many_arguments)]
fn shortcut(
    mut keys_in: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    q_fields: Query<(), With<cadrs_ui::KeyboardField>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    doc: Option<Res<ActiveDocument>>,
    sketch: Option<Res<crate::sketch::SketchSession>>,
    mut tabs: ResMut<TabClipboard>,
    mut clipboard: Option<ResMut<Clipboard>>,
    mut commands: Commands,
) {
    if tabs.claimed {
        tabs.claimed = false;
    }
    let ctrl = ctrl_alone(&keys);
    let typing = focus.get().is_some_and(|e| q_fields.contains(e));
    for k in keys_in.read() {
        if k.state != ButtonState::Pressed
            || k.repeat
            || !ctrl
            || typing
            || !q_dialogs.is_empty()
            || k.shortcut_code() != KeyCode::KeyV
        {
            continue;
        }
        // Features go into a Part Studio, and not while one of its sketches is edited.
        let studio = sketch.is_none()
            && doc
                .as_ref()
                .and_then(|d| d.active_element())
                .is_some_and(|e| matches!(e.kind, ElementKind::PartStudio { .. }));
        if let Some(tab) = tabs
            .for_shortcut(clipboard.as_deref_mut())
            .filter(|t| !t.is_features() || studio)
        {
            tabs.claimed = true;
            commands.queue(move |world: &mut World| paste_copied(world, &tab));
        }
    }
}

/// Ctrl+C with features selected in a Part Studio's feature list: they go to the clipboard.
#[allow(clippy::too_many_arguments)]
fn copy_shortcut(
    mut keys_in: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    q_fields: Query<(), With<cadrs_ui::KeyboardField>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    sketch: Option<Res<crate::sketch::SketchSession>>,
    selection: Res<Selection>,
    mut commands: Commands,
) {
    let ctrl = ctrl_alone(&keys);
    let typing = focus.get().is_some_and(|e| q_fields.contains(e));
    for k in keys_in.read() {
        if k.state != ButtonState::Pressed
            || k.repeat
            || !ctrl
            || typing
            || sketch.is_some()
            || !q_dialogs.is_empty()
            || k.shortcut_code() != KeyCode::KeyC
        {
            continue;
        }
        let features: Vec<FeatureId> = selection
            .0
            .iter()
            .filter_map(|p| match p {
                Pick::Feature(f) => Some(*f),
                _ => None,
            })
            .collect();
        if !features.is_empty() {
            commands.queue(move |world: &mut World| copy_features(world, &features));
        }
    }
}

/// Something else was copied in this cadrs: Ctrl+V is its, until a tab is copied again.
fn note_local_copy(mut tabs: ResMut<TabClipboard>, mut clipboard: Option<ResMut<Clipboard>>) {
    if !tabs.local_copy {
        return;
    }
    tabs.local_copy = false;
    tabs.superseded = tabs.peek(clipboard.as_deref_mut()).map(|t| t.key);
}

/// A browser's clipboard read comes back.
fn finish_read(world: &mut World) {
    let mut tabs = world.resource_mut::<TabClipboard>();
    let Some((read, _)) = tabs.reading.as_mut() else {
        return;
    };
    let Some(result) = read.poll_result() else {
        return;
    };
    let (_, paste) = tabs.reading.take().expect("just polled");
    let before = tabs.known.as_ref().map(|k| k.key);
    match result {
        Ok(text) => tabs.see(&text),
        Err(ClipboardError::ClipboardNotSupported) => {}
        Err(_) => tabs.known = None,
    }
    // Only what wasn't known when Ctrl+V was pressed: what was known was pasted then.
    let new = tabs
        .known
        .clone()
        .filter(|t| paste && before != Some(t.key) && tabs.superseded != Some(t.key));
    if let Some(tab) = new
        && world.get_resource::<ActiveDocument>().is_some()
    {
        paste_copied(world, &tab);
    }
}
