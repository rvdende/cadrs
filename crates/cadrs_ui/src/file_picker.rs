//! File prompts: the system's own dialog in a person's runs ([`NativeFileDialogs`], issue #136:
//! the Windows file dialog, the macOS Open panel, the desktop portal's on Linux), else this
//! in-app picker, which scripted and headless runs use because a scenario can drive it. Either
//! way the pick arrives as the same [`FilePicked`] or [`FilesPicked`] message.
//!
//! In a page (the browser build) the system's dialog is the browser's file chooser: the chosen
//! files are copied into the page's files, in [`UPLOADS`], and those are the paths picked. A
//! browser can't give a folder, so a folder prompt there is the in-app picker's.
//!
//! The in-app picker (P3C.7: Insert DXF/DWG and Insert image) is a modal [`Dialog`] like
//! gpui-component's file prompt: the folder in an editable path field with an Up button, the
//! folder's subfolders and the files of the wanted types in a scrolling list (folders first,
//! then files by name), and a File name field. A click on a folder opens it; a click on a file
//! puts its name in the field; a double-click on a file, Open or Enter picks it.
//!
//! [`open_file_picker`] opens it with a tag; the pick arrives as a [`FilePicked`] message
//! carrying the tag and the path. [`open_files_picker`] picks several files at once (P3H.3:
//! Import ECAD files takes a `.emn` and its `.emp`), like a browser's `<input multiple>`: a click
//! picks one file, Ctrl+click (or Shift+click) adds or removes a file, and the File name field
//! lists the picked files quoted (`"a.emn" "a.emp"`, which can also be typed); the pick arrives
//! as one [`FilesPicked`] message. Names: `<name>` (the dialog), `<name>-folder`, `<name>-up`,
//! `<name>-list`, `<name>-row-<file name>`, `<name>-name`, `<name>-open`, `<name>-cancel`.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc;

use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight, TextEdit};
use bevy::ui_widgets::{Activate, observe};

use crate::button::{Button, IconButton};
use crate::dialog::{Dialog, DialogClose};
use crate::i18n::{Translated, tr, tr_with};
use crate::inline_edit::{DoubleClick, DoubleClickable};
use crate::input::{TextInput, TextInputField, TextSubmit};
use crate::list::ListItem;
use crate::theme::Theme;

pub struct FilePickerPlugin;

impl Plugin for FilePickerPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<FilePicked>()
            .add_message::<FilesPicked>()
            .init_resource::<NativeFileDialogs>()
            .init_resource::<NativeJobs>()
            .add_systems(Update, (rebuild_lists, take_native_picks))
            .add_observer(on_submit);
    }
}

/// Where a page puts the files chosen in the browser's file chooser.
pub const UPLOADS: &str = "/Uploads";

/// Whether file prompts open the system's own dialog instead of the in-app picker. Off unless
/// the app turns it on (`cadrs` does for a person's runs, not for scripted or headless ones).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeFileDialogs(pub bool);

/// A system dialog open now: its answer (the picked paths, or none when cancelled) arrives on
/// `answer` from the thread that waits for it.
struct NativeJob {
    tag: String,
    multi: bool,
    answer: Mutex<mpsc::Receiver<Option<Vec<PathBuf>>>>,
}

#[derive(Resource, Default)]
struct NativeJobs(Vec<NativeJob>);

/// A file was picked in the picker opened with `tag`.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct FilePicked {
    pub tag: String,
    pub path: PathBuf,
}

/// Several files were picked in a multi-select picker ([`open_files_picker`]) opened with
/// `tag`, in the order they were picked.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct FilesPicked {
    pub tag: String,
    pub paths: Vec<PathBuf>,
}

/// The picker's state (on its dialog root).
#[derive(Component, Debug, Clone)]
pub struct FilePickerState {
    pub name: String,
    pub tag: String,
    pub dir: PathBuf,
    /// Lower-case extensions without the dot (`["dxf", "dwg"]`); empty for every file.
    pub extensions: Vec<String>,
    pub selected: Option<String>,
    /// Picks a folder (the one shown) instead of a file: the list holds folders only.
    pub folders_only: bool,
    /// Picks several files ([`open_files_picker`]).
    pub multi: bool,
    /// A multi-select picker's picked files (names in the folder shown), in pick order.
    pub picked: Vec<String>,
    /// What the list shows now (folder and selection).
    shown: Option<String>,
}

