//! A gallery of every component in its states, like gpui-component's story app. The app shows it
//! in its hidden `Gallery` state (`--scenario ui_gallery` starts there).

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};

use crate::button::{Button, IconButton};
use crate::dialog::{Dialog, DialogClose};
use crate::icon::{IconAtlas, icon};
use crate::input::TextInput;
use crate::list::{GridItem, ListItem};
use crate::menu::{Menu, MenuItem, open_menu};
use crate::style::VisualState;
use crate::theme::Theme;

const STATES: [(VisualState, &str); 4] = [
    (VisualState::Normal, "normal"),
    (VisualState::Hover, "hover"),
    (VisualState::Pressed, "pressed"),
    (VisualState::Disabled, "disabled"),
];

fn section(theme: &Theme, title: &str) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(theme.space[4]),
            padding: UiRect::all(Val::Px(theme.space[6])),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(theme.radius_lg)),
            ..default()
        },
        BorderColor::all(theme.separator),
        children![theme.text(title, theme.font_md, FontWeight::SEMIBOLD, theme.foreground)],
    )
}

fn row(theme: &Theme) -> Node {
    Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: Val::Px(theme.space[4]),
        ..default()
    }
}

fn caption(theme: &Theme, text: &str) -> impl Bundle {
    (
        theme.text(
            text,
            theme.font_sm,
            FontWeight::NORMAL,
            theme.muted_foreground,
        ),
        Node {
            width: Val::Px(72.0),
            ..default()
        },
    )
}

/// The menu shown by the gallery's "Create" button (mirrors Onshape's Create menu).
pub fn gallery_menu(theme: &Theme) -> impl Bundle {
    Menu::new("gallery-menu")
        .item(MenuItem::new("menu-document", "Document…").icon("file"))
        .item(MenuItem::new("menu-folder", "Folder…").icon("folder"))
        .separator()
        .item(MenuItem::new("menu-import", "Import files…").icon("upload"))
        .item(
            MenuItem::new("menu-import-from", "Import from")
                .icon("upload")
                .submenu(vec![
                    MenuItem::new("menu-import-drive", "Cloud drive…").into(),
                    MenuItem::new("menu-import-url", "URL…").into(),
                ]),
        )
        .item(MenuItem::new("menu-label", "Label…").disabled(true))
        .item(
            MenuItem::new("menu-settings", "Settings")
                .icon("settings")
                .shortcut("Ctrl+,"),
        )
        .build(theme)
}

/// The dialog opened from the gallery (mirrors Onshape's "New document" dialog).
pub fn gallery_dialog(theme: &Theme) -> impl Bundle {
    let t = theme.clone();
    let t2 = theme.clone();
    Dialog::new("gallery-dialog")
        .title("New document")
        .body(move |p| {
            p.spawn(t.text(
                "Document name",
                t.font_base,
                FontWeight::SEMIBOLD,
                t.foreground,
            ));
            p.spawn(
                TextInput::new("dialog-name")
                    .value("Untitled document")
                    .select_all_on_focus()
                    .autofocus()
                    .build(&t),
            );
            p.spawn(t.text(
                "Document labels",
                t.font_base,
                FontWeight::SEMIBOLD,
                t.foreground,
            ));
            p.spawn(
                TextInput::new("dialog-labels")
                    .placeholder("Search labels")
                    .build(&t),
            );
        })
        .footer(move |p| {
            let root_close = |a: On<Activate>,
                              parents: Query<&ChildOf>,
                              roots: Query<(), With<crate::dialog::DialogRoot>>,
                              mut commands: Commands| {
                if let Some(root) = parents
                    .iter_ancestors(a.entity)
                    .find(|e| roots.contains(*e))
                {
                    commands.trigger(DialogClose { entity: root });
                }
            };
            p.spawn((
                Button::new("dialog-create")
                    .label("Create")
                    .primary()
                    .build(&t2),
                observe(root_close),
            ));
            p.spawn((
                Button::new("dialog-cancel").label("Cancel").build(&t2),
                observe(root_close),
            ));
        })
        .build(theme)
}

