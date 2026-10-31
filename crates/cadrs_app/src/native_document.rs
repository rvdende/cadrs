//! Whole-document native export and library import.
use crate::{ActiveDocument, AppClock, AppState, DocumentStore, UserProfile};
use bevy::{
    prelude::*,
    text::{EditableText, FontWeight, TextEdit},
    ui_widgets::Activate,
};
use cadrs_ui::i18n::{Translated, tr, tr_with};
use cadrs_ui::{
    Button, DialogClose, Notification, TextInputField, TextSubmit, prelude::*, show_notification,
};
use std::path::{Path, PathBuf};

pub struct NativeDocumentPlugin;
impl Plugin for NativeDocumentPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(activate)
            .add_observer(submit)
            .add_systems(Update, folder_picked);
    }
}
#[derive(Component)]
struct NativeExport;

pub fn open_export(world: &mut World) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let base = doc.doc.name.clone();
    let dir = crate::export_dir(world)
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let theme = world.resource::<Theme>().clone();
    let (body, footer) = (theme.clone(), theme.clone());
    world.commands().spawn((
        Dialog::new("native-export-dialog")
            .title(tr("io-native-export-title"))
            .width(460.0)
            .body(move |b| {
                let t = &body;
                b.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(10.0),
                    width: Val::Percent(100.0),
                    ..default()
                })
                .with_children(|g| {
                    g.spawn((
                        t.text(
                            tr("common-file-name"),
                            t.font_base,
                            FontWeight::BOLD,
                            t.foreground,
                        ),
                        Translated("common-file-name"),
                    ));
                    g.spawn(
                        TextInput::new("native-export-name")
                            .value(base)
                            .autofocus()
                            .select_all_on_focus()
                            .width(Val::Percent(100.0))
                            .height(26.0)
                            .build(t),
                    );
                    g.spawn((
                        t.text(
                            tr("io-native-export-format"),
                            t.font_base,
                            FontWeight::NORMAL,
                            t.foreground,
                        ),
                        Translated("io-native-export-format"),
                    ));
                    g.spawn(t.text(
                        tr("io-native-export-contents"),
                        t.font_sm,
                        FontWeight::NORMAL,
                        t.muted_foreground,
                    ))
                    .insert((
                        Translated("io-native-export-contents"),
                        TextLayout::new(
                            bevy::text::Justify::Left,
                            bevy::text::LineBreak::WordBoundary,
                        ),
                        Node {
                            width: Val::Percent(100.0),
                            min_width: Val::Px(0.0),
                            ..default()
                        },
                    ));
                    g.spawn((
                        t.text(
                            tr("common-folder"),
                            t.font_base,
                            FontWeight::BOLD,
                            t.foreground,
                        ),
                        Translated("common-folder"),
                    ));
                    g.spawn(
                        TextInput::new("native-export-folder")
                            .value(dir)
                            .width(Val::Percent(100.0))
                            .height(26.0)
                            .build(t),
                    );
                    g.spawn(
                        Button::new("native-export-browse")
                            .label(tr("common-browse"))
                            .outline()
                            .build(t),
                    );
                });
            })
            .footer(move |f| {
                f.spawn(
                    Button::new("native-export-ok")
                        .label(tr("common-export"))
                        .primary()
                        .build(&footer),
                );
                f.spawn(
                    Button::new("native-export-cancel")
                        .label(tr("common-cancel"))
                        .build(&footer),
                );
            })
            .build(&theme),
        NativeExport,
        DespawnOnExit(AppState::Document),
    ));
    world.flush();
}
fn field(world: &mut World, name: &str) -> String {
    let mut q = world.query_filtered::<(&Name, &EditableText), With<TextInputField>>();
    q.iter(world)
        .find(|(n, _)| n.as_str() == name)
        .map(|(_, t)| t.value().to_string())
        .unwrap_or_default()
}
fn close(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<NativeExport>>();
    for entity in q.iter(world).collect::<Vec<_>>() {
        world.trigger(DialogClose { entity });
    }
}
fn notify(world: &mut World, note: Notification) {
    let theme = world.resource::<Theme>().clone();
    show_notification(
        &mut world.commands(),
        &theme,
        note.name("native-document-toast"),
    );
    world.flush();
}
fn export(world: &mut World) {
    let name = field(world, "native-export-name-field");
    let folder = field(world, "native-export-folder-field");
    if name.trim().is_empty() || folder.trim().is_empty() {
        notify(world, Notification::warning(tr("io-native-export-missing")));
        return;
    }
    let base = name.trim();
    let base = if base.to_ascii_lowercase().ends_with(".cadrs") {
        &base[..base.len() - 6]
    } else {
        base
    };
    let path = cadrs_core::export::unique_path_ext(
        Path::new(folder.trim()),
        &cadrs_core::export::sanitize(base),
        "cadrs",
    );
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let meta = doc.meta.clone().unwrap_or_else(|| {
        cadrs_core::DocumentMeta::new(
            &world.resource::<UserProfile>().id,
            world.resource::<AppClock>().now(),
        )
    });
    let result =
        cadrs_core::native::export(&path, &doc.doc, &meta, &world.resource::<DocumentStore>().0);
    match result {
        Ok(()) => {
            close(world);
            notify(
                world,
                Notification::info(tr_with(
                    "io-exported-path",
                    &[("path", &path.display().to_string())],
                )),
            );
        }
        Err(e) => notify(
            world,
            Notification::warning(tr_with("io-export-failed", &[("error", &e.to_string())])),
        ),
    }
}
fn activate(ev: On<Activate>, names: Query<&Name>, mut commands: Commands) {
    let Ok(name) = names.get(ev.entity) else {
        return;
    };
    match name.as_str() {
        "native-export-ok" => commands.queue(export),
        "native-export-cancel" => commands.queue(close),
        "native-export-browse" => commands.queue(|world: &mut World| {
            let dir = PathBuf::from(field(world, "native-export-folder-field").trim());
            let dir = if cadrs_fs::is_dir(&dir) {
                dir
            } else {
                std::env::current_dir().unwrap_or_default()
            };
            let theme = world.resource::<Theme>().clone();
            cadrs_ui::file_picker::open_folder_picker(
                &mut world.commands(),
                &theme,
                "folder-picker",
                &tr("common-choose-folder"),
                "native-export-folder",
                dir,
            );
            world.flush();
        }),
        _ => {}
    }
}
fn submit(ev: On<TextSubmit>, names: Query<&Name>, mut commands: Commands) {
    if names
        .get(ev.entity)
        .is_ok_and(|n| n.as_str() == "native-export-name-field")
    {
        commands.queue(export);
    }
}
fn folder_picked(mut msgs: MessageReader<cadrs_ui::FilePicked>, mut commands: Commands) {
    for m in msgs.read().filter(|m| m.tag == "native-export-folder") {
        let text = m.path.display().to_string();
        commands.queue(move |world: &mut World| {
            let mut q = world.query_filtered::<(&Name, &mut EditableText), With<TextInputField>>();
            if let Some((_, mut t)) = q
                .iter_mut(world)
                .find(|(n, _)| n.as_str() == "native-export-folder-field")
            {
                t.queue_edit(TextEdit::SelectAll);
                t.queue_edit(TextEdit::Insert(text.into()));
            }
        });
    }
}
pub fn import(world: &mut World, path: &Path) {
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    match cadrs_core::native::import(path, &store, &user, now) {
        Ok(file) => {
            let name = file.document.name.clone();
            let mut active = ActiveDocument::stored(file.document, file.meta);
            active.fresh = true;
            world.insert_resource(active);
            world
                .resource_mut::<NextState<AppState>>()
                .set(AppState::Document);
            notify(
                world,
                Notification::info(tr_with("io-imported-name", &[("name", &name)])),
            );
        }
        Err(e) => notify(
            world,
            Notification::warning(tr_with("io-import-failed", &[("error", &e.to_string())])),
        ),
    }
}