/// Splits a File name field's text into names: `"a b.emn" "c.emp"` → `a b.emn`, `c.emp`; text
/// without quotes is one name.
pub fn split_names(text: &str) -> Vec<String> {
    let t = text.trim();
    if !t.contains('"') {
        return if t.is_empty() {
            vec![]
        } else {
            vec![t.to_string()]
        };
    }
    t.split('"')
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, n)| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .collect()
}

/// The File name field's text for picked names: the one name, or each name quoted.
pub fn join_names(names: &[String]) -> String {
    match names {
        [one] => one.clone(),
        many => many
            .iter()
            .map(|n| format!("\"{n}\""))
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// One entry of a folder: (name, is a folder, size in bytes).
pub fn list_dir(dir: &Path, extensions: &[String]) -> Vec<(String, bool, u64)> {
    let Ok(rd) = cadrs_fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, bool, u64)> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let meta = e.metadata().ok()?;
            if meta.is_dir() {
                return Some((name, true, 0));
            }
            let ext = Path::new(&name)
                .extension()
                .map(|x| x.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            (extensions.is_empty() || extensions.contains(&ext)).then_some((
                name,
                false,
                meta.len(),
            ))
        })
        .collect();
    out.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    out
}

fn size_label(n: u64) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    }
}

/// Opens a file picker named `name` in `dir`, for files with `extensions`; the pick comes back
/// as [`FilePicked`] with `tag`.
pub fn open_file_picker(
    commands: &mut Commands,
    theme: &Theme,
    name: impl Into<Cow<'static, str>>,
    title: &str,
    tag: &str,
    dir: PathBuf,
    extensions: &[&str],
) {
    open_picker(
        commands,
        theme,
        name.into().into_owned(),
        title,
        tag,
        dir,
        extensions,
        false,
        false,
    );
}

/// Opens a picker for several files at once (see the module docs); the pick comes back as
/// [`FilesPicked`] with `tag`.
pub fn open_files_picker(
    commands: &mut Commands,
    theme: &Theme,
    name: impl Into<Cow<'static, str>>,
    title: &str,
    tag: &str,
    dir: PathBuf,
    extensions: &[&str],
) {
    open_picker(
        commands,
        theme,
        name.into().into_owned(),
        title,
        tag,
        dir,
        extensions,
        false,
        true,
    );
}

/// Opens a folder picker (Browse… for a folder): the list shows subfolders, a click opens one,
/// and Select folder picks the folder shown. The pick arrives as [`FilePicked`] with `tag`.
pub fn open_folder_picker(
    commands: &mut Commands,
    theme: &Theme,
    name: impl Into<Cow<'static, str>>,
    title: &str,
    tag: &str,
    dir: PathBuf,
) {
    open_picker(
        commands,
        theme,
        name.into().into_owned(),
        title,
        tag,
        dir,
        &[],
        true,
        false,
    );
}

/// The user's home folder (`USERPROFILE` on Windows, where `HOME` isn't set: issue #136).
pub fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
}

/// The folder as the picker shows it (P3E.2, P3E.1 judge: goldens mustn't depend on where the
/// working folder is): relative to the working folder when inside it (`./target/…`), else with
/// the home folder as `~`, else absolute.
pub fn display_dir(dir: &std::path::Path) -> String {
    let rel = |base: Option<PathBuf>, prefix: &str| {
        let base = base.and_then(|b| cadrs_fs::canonicalize(&b).ok().or(Some(b)))?;
        let r = dir.strip_prefix(&base).ok()?;
        Some(if r.as_os_str().is_empty() {
            prefix.to_string()
        } else {
            format!("{prefix}/{}", r.display())
        })
    };
    rel(std::env::current_dir().ok(), ".")
        .or_else(|| rel(home_dir(), "~"))
        .unwrap_or_else(|| dir.display().to_string())
}