/// Spawns the gallery and returns its root entity.
pub fn spawn_gallery(commands: &mut Commands, theme: &Theme, atlas: &IconAtlas) -> Entity {
    let t = theme.clone();
    let root = commands
        .spawn((
            Name::new("gallery"),
            bevy::input_focus::tab_navigation::TabGroup::new(0),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(t.space[6])),
                row_gap: Val::Px(t.space[5]),
                ..default()
            },
            BackgroundColor(t.background),
        ))
        .id();

    commands.entity(root).with_children(|p| {
        p.spawn(t.text(
            "cadrs_ui gallery",
            t.font_xl,
            FontWeight::SEMIBOLD,
            t.foreground,
        ));

        p.spawn(Node {
            column_gap: Val::Px(t.space[5]),
            align_items: AlignItems::FlexStart,
            ..default()
        })
        .with_children(|cols| {
            // Column 1: buttons.
            cols.spawn(section(&t, "Buttons")).with_children(|s| {
                for (label, variant) in [
                    ("Primary", crate::ButtonVariant::Primary),
                    ("Secondary", crate::ButtonVariant::Secondary),
                    ("Ghost", crate::ButtonVariant::Ghost),
                    ("Link", crate::ButtonVariant::Link),
                ] {
                    s.spawn(row(&t)).with_children(|r| {
                        r.spawn(caption(&t, label));
                        for (state, sname) in STATES {
                            r.spawn(
                                Button::new(format!(
                                    "gallery-btn-{}-{sname}",
                                    label.to_lowercase()
                                ))
                                .label(sname)
                                .variant(variant)
                                .force_state(state)
                                .build(&t),
                            );
                        }
                    });
                }
                s.spawn(row(&t)).with_children(|r| {
                    r.spawn(caption(&t, "Live"));
                    r.spawn((
                        Button::new("gallery-live-button")
                            .label("Click me")
                            .primary()
                            .build(&t),
                        observe(on_live_click),
                    ));
                    r.spawn(
                        Button::new("gallery-live-secondary")
                            .label("Cancel")
                            .build(&t),
                    );
                    r.spawn(
                        Button::new("gallery-live-disabled")
                            .label("Disabled")
                            .primary()
                            .disabled(true)
                            .build(&t),
                    );
                });
                s.spawn(row(&t)).with_children(|r| {
                    r.spawn(caption(&t, "Icon/caret"));
                    r.spawn((
                        Button::new("gallery-menu-button")
                            .label("Create")
                            .primary()
                            .large()
                            .dropdown_caret()
                            .width(Val::Px(138.0))
                            .build(&t),
                        observe(on_menu_button),
                    ));
                    r.spawn(
                        Button::new("gallery-btn-icon")
                            .label("Sketch")
                            .icon("edit")
                            .ghost()
                            .build(&t),
                    );
                    r.spawn(
                        Button::new("gallery-btn-add")
                            .label("Add")
                            .icon("plus")
                            .link()
                            .build(&t),
                    );
                    r.spawn(
                        Button::new("gallery-btn-small")
                            .label("Small")
                            .small()
                            .build(&t),
                    );
                });
                s.spawn(row(&t)).with_children(|r| {
                    r.spawn(caption(&t, "Icon btn"));
                    for (i, (state, sname)) in STATES.into_iter().enumerate() {
                        let icons = ["undo", "redo", "settings", "delete"];
                        r.spawn(
                            IconButton::new(format!("gallery-iconbtn-{sname}"), icons[i])
                                .force_state(state)
                                .build(&t),
                        );
                    }
                    r.spawn(
                        IconButton::new("gallery-tooltip-button", "help")
                            .tooltip("Help and documentation")
                            .build(&t),
                    );
                    r.spawn(
                        IconButton::new("gallery-iconbtn-selected", "apps")
                            .selected(true)
                            .build(&t),
                    );
                });
                s.spawn(row(&t)).with_children(|r| {
                    r.spawn(caption(&t, "Dialog"));
                    r.spawn((
                        Button::new("gallery-open-dialog")
                            .label("Open dialog…")
                            .build(&t),
                        observe(on_open_dialog),
                    ));
                });
            });

            // Column 2: inputs and lists.
            cols.spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(t.space[5]),
                width: Val::Px(420.0),
                ..default()
            })
            .with_children(|c| {
                c.spawn(section(&t, "Text input")).with_children(|s| {
                    s.spawn(
                        TextInput::new("gallery-input-empty")
                            .placeholder("Search in Owned by me")
                            .build(&t),
                    );
                    s.spawn(
                        TextInput::new("gallery-input-name")
                            .value("Untitled document")
                            .select_all_on_focus()
                            .build(&t),
                    );
                    s.spawn(
                        TextInput::new("gallery-input-disabled")
                            .value("Disabled")
                            .disabled(true)
                            .build(&t),
                    );
                });
                c.spawn(section(&t, "List")).with_children(|s| {
                    s.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|l| {
                        l.spawn(
                            ListItem::new("gallery-filter-explore")
                                .icon("public")
                                .label("Explore")
                                .height(29.0)
                                .build(&t),
                        );
                        l.spawn(
                            ListItem::new("gallery-filter-owned")
                                .icon("user")
                                .label("Owned by me")
                                .height(29.0)
                                .selection_indicator()
                                .selected(true)
                                .build(&t),
                        );
                        l.spawn(
                            ListItem::new("gallery-filter-recent")
                                .icon("clock")
                                .label("Recently opened")
                                .height(29.0)
                                .build(&t),
                        );
                    });
                    s.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|l| {
                        for (i, (state, sname)) in [
                            (VisualState::Normal, "normal"),
                            (VisualState::Hover, "hover"),
                            (VisualState::Selected, "selected"),
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            l.spawn(
                                ListItem::new(format!("gallery-row-{sname}"))
                                    .icon("part")
                                    .label(format!("Document {} ({sname})", i + 1))
                                    .detail("2:19 PM Sep 22")
                                    .force_state(state)
                                    .build(&t),
                            );
                        }
                        l.spawn(
                            ListItem::new("gallery-row-live")
                                .icon("part")
                                .label("Hover me")
                                .detail("11:44 AM Sep 14")
                                .build(&t),
                        );
                    });
                });
            });

            // Column 3: grid items and icons.
            cols.spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(t.space[5]),
                width: Val::Px(420.0),
                ..default()
            })
            .with_children(|c| {
                c.spawn(section(&t, "Grid items")).with_children(|s| {
                    s.spawn(Node {
                        column_gap: Val::Px(t.space[4]),
                        ..default()
                    })
                    .with_children(|g| {
                        g.spawn(
                            GridItem::new("gallery-grid-normal", "Bracket")
                                .size(Vec2::new(120.0, 110.0))
                                .build(&t),
                        );
                        g.spawn(
                            GridItem::new("gallery-grid-hover", "Enclosure")
                                .size(Vec2::new(120.0, 110.0))
                                .force_state(VisualState::Hover)
                                .build(&t),
                        );
                        g.spawn(
                            GridItem::new("gallery-grid-selected", "Robot")
                                .size(Vec2::new(120.0, 110.0))
                                .selected(true)
                                .build(&t),
                        );
                    });
                });
                c.spawn(section(&t, "Icons (icon-rs)")).with_children(|s| {
                    s.spawn(Node {
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: Val::Px(t.space[5]),
                        row_gap: Val::Px(t.space[5]),
                        width: Val::Px(370.0),
                        ..default()
                    })
                    .with_children(|g| {
                        for name in atlas.names() {
                            g.spawn((icon(name, 16.0, t.foreground), crate::Tooltip::new(name)));
                        }
                    });
                });
            });
        });

        p.spawn((
            Name::new("gallery-status"),
            t.text(
                "Status: idle",
                t.font_base,
                FontWeight::NORMAL,
                t.muted_foreground,
            ),
            GalleryStatus,
        ));
    });
    root
}

/// The status line at the bottom of the gallery.
#[derive(Component)]
pub struct GalleryStatus;

fn set_status(text: String) -> impl FnOnce(&mut World) {
    move |world: &mut World| {
        let mut q = world.query_filtered::<&mut Text, With<GalleryStatus>>();
        for mut t in q.iter_mut(world) {
            t.0 = text.clone();
        }
    }
}

fn on_live_click(_: On<Activate>, mut commands: Commands, mut count: Local<u32>) {
    *count += 1;
    commands.queue(set_status(format!("Status: clicked {} time(s)", *count)));
}

fn on_menu_button(a: On<Activate>, mut commands: Commands, theme: Res<Theme>) {
    open_menu(&mut commands, a.entity, gallery_menu(&theme));
}

fn on_open_dialog(_: On<Activate>, mut commands: Commands, theme: Res<Theme>) {
    commands.spawn(gallery_dialog(&theme));
}

/// Updates the gallery status line when a menu item is chosen.
pub fn on_gallery_menu_action(ev: On<crate::MenuAction>, mut commands: Commands) {
    commands.queue(set_status(format!("Status: menu item {:?}", ev.item)));
}
