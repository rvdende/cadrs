//! Tabs between documents and between running cadrs: **Copy** in a tab's menu, **Paste** (the
//! tab bar's menu, its **+** menu, or Ctrl+V), and a tab as a `.cadrs` file (the Export
//! dialog's CADRS format, **+ ▸ Import…**). The model is [`cadrs_core::tab_package`].
//!
//! - **Copy** puts the tab on the system clipboard as text (its `.cadrs` package: the features,
//!   the tabs it references and the files it uses), so another cadrs reads it. Without a system
//!   clipboard (a machine with no display) the copy is still kept here.
//! - **Paste** adds it right of the active tab and opens it: one undo step. The menus name it
//!   "Paste <tab> from <document>".
//! - **Ctrl+V** pastes the tab unless something was copied in this cadrs since (sketch
//!   entities, assembly instances, schematic items: [`TabClipboard::local_copy`]); those paste
//!   as before then. In a browser the clipboard is read in the background, so a tab copied in
//!   another cadrs is pasted when the read comes back.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy::clipboard::{Clipboard, ClipboardError, ClipboardRead};
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use cadrs_core::{ElementId, native, tab_package};
use cadrs_ui::ShortcutKey;
use cadrs_ui::i18n::tr_with;

use crate::{ActiveDocument, AppClock, AppState, DocumentStore, UserProfile};

pub struct TabClipboardPlugin;

impl Plugin for TabClipboardPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TabClipboard>().add_systems(
            Update,
            (
                shortcut
                    .in_set(TabPaste)
                    .run_if(in_state(AppState::Document)),
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

/// A tab on the clipboard.
#[derive(Clone)]
pub struct CopiedTab {
    /// Tells one copy from the next.
    key: u64,
    /// The tab's name and its document's.
    pub tab: String,
    pub document: String,
    package: Arc<native::Package>,
}

/// The tab clipboard: what the system clipboard was last seen to hold.
#[derive(Resource, Default)]
pub struct TabClipboard {
    known: Option<CopiedTab>,
    /// The copy that something copied in this cadrs afterwards took Ctrl+V from.
    superseded: Option<u64>,
    /// Set by a copy of something else in this cadrs (see the module docs).
    pub local_copy: bool,
    /// This frame's Ctrl+V pasted a tab: the other paste shortcuts leave it alone.
    pub claimed: bool,
    /// A read under way (a browser's), and whether its tab is pasted when it comes.
    reading: Option<(ClipboardRead, bool)>,
}

fn key_of(text: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

fn parse(text: &str) -> Option<CopiedTab> {
    let package = native::unpack(&tab_package::from_text(text)?)
        .map_err(|e| warn!("the tab on the clipboard: {e}"))
        .ok()?;
    let doc = &package.file.document;
    let roots = tab_package::roots(doc);
    Some(CopiedTab {
        key: key_of(text),
        tab: roots
            .first()
            .or(doc.elements.first().as_ref())?
            .name
            .clone(),
        document: doc.name.clone(),
        package: Arc::new(package),
    })
}

impl TabClipboard {
    fn see(&mut self, text: &str) {
        if !tab_package::is_text(text) {
            self.known = None;
        } else if self.known.as_ref().is_none_or(|k| k.key != key_of(text)) {
            self.known = parse(text);
        }
    }

    /// The tab on the clipboard now, if it holds one.
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

    /// The tab Ctrl+V pastes now (see the module docs).
    fn for_shortcut(&mut self, clipboard: Option<&mut Clipboard>) -> Option<CopiedTab> {
        let tab = self.peek(clipboard);
        if let Some((_, paste)) = self.reading.as_mut() {
            *paste = true;
        }
        tab.filter(|t| self.superseded != Some(t.key))
    }
}

/// The tab on the clipboard, for a menu's "Paste <tab> from <document>".
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

/// The `.cadrs` package of the tab `tab` of the open document.
fn pack(world: &World, tab: ElementId) -> Result<(String, Vec<u8>), String> {
    let doc = world
        .get_resource::<ActiveDocument>()
        .ok_or("No document is open")?;
    let package = tab_package::tab_document(&doc.doc, tab).ok_or("The tab is gone")?;
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
    let (name, bytes) = match pack(world, tab) {
        Ok(p) => p,
        Err(e) => return crate::linked::error_toast(world, e),
    };
    let text = tab_package::to_text(&bytes);
    let copied = parse(&text);
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

/// **Paste**: the tab on the clipboard, right of the active tab, opened.
pub fn paste(world: &mut World) {
    match peek(world) {
        Some(tab) => {
            paste_package(world, &tab.package);
        }
        // (A browser's read: the tab is pasted when it comes.)
        None => {
            if let Some((_, paste)) = world.resource_mut::<TabClipboard>().reading.as_mut() {
                *paste = true;
            }
        }
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
    let (name, bytes) = pack(world, tab)?;
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

/// Ctrl+V: pastes the tab on the clipboard (see the module docs).
#[allow(clippy::too_many_arguments)]
fn shortcut(
    mut keys_in: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    q_fields: Query<(), With<cadrs_ui::KeyboardField>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    mut tabs: ResMut<TabClipboard>,
    mut clipboard: Option<ResMut<Clipboard>>,
    mut commands: Commands,
) {
    if tabs.claimed {
        tabs.claimed = false;
    }
    let ctrl = keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    let other = keys.any_pressed([
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
    ]);
    let typing = focus.get().is_some_and(|e| q_fields.contains(e));
    for k in keys_in.read() {
        if k.state != ButtonState::Pressed
            || k.repeat
            || !ctrl
            || other
            || typing
            || !q_dialogs.is_empty()
            || k.shortcut_code() != KeyCode::KeyV
        {
            continue;
        }
        if let Some(tab) = tabs.for_shortcut(clipboard.as_deref_mut()) {
            tabs.claimed = true;
            commands.queue(move |world: &mut World| {
                paste_package(world, &tab.package);
            });
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
    // Only a tab that wasn't known when Ctrl+V was pressed: a known one was pasted then.
    let new = tabs
        .known
        .clone()
        .filter(|t| paste && before != Some(t.key) && tabs.superseded != Some(t.key));
    if let Some(tab) = new
        && world.get_resource::<ActiveDocument>().is_some()
    {
        paste_package(world, &tab.package);
    }
}