/// Opens the system's dialog or the in-app picker, as [`NativeFileDialogs`] says.
#[allow(clippy::too_many_arguments)]
fn open_picker(
    commands: &mut Commands,
    theme: &Theme,
    name: String,
    title: &str,
    tag: &str,
    dir: PathBuf,
    extensions: &[&str],
    folders_only: bool,
    multi: bool,
) {
    let theme = theme.clone();
    let (title, tag) = (title.to_string(), tag.to_string());
    let extensions: Vec<String> = extensions.iter().map(|e| e.to_string()).collect();
    commands.queue(move |world: &mut World| {
        // A browser can't give a folder.
        if world.resource::<NativeFileDialogs>().0
            && !(folders_only && cfg!(target_arch = "wasm32"))
        {
            open_native(world, &title, tag, dir, &extensions, folders_only, multi);
            return;
        }
        let extensions: Vec<&str> = extensions.iter().map(String::as_str).collect();
        let mut commands = world.commands();
        spawn_in_app_picker(
            &mut commands,
            &theme,
            name,
            &title,
            &tag,
            dir,
            &extensions,
            folders_only,
            multi,
        );
        world.flush();
    });
}

/// Opens the system's file dialog on a thread of its own and keeps its answer coming
/// ([`take_native_picks`]). A second prompt for the same `tag` while one is open is ignored.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn open_native(
    world: &mut World,
    title: &str,
    tag: String,
    dir: PathBuf,
    extensions: &[String],
    folders_only: bool,
    multi: bool,
) {
    if world
        .resource::<NativeJobs>()
        .0
        .iter()
        .any(|j| j.tag == tag)
    {
        return;
    }
    let mut dialog = rfd::AsyncFileDialog::new().set_title(title);
    if dir.is_dir() {
        dialog = dialog.set_directory(&dir);
    }
    if !folders_only && !extensions.is_empty() {
        // Both cases: on Linux the portal's patterns are case-sensitive (PART.STEP).
        let mut patterns: Vec<String> = extensions.to_vec();
        patterns.extend(extensions.iter().map(|e| e.to_uppercase()));
        let name = extensions
            .iter()
            .map(|e| e.to_uppercase())
            .collect::<Vec<_>>()
            .join(", ");
        dialog = dialog.add_filter(name, &patterns);
    }
    // On Windows the dialog belongs to the app's window: it stays in front and the window waits
    // for it.
    #[cfg(windows)]
    {
        let mut q = world
            .query_filtered::<&bevy::window::RawHandleWrapper, With<bevy::window::PrimaryWindow>>();
        if let Some(handle) = q.iter(world).next() {
            // SAFETY: the handle is only read here, to give the dialog its owner window.
            dialog = dialog.set_parent(&unsafe { handle.get_handle() });
        }
    }
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("file-dialog".into())
        .spawn(move || {
            let picked = bevy::tasks::block_on(async move {
                let path = |h: rfd::FileHandle| h.path().to_path_buf();
                if folders_only {
                    dialog.pick_folder().await.map(|h| vec![path(h)])
                } else if multi {
                    dialog
                        .pick_files()
                        .await
                        .map(|v| v.into_iter().map(path).collect())
                } else {
                    dialog.pick_file().await.map(|h| vec![path(h)])
                }
            });
            let _ = tx.send(picked);
        });
    if let Err(e) = spawned {
        warn!("cannot open the file dialog: {e}");
        return;
    }
    world.resource_mut::<NativeJobs>().0.push(NativeJob {
        tag,
        multi,
        answer: Mutex::new(rx),
    });
}

/// The browser's file chooser, for files (see the module docs): the chosen files are read into
/// [`UPLOADS`] and the answer comes as a native one does ([`take_native_picks`]).
#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments)]
fn open_native(
    world: &mut World,
    title: &str,
    tag: String,
    _dir: PathBuf,
    extensions: &[String],
    _folders_only: bool,
    multi: bool,
) {
    if world
        .resource::<NativeJobs>()
        .0
        .iter()
        .any(|j| j.tag == tag)
    {
        return;
    }
    let mut dialog = rfd::AsyncFileDialog::new().set_title(title);
    if !extensions.is_empty() {
        let name = extensions
            .iter()
            .map(|e| e.to_uppercase())
            .collect::<Vec<_>>()
            .join(", ");
        dialog = dialog.add_filter(name, extensions);
    }
    let (tx, rx) = mpsc::channel();
    wasm_bindgen_futures::spawn_local(async move {
        let chosen = if multi {
            dialog.pick_files().await
        } else {
            dialog.pick_file().await.map(|h| vec![h])
        };
        let mut paths = Vec::new();
        for file in chosen.into_iter().flatten() {
            let path = Path::new(UPLOADS).join(file.file_name());
            let bytes = file.read().await;
            match cadrs_fs::create_dir_all(UPLOADS).and_then(|()| cadrs_fs::write(&path, bytes)) {
                Ok(()) => paths.push(path),
                Err(e) => warn!("cannot keep {}: {e}", path.display()),
            }
        }
        let _ = tx.send((!paths.is_empty()).then_some(paths));
    });
    world.resource_mut::<NativeJobs>().0.push(NativeJob {
        tag,
        multi,
        answer: Mutex::new(rx),
    });
}

/// Turns a system dialog's answer into the same message the in-app picker sends; a cancelled
/// dialog sends nothing.
fn take_native_picks(
    mut jobs: ResMut<NativeJobs>,
    mut one: MessageWriter<FilePicked>,
    mut many: MessageWriter<FilesPicked>,
) {
    if jobs.0.is_empty() {
        return;
    }
    jobs.0.retain(|job| {
        let answer = match job.answer.lock() {
            Ok(rx) => rx.try_recv(),
            Err(_) => return false,
        };
        match answer {
            Err(mpsc::TryRecvError::Empty) => true,
            Ok(Some(paths)) if job.multi => {
                many.write(FilesPicked {
                    tag: job.tag.clone(),
                    paths,
                });
                false
            }
            Ok(Some(paths)) => {
                if let Some(path) = paths.into_iter().next() {
                    one.write(FilePicked {
                        tag: job.tag.clone(),
                        path,
                    });
                }
                false
            }
            Ok(None) | Err(mpsc::TryRecvError::Disconnected) => false,
        }
    });
}

/// The in-app picker's dialog.
#[allow(clippy::too_many_arguments)]
fn spawn_in_app_picker(
    commands: &mut Commands,
    theme: &Theme,
    name: String,
    title: &str,
    tag: &str,
    dir: PathBuf,
    extensions: &[&str],
    folders_only: bool,
    multi: bool,
) {
    let dir = cadrs_fs::canonicalize(&dir).unwrap_or(dir);
    let t = theme.clone();
    let (tb, tf) = (t.clone(), t.clone());
    let (n1, n2) = (name.clone(), name.clone());
    let dir_text = display_dir(&dir);
    let types = if extensions.is_empty() {
        tr("ui-file-picker-all-files")
    } else {
        extensions
            .iter()
            .map(|e| format!("*.{e}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    commands.spawn((
        Dialog::new(name.clone())
            .title(title.to_string())
            .width(560.0)
            .body(move |b| {
                let t = &tb;
                let n = &n1;
                b.spawn(Node {
                    column_gap: Val::Px(6.0),
                    align_items: AlignItems::Center,
                    width: Val::Percent(100.0),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        t.text(
                            tr("ui-file-picker-look-in"),
                            t.font_base,
                            FontWeight::MEDIUM,
                            t.foreground,
                        ),
                        Translated("ui-file-picker-look-in"),
                    ));
                    row.spawn(
                        TextInput::new(format!("{n}-folder"))
                            .value(dir_text)
                            .width(Val::Percent(100.0))
                            .height(30.0)
                            .build(t),
                    );
                    let nn = n.clone();
                    row.spawn((
                        IconButton::new(format!("{n}-up"), "arrow-up")
                            .tooltip(tr("ui-file-picker-up"))
                            .build(t),
                        observe(move |_: On<Activate>, mut commands: Commands| {
                            let nn = nn.clone();
                            commands.queue(move |w: &mut World| {
                                update(w, &nn, |s| {
                                    if let Some(p) = s.dir.parent() {
                                        s.dir = p.to_path_buf();
                                        s.selected = None;
                                    }
                                })
                            });
                        }),
                    ));
                });
                b.spawn((
                    Name::new(format!("{n}-list-frame")),
                    Node {
                        height: Val::Px(260.0),
                        width: Val::Percent(100.0),
                        margin: UiRect::vertical(Val::Px(8.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgb_u8(0xd0, 0xd0, 0xd0)),
                ))
                .with_children(|f| {
                    let list = f
                        .spawn((
                            Name::new(format!("{n}-list")),
                            bevy::ui_widgets::ScrollArea,
                            Node {
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                overflow: Overflow::scroll_y(),
                                padding: UiRect::right(Val::Px(14.0)),
                                ..default()
                            },
                        ))
                        .id();
                    f.spawn(crate::scrollbar::vertical_scrollbar(
                        t,
                        &format!("{n}-list-scrollbar"),
                        list,
                    ));
                });
                if !folders_only {
                    b.spawn(Node {
                        column_gap: Val::Px(6.0),
                        align_items: AlignItems::Center,
                        width: Val::Percent(100.0),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            t.text(
                                tr("common-file-name"),
                                t.font_base,
                                FontWeight::MEDIUM,
                                t.foreground,
                            ),
                            Translated("common-file-name"),
                        ));
                        row.spawn(
                            TextInput::new(format!("{n}-name"))
                                .width(Val::Percent(100.0))
                                .height(30.0)
                                .build(t),
                        );
                    });
                }
                b.spawn((
                    Name::new(format!("{n}-types")),
                    t.text(
                        if folders_only {
                            tr("ui-file-picker-folders-hint")
                        } else if multi {
                            tr_with("ui-file-picker-types-multi", &[("types", &types)])
                        } else {
                            tr_with("ui-file-picker-types", &[("types", &types)])
                        },
                        t.font_sm,
                        FontWeight::NORMAL,
                        t.muted_foreground,
                    ),
                ));
            })
            .footer(move |f| {
                let t = &tf;
                let n = n2.clone();
                let n_open = n.clone();
                f.spawn((
                    Button::new(format!("{n}-open"))
                        .label(if folders_only {
                            tr("ui-file-picker-select-folder")
                        } else {
                            tr("common-open")
                        })
                        .primary()
                        .build(t),
                    observe(move |_: On<Activate>, mut commands: Commands| {
                        let n = n_open.clone();
                        commands.queue(move |w: &mut World| accept(w, &n));
                    }),
                ));
                let n_cancel = n.clone();
                f.spawn((
                    Button::new(format!("{n}-cancel"))
                        .label(tr("common-cancel"))
                        .build(t),
                    observe(
                        move |_: On<Activate>,
                              q: Query<(Entity, &FilePickerState)>,
                              mut commands: Commands| {
                            for (e, s) in &q {
                                if s.name == n_cancel {
                                    commands.trigger(DialogClose { entity: e });
                                }
                            }
                        },
                    ),
                ));
            })
            .build(theme),
        FilePickerState {
            name: name.clone(),
            tag: tag.to_string(),
            dir,
            extensions: extensions.iter().map(|e| e.to_lowercase()).collect(),
            selected: None,
            folders_only,
            multi,
            picked: Vec::new(),
            shown: None,
        },
    ));
}

fn update(w: &mut World, name: &str, f: impl FnOnce(&mut FilePickerState)) {
    let mut q = w.query::<&mut FilePickerState>();
    if let Some(mut s) = q.iter_mut(w).find(|s| s.name == name) {
        f(&mut s);
    }
}

fn field_text(w: &mut World, name: &str) -> Option<String> {
    let mut q = w.query_filtered::<(&Name, &EditableText), With<TextInputField>>();
    q.iter(w)
        .find(|(n, _)| n.as_str() == name)
        .map(|(_, t)| t.value().to_string())
}

fn set_field(w: &mut World, name: &str, value: &str) {
    let mut q = w.query_filtered::<(&Name, &mut EditableText), With<TextInputField>>();
    if let Some((_, mut t)) = q.iter_mut(w).find(|(n, _)| n.as_str() == name) {
        t.queue_edit(TextEdit::SelectAll);
        t.queue_edit(TextEdit::Insert(value.to_string().into()));
    }
}

/// Open (or Enter in the name field): a folder opens, a file is picked.
fn accept(w: &mut World, name: &str) {
    let mut q = w.query::<(Entity, &FilePickerState)>();
    let Some((root, state)) = q
        .iter(w)
        .find(|(_, s)| s.name == name)
        .map(|(e, s)| (e, s.clone()))
    else {
        return;
    };
    if state.folders_only {
        w.write_message(FilePicked {
            tag: state.tag.clone(),
            path: state.dir.clone(),
        });
        w.trigger(DialogClose { entity: root });
        return;
    }
    let typed = field_text(w, &format!("{name}-name-field")).unwrap_or_default();
    if state.multi {
        let names = split_names(&typed);
        let names = if names.is_empty() {
            state.picked.clone()
        } else {
            names
        };
        let paths: Vec<PathBuf> = names
            .iter()
            .map(|n| {
                if Path::new(n).is_absolute() {
                    PathBuf::from(n)
                } else {
                    state.dir.join(n)
                }
            })
            .collect();
        if let [one] = paths.as_slice()
            && cadrs_fs::is_dir(one)
        {
            let one = one.clone();
            update(w, name, |s| {
                s.dir = one;
                s.selected = None;
                s.picked.clear();
            });
            set_field(w, &format!("{name}-name-field"), "");
            return;
        }
        if paths.is_empty() || !paths.iter().all(cadrs_fs::is_file) {
            return;
        }
        w.write_message(FilesPicked {
            tag: state.tag.clone(),
            paths,
        });
        w.trigger(DialogClose { entity: root });
        return;
    }
    let typed = typed.trim();
    let chosen = if typed.is_empty() {
        state.selected.clone().unwrap_or_default()
    } else {
        typed.to_string()
    };
    if chosen.is_empty() {
        return;
    }
    let path = if Path::new(&chosen).is_absolute() {
        PathBuf::from(&chosen)
    } else {
        state.dir.join(&chosen)
    };
    if cadrs_fs::is_dir(&path) {
        update(w, name, |s| {
            s.dir = path;
            s.selected = None;
        });
        set_field(w, &format!("{name}-name-field"), "");
        return;
    }
    if !cadrs_fs::is_file(&path) {
        return;
    }
    w.write_message(FilePicked {
        tag: state.tag.clone(),
        path,
    });
    w.trigger(DialogClose { entity: root });
}

/// Enter in the folder field opens that folder; in the name field, accepts.
fn on_submit(
    ev: On<TextSubmit>,
    q: Query<&Name>,
    q_state: Query<&FilePickerState>,
    mut commands: Commands,
) {
    let Ok(n) = q.get(ev.entity) else { return };
    for s in &q_state {
        if n.as_str() == format!("{}-folder-field", s.name) {
            let (name, value) = (s.name.clone(), ev.value.clone());
            commands.queue(move |w: &mut World| {
                // "~/…" is the home folder (as the field shows it, [`display_dir`]).
                let v = value.trim();
                let p = match (v.strip_prefix('~'), home_dir()) {
                    (Some(rest), Some(home)) => home.join(rest.trim_start_matches(['/', '\\'])),
                    _ => PathBuf::from(v),
                };
                if cadrs_fs::is_dir(&p) {
                    update(w, &name, |s| {
                        s.dir = cadrs_fs::canonicalize(&p).unwrap_or(p);
                        s.selected = None;
                    });
                }
            });
        } else if n.as_str() == format!("{}-name-field", s.name) {
            let name = s.name.clone();
            commands.queue(move |w: &mut World| accept(w, &name));
        }
    }
}

/// Fills a picker's list when its folder or selection changes.
fn rebuild_lists(
    mut q: Query<(Entity, &mut FilePickerState)>,
    q_names: Query<(Entity, &Name)>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    for (_, mut state) in &mut q {
        let list_name = format!("{}-list", state.name);
        let Some((list, _)) = q_names.iter().find(|(_, n)| n.as_str() == list_name) else {
            continue;
        };
        let key = format!(
            "{}|{}|{}",
            state.dir.display(),
            state.selected.as_deref().unwrap_or(""),
            state.picked.join("|")
        );
        if state.shown.as_ref() == Some(&key) {
            continue;
        }
        state.shown = Some(key);
        commands.entity(list).despawn_related::<Children>();
        let mut entries = list_dir(&state.dir, &state.extensions);
        if state.folders_only {
            entries.retain(|e| e.1);
        }
        let t = theme.clone();
        let name = state.name.clone();
        let dir_text = display_dir(&state.dir);
        let selected = state.selected.clone();
        let picked = state.picked.clone();
        let multi = state.multi;
        commands.entity(list).with_children(|l| {
            if entries.is_empty() {
                l.spawn((
                    Name::new(format!("{name}-empty")),
                    t.text(
                        tr("ui-file-picker-empty"),
                        t.font_base,
                        FontWeight::NORMAL,
                        t.muted_foreground,
                    ),
                    Translated("ui-file-picker-empty"),
                    Node {
                        margin: UiRect::all(Val::Px(10.0)),
                        ..default()
                    },
                ));
            }
            for (file, is_dir, size) in entries {
                let row_name = format!("{name}-row-{file}");
                let item = ListItem::new(row_name)
                    .icon(if is_dir { "folder" } else { "file" })
                    .label(file.clone())
                    .height(26.0)
                    .selected(if multi {
                        picked.contains(&file)
                    } else {
                        selected.as_deref() == Some(file.as_str())
                    });
                let item = if is_dir {
                    item
                } else {
                    item.detail(size_label(size))
                };
                let (n1, f1, n2) = (name.clone(), file.clone(), name.clone());
                l.spawn((
                    item.build(&t),
                    DoubleClickable,
                    observe(move |_: On<Activate>, mut commands: Commands| {
                        let (n, f) = (n1.clone(), f1.clone());
                        commands.queue(move |w: &mut World| {
                            if is_dir {
                                update(w, &n, |s| {
                                    s.dir = s.dir.join(&f);
                                    s.selected = None;
                                    s.picked.clear();
                                });
                                set_field(w, &format!("{n}-name-field"), "");
                            } else if multi {
                                let add =
                                    w.get_resource::<ButtonInput<KeyCode>>().is_some_and(|k| {
                                        k.any_pressed([
                                            KeyCode::ControlLeft,
                                            KeyCode::ControlRight,
                                            KeyCode::ShiftLeft,
                                            KeyCode::ShiftRight,
                                        ])
                                    });
                                let mut names = Vec::new();
                                update(w, &n, |s| {
                                    if !add {
                                        s.picked = vec![f.clone()];
                                    } else if let Some(i) = s.picked.iter().position(|p| *p == f) {
                                        s.picked.remove(i);
                                    } else {
                                        s.picked.push(f.clone());
                                    }
                                    s.selected = s.picked.last().cloned();
                                    names = s.picked.clone();
                                });
                                set_field(w, &format!("{n}-name-field"), &join_names(&names));
                            } else {
                                update(w, &n, |s| s.selected = Some(f.clone()));
                                set_field(w, &format!("{n}-name-field"), &f);
                            }
                        });
                    }),
                    observe(move |_: On<DoubleClick>, mut commands: Commands| {
                        let n = n2.clone();
                        // A double-click in a multi-select picker picks what is selected (the
                        // first click of it already picked this file).
                        if !is_dir {
                            commands.queue(move |w: &mut World| accept(w, &n));
                        }
                    }),
                ));
            }
        });
        // The folder field shows the folder.
        let folder_field = format!("{}-folder-field", state.name);
        commands.queue(move |w: &mut World| {
            if field_text(w, &folder_field).as_deref() != Some(dir_text.as_str()) {
                set_field(w, &folder_field, &dir_text);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_split_and_join() {
        assert_eq!(split_names(""), Vec::<String>::new());
        assert_eq!(split_names(" Vision PCB.emn "), ["Vision PCB.emn"]);
        assert_eq!(
            split_names(r#""Vision PCB.emn" "Vision PCB.emp""#),
            ["Vision PCB.emn", "Vision PCB.emp"]
        );
        let names = vec!["a b.emn".to_string(), "c.emp".to_string()];
        assert_eq!(split_names(&join_names(&names)), names);
        assert_eq!(join_names(&names[..1]), "a b.emn");
    }

    #[test]
    fn lists_folders_first_then_matching_files() {
        let d = std::env::temp_dir().join(format!("cadrs-picker-{}", std::process::id()));
        let _ = cadrs_fs::remove_dir_all(&d);
        cadrs_fs::create_dir_all(d.join("sub")).unwrap();
        cadrs_fs::write(d.join("b.DXF"), "x").unwrap();
        cadrs_fs::write(d.join("a.dxf"), "xy").unwrap();
        cadrs_fs::write(d.join("c.txt"), "x").unwrap();
        cadrs_fs::write(d.join(".hidden.dxf"), "x").unwrap();
        let l = list_dir(&d, &["dxf".into(), "dwg".into()]);
        let names: Vec<&str> = l.iter().map(|e| e.0.as_str()).collect();
        assert_eq!(names, ["sub", "a.dxf", "b.DXF"]);
        assert_eq!(l[1].2, 2);
        let _ = cadrs_fs::remove_dir_all(&d);
    }
}
