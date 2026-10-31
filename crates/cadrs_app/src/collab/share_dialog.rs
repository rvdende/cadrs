//! The share dialog, as Onshape's (**Share** in the document's top bar), for a peer-to-peer
//! cadrs: people are added by their **cadrs ID** (Preferences ▸ Collaboration shows one's
//! own) or picked from the contacts, rather than by email; Teams and Companies need a server
//! and are left out.
//!
//! - **Individuals**: the cadrs ID, **Can edit** or **Can view**, the checkboxes (Copy,
//!   Export, Share, Comment, Link document), an optional message, and **Share**. Below, who it
//!   is shared with, each with their permission (changed in place) and an ✕.
//! - **Link sharing**: **Turn on link sharing**; then the link, **Copy to clipboard**, its
//!   permission, **Allow exporting from the link** and an ✕ next to "Anyone with link".
//! - **Public** ([`super::public`]): **Publish** the document as it is, for anyone using cadrs
//!   to find, view and fork (never edit); once it is public, the revision and its stars,
//!   **Publish again** and **Make private**.
//!
//! Someone it is shared with who may share sees Individuals only, and their additions go
//! through the owner.

use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::DocumentId;
use cadrs_core::collab::{Member, Permission, Rights};
use cadrs_ui::prelude::*;
use cadrs_ui::{
    Avatar, Button, Checkbox, CheckboxChange, CheckboxState, DialogClose, IconButton, Segmented,
    SegmentedSelect, Select, SelectChange, SelectState, SelectableText, TextInput, TextInputField,
    form_row,
};

use super::sharing::{self, ContactBook};
use cadrs_ui::i18n::{Translated, tr, tr_with};

pub(super) fn plugin(app: &mut App) {
    app.add_observer(on_select)
        .add_observer(on_checkbox)
        .add_observer(on_segment)
        .add_observer(on_remove);
}

/// The dialog's root.
#[derive(Component, Debug, Clone, Copy)]
pub struct ShareDialog {
    pub document: DocumentId,
    /// The owner's dialog; otherwise someone it is shared with who may share.
    pub owner: bool,
}

/// The rights' checkboxes: name, label's catalog key.
const RIGHTS: [(&str, &str); 5] = [
    ("share-right-copy", "common-copy"),
    ("share-right-export", "common-export"),
    ("share-right-share", "share"),
    ("share-right-comment", "shell-share-right-comment"),
    ("share-right-link", "shell-share-right-link"),
];

/// A permission's name, as the selects list it.
fn permission_label(p: Permission) -> String {
    tr(match p {
        Permission::Edit => "shell-share-can-edit",
        Permission::View => "shell-share-can-view",
    })
}

fn rights_list(r: Rights) -> [bool; 5] {
    [r.copy, r.export, r.share, r.comment, r.link]
}

fn dialog(world: &mut World) -> Option<ShareDialog> {
    let mut q = world.query::<&ShareDialog>();
    q.iter(world).next().copied()
}

/// Opens the share dialog for `id` (the open document).
pub fn open(world: &mut World, id: DocumentId) {
    close(world);
    let Some((doc, meta)) = sharing::read(world, id) else {
        return;
    };
    let owner = meta.shared.is_none();
    let contacts: Vec<(String, String)> = world
        .get_resource::<ContactBook>()
        .map(|b| {
            b.0.0
                .iter()
                .map(|c| (c.id.clone(), c.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let title = tr_with("shell-share-title", &[("name", doc.name.as_str())]);
    world.commands().spawn((
        Dialog::new("share-dialog")
            .title(title)
            .width(560.0)
            .body(move |b| {
                let t = &tb;
                if owner {
                    b.spawn(
                        Segmented::new("share-tabs")
                            .option(
                                tr("shell-share-individuals"),
                                None,
                                tr("shell-share-individuals-tooltip"),
                            )
                            .option(
                                tr("shell-share-link-sharing"),
                                None,
                                tr("shell-share-link-sharing-tooltip"),
                            )
                            .option(
                                tr("shell-share-public"),
                                None,
                                tr("shell-share-public-tooltip"),
                            )
                            .size(26.0, 12.0)
                            .build(t),
                    )
                    .entry::<Node>()
                    .and_modify(|mut n| n.margin = UiRect::bottom(Val::Px(10.0)));
                }
                b.spawn((
                    Name::new("share-individuals"),
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(8.0),
                        ..default()
                    },
                ))
                .with_children(|p| individuals(p, t, &contacts));
                b.spawn((
                    Name::new("share-link"),
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(8.0),
                        display: Display::None,
                        ..default()
                    },
                ));
                b.spawn((
                    Name::new("share-public"),
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(10.0),
                        display: Display::None,
                        ..default()
                    },
                ));
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("share-confirm")
                        .label(tr("share"))
                        .primary()
                        .build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(confirm);
                    }),
                ));
                f.spawn((
                    Button::new("share-close")
                        .label(tr("common-close"))
                        .build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(close);
                    }),
                ));
            })
            .build(&theme),
        ShareDialog {
            document: id,
            owner,
        },
    ));
    world.flush();
    refresh(world);
}

/// The Individuals tab's form.
fn individuals(p: &mut ChildSpawner, t: &Theme, contacts: &[(String, String)]) {
    p.spawn(form_row(
        t,
        "share-id-row",
        tr("shell-share-cadrs-id"),
        110.0,
    ))
    .with_children(|r| {
        r.spawn(
            TextInput::new("share-id")
                .placeholder(tr("shell-share-id-placeholder"))
                .autofocus()
                .build(t),
        )
        .entry::<Node>()
        .and_modify(|mut n| n.flex_grow = 1.0);
        if !contacts.is_empty() {
            let mut select = Select::new("share-contact").width(Val::Px(140.0));
            select = select.option(tr("shell-share-contacts"), false);
            for (_, name) in contacts {
                select = select.option(name.clone(), true);
            }
            r.spawn(select.build(t))
                .entry::<Node>()
                .and_modify(|mut n| n.margin = UiRect::left(Val::Px(6.0)));
        }
    });
    let mut permission = Select::new("share-permission").width(Val::Px(140.0));
    for p in Permission::ALL {
        permission = permission.option(permission_label(p), true);
    }
    p.spawn(form_row(
        t,
        "share-permission-row",
        tr("shell-share-permission"),
        110.0,
    ))
    .with_child(permission.selected(0).build(t));
    p.spawn((
        Name::new("share-rights"),
        Node {
            column_gap: Val::Px(12.0),
            flex_wrap: FlexWrap::Wrap,
            margin: UiRect::left(Val::Px(110.0)),
            ..default()
        },
    ))
    .with_children(|r| {
        let on = rights_list(Rights::of(Permission::Edit));
        for ((name, label), checked) in RIGHTS.into_iter().zip(on) {
            r.spawn(
                Checkbox::new(name)
                    .label(tr(label))
                    .checked(checked)
                    .build(t),
            );
        }
    });
    p.spawn(form_row(
        t,
        "share-message-row",
        tr("shell-share-message"),
        110.0,
    ))
    .with_children(|r| {
        r.spawn(
            TextInput::new("share-message")
                .placeholder(tr("shell-share-message-placeholder"))
                .build(t),
        )
        .entry::<Node>()
        .and_modify(|mut n| n.flex_grow = 1.0);
    });
    p.spawn((
        Name::new("share-members"),
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            margin: UiRect::top(Val::Px(8.0)),
            ..default()
        },
    ));
}

fn named(world: &mut World, name: &str) -> Option<Entity> {
    let mut q = world.query::<(Entity, &Name)>();
    q.iter(world)
        .find(|(_, n)| n.as_str() == name)
        .map(|(e, _)| e)
}

fn field(world: &mut World, name: &str) -> String {
    let field = format!("{name}-field");
    let mut q = world.query_filtered::<(&Name, &EditableText), With<TextInputField>>();
    q.iter(world)
        .find(|(n, _)| n.as_str() == field)
        .map(|(_, t)| t.value().to_string())
        .unwrap_or_default()
}

fn set_field(world: &mut World, name: &str, value: &str) {
    crate::pcb::dialogs::set_text(world, &format!("{name}-field"), value);
}

fn selected(world: &mut World, name: &str) -> usize {
    let mut q = world.query::<(&Name, &SelectState)>();
    q.iter(world)
        .find(|(n, _)| n.as_str() == name)
        .map_or(0, |(_, s)| s.selected)
}

fn checked(world: &mut World, name: &str) -> bool {
    let mut q = world.query::<(&Name, &CheckboxState)>();
    q.iter(world)
        .find(|(n, _)| n.as_str() == name)
        .is_some_and(|(_, s)| s.checked)
}

fn set_checked(world: &mut World, name: &str, on: bool) {
    let mut q = world.query::<(&Name, &mut CheckboxState)>();
    if let Some((_, mut s)) = q.iter_mut(world).find(|(n, _)| n.as_str() == name) {
        s.checked = on;
    }
}

/// Fills the parts that follow the sharing: who it is shared with, and the link.
fn refresh(world: &mut World) {
    let Some(d) = dialog(world) else { return };
    let Some((_, meta)) = sharing::read(world, d.document) else {
        return;
    };
    let theme = world.resource::<Theme>().clone();
    let t = &theme;
    let names: Vec<(String, String)> = world
        .get_resource::<ContactBook>()
        .map(|b| {
            b.0.0
                .iter()
                .map(|c| (c.id.clone(), c.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    let name_of = |id: &str, fallback: &str| {
        names
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, n)| n.clone())
            .filter(|_| fallback.is_empty())
            .unwrap_or_else(|| {
                if fallback.is_empty() {
                    id.chars().take(10).collect()
                } else {
                    fallback.to_string()
                }
            })
    };
    if let Some(list) = named(world, "share-members") {
        world.entity_mut(list).despawn_children();
        let owner_name = match &meta.shared {
            Some(s) => s.owner_name.clone(),
            None => world.resource::<crate::UserProfile>().display_name.clone(),
        };
        let members: Vec<(usize, String, String, Permission)> = meta
            .sharing
            .iter()
            .flat_map(|s| s.members.iter().enumerate())
            .map(|(i, m)| (i, m.id.clone(), name_of(&m.id, &m.name), m.permission))
            .collect();
        let owner = d.owner;
        world.entity_mut(list).with_children(|l| {
            l.spawn((
                t.text(
                    tr("shell-share-shared-with"),
                    t.font_base,
                    FontWeight::BOLD,
                    t.foreground,
                ),
                Translated("shell-share-shared-with"),
            ));
            person_row(
                l,
                t,
                "share-owner",
                &owner_name,
                "",
                &tr("shell-share-owner"),
                None,
                false,
            );
            for (i, id, name, permission) in members {
                let short: String = id.chars().take(10).collect();
                person_row(
                    l,
                    t,
                    &format!("share-member-{i}"),
                    &name,
                    &short,
                    &permission_label(permission),
                    Some(permission),
                    owner,
                );
            }
        });
    }
    if let Some(panel) = named(world, "share-link") {
        world.entity_mut(panel).despawn_children();
        let link = d
            .owner
            .then(|| {
                meta.sharing
                    .as_ref()
                    .and_then(|s| s.link.map(|p| (p, s.link_export)))
            })
            .flatten();
        let text = sharing::link_for(world, d.document).map(|l| l.to_string());
        world
            .entity_mut(panel)
            .with_children(|p| match (link, text) {
                (Some((permission, export)), Some(text)) => {
                    let mut select = Select::new("share-link-permission").width(Val::Px(120.0));
                    for p in Permission::ALL {
                        select = select.option(permission_label(p), true);
                    }
                    let at = Permission::ALL
                        .iter()
                        .position(|x| *x == permission)
                        .unwrap_or(1);
                    p.spawn(Node {
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(8.0),
                        ..default()
                    })
                    .with_children(|r| {
                        r.spawn(icon("link", 16.0, t.foreground));
                        r.spawn((
                            t.text(
                                tr("shell-share-anyone-with-link"),
                                t.font_base,
                                FontWeight::MEDIUM,
                                t.foreground,
                            ),
                            Translated("shell-share-anyone-with-link"),
                        ))
                        .entry::<Node>()
                        .and_modify(|mut n| n.flex_grow = 1.0);
                        r.spawn(select.selected(at).build(t));
                        r.spawn((
                            IconButton::new("share-link-off", "close")
                                .small()
                                .tooltip(tr("shell-share-link-off"))
                                .build(t),
                            observe(|_: On<Activate>, mut commands: Commands| {
                                commands.queue(|w: &mut World| link_change(w, None, None));
                            }),
                        ));
                    });
                    // The link is long: shown from its start, cut off at the box's end.
                    p.spawn((
                        Name::new("share-link-box"),
                        Node {
                            width: Val::Px(520.0),
                            overflow: Overflow::clip(),
                            justify_content: JustifyContent::FlexStart,
                            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(t.radius)),
                            ..default()
                        },
                        BorderColor::all(t.input_border),
                    ))
                    .with_children(|b| {
                        b.spawn(
                            SelectableText::new("share-link-text")
                                .value(text)
                                .font(t.font_sm, FontWeight::NORMAL)
                                .build(t),
                        )
                        .entry::<Node>()
                        .and_modify(|mut n| {
                            n.flex_shrink = 0.0;
                            n.flex_grow = 0.0;
                        });
                    });
                    p.spawn(Node {
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(12.0),
                        ..default()
                    })
                    .with_children(|r| {
                        r.spawn((
                            Button::new("share-link-copy")
                                .label(tr("shell-share-copy-link"))
                                .build(t),
                            observe(|_: On<Activate>, mut commands: Commands| {
                                commands.queue(copy);
                            }),
                        ));
                        r.spawn(
                            Checkbox::new("share-link-export")
                                .label(tr("shell-share-link-export"))
                                .checked(export)
                                .build(t),
                        );
                    });
                }
                _ => {
                    p.spawn((
                        t.text(
                            tr("shell-share-link-hint"),
                            t.font_sm,
                            FontWeight::NORMAL,
                            t.muted_foreground,
                        ),
                        Translated("shell-share-link-hint"),
                        Node {
                            width: Val::Px(520.0),
                            ..default()
                        },
                    ))
                    .insert(TextLayout::default());
                    p.spawn((
                        Button::new("share-link-on")
                            .label(tr("shell-share-link-on"))
                            .build(t),
                        observe(|_: On<Activate>, mut commands: Commands| {
                            commands.queue(|w: &mut World| {
                                link_change(w, Some(Permission::View), Some(false))
                            });
                        }),
                    ));
                }
            });
    }
    if d.owner
        && let Some(panel) = named(world, "share-public")
    {
        world.entity_mut(panel).despawn_children();
        let date = meta
            .published
            .as_ref()
            .map(|p| world.resource::<crate::AppClock>().format(p.time));
        let stars = super::public::own_stars(world, d.document).unwrap_or(0);
        world
            .entity_mut(panel)
            .with_children(|p| public_tab(p, t, meta.published.as_ref(), date, stars));
    }
}

/// The Public tab: what publishing means, and Publish, or (published) the revision, its stars,
/// Publish again and Make private.
fn public_tab(
    p: &mut ChildSpawner,
    t: &Theme,
    published: Option<&cadrs_core::collab::Published>,
    date: Option<String>,
    stars: usize,
) {
    let note = |p: &mut ChildSpawner, key: &'static str| {
        p.spawn((
            t.text(tr(key), t.font_sm, FontWeight::NORMAL, t.muted_foreground),
            Translated(key),
            Node {
                width: Val::Px(520.0),
                ..default()
            },
        ))
        .insert(TextLayout::default());
    };
    let publish = |what: &'static str| {
        observe(move |_: On<Activate>, mut commands: Commands| {
            commands.queue(move |w: &mut World| {
                let Some(d) = dialog(w) else { return };
                let text = match what {
                    "publish" => super::public::publish(w, d.document).map(|l| {
                        tr_with(
                            "shell-share-published",
                            &[("revision", &l.revision.to_string())],
                        )
                    }),
                    _ => super::public::make_private(w, d.document)
                        .map(|()| tr("shell-share-made-private")),
                };
                crate::linked::toast(w, text.unwrap_or_else(|why| why));
                refresh(w);
            });
        })
    };
    match published {
        None => {
            p.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|r| {
                r.spawn(icon("public", 16.0, t.foreground));
                r.spawn((
                    t.text(
                        tr("shell-share-public-off"),
                        t.font_base,
                        FontWeight::MEDIUM,
                        t.foreground,
                    ),
                    Translated("shell-share-public-off"),
                ));
            });
            note(p, "shell-share-public-hint");
            p.spawn(Node::default()).with_children(|r| {
                r.spawn((
                    Button::new("share-publish")
                        .label(tr("shell-share-publish"))
                        .icon("public")
                        .primary()
                        .build(t),
                    publish("publish"),
                ));
            });
        }
        Some(pb) => {
            let status = tr_with(
                "shell-share-public-on",
                &[
                    ("revision", &pb.revision.to_string()),
                    ("date", date.as_deref().unwrap_or("")),
                ],
            );
            p.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|r| {
                r.spawn(icon("public", 16.0, t.foreground));
                r.spawn((
                    Name::new("share-public-status"),
                    t.text(status, t.font_base, FontWeight::MEDIUM, t.foreground),
                ))
                .entry::<Node>()
                .and_modify(|mut n| n.flex_grow = 1.0);
                r.spawn(icon("star", 14.0, t.muted_foreground));
                r.spawn((
                    Name::new("share-public-stars"),
                    t.text(
                        stars.to_string(),
                        t.font_base,
                        FontWeight::NORMAL,
                        t.foreground,
                    ),
                ));
            });
            note(p, "shell-share-public-hint-on");
            p.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|r| {
                r.spawn((
                    Button::new("share-publish")
                        .label(tr("shell-share-publish-again"))
                        .icon("public")
                        .primary()
                        .build(t),
                    publish("publish"),
                ));
                r.spawn((
                    Button::new("share-make-private")
                        .label(tr("shell-share-make-private"))
                        .outline()
                        .build(t),
                    publish("private"),
                ));
            });
        }
    }
}

/// A row of the Shared with list: an avatar, the name, the permission (a select for the
/// owner) and an ✕.
#[allow(clippy::too_many_arguments)]
fn person_row(
    l: &mut ChildSpawner,
    t: &Theme,
    name: &str,
    person: &str,
    short_id: &str,
    what: &str,
    permission: Option<Permission>,
    editable: bool,
) {
    l.spawn((
        Name::new(name.to_string()),
        Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            min_height: Val::Px(28.0),
            ..default()
        },
    ))
    .with_children(|r| {
        r.spawn(
            Avatar::new(format!("{name}-avatar"), person)
                .size(22.0)
                .build(t),
        );
        let label = if short_id.is_empty() {
            person.to_string()
        } else {
            format!("{person} · {short_id}")
        };
        r.spawn(t.text(label, t.font_base, FontWeight::NORMAL, t.foreground))
            .entry::<Node>()
            .and_modify(|mut n| n.flex_grow = 1.0);
        match permission.filter(|_| editable) {
            Some(p) => {
                let mut select = Select::new(format!("{name}-permission")).width(Val::Px(110.0));
                for x in Permission::ALL {
                    select = select.option(permission_label(x), true);
                }
                let at = Permission::ALL.iter().position(|x| *x == p).unwrap_or(0);
                r.spawn(select.selected(at).build(t));
                r.spawn(
                    IconButton::new(format!("{name}-remove"), "close")
                        .small()
                        .tooltip(tr("shell-share-stop-sharing"))
                        .build(t),
                );
            }
            None => {
                r.spawn(t.text(what, t.font_sm, FontWeight::NORMAL, t.muted_foreground));
            }
        }
    });
}

/// The ✕ of a member's row (global: a button's `Activate` doesn't reach its row).
fn on_remove(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    let Some(i) = q.get(a.event_target()).ok().and_then(|n| {
        n.as_str()
            .strip_prefix("share-member-")?
            .strip_suffix("-remove")?
            .parse()
            .ok()
    }) else {
        return;
    };
    commands.queue(move |w: &mut World| member_change(w, i, None));
}

/// Changes member `i`'s permission, or (`None`) stops sharing with them.
fn member_change(world: &mut World, i: usize, permission: Option<Permission>) {
    let Some(d) = dialog(world) else { return };
    let Some(m) = sharing::read(world, d.document)
        .and_then(|(_, meta)| meta.sharing?.members.get(i).cloned())
    else {
        return;
    };
    let r = match permission {
        Some(p) => sharing::share_with(
            world,
            d.document,
            Member {
                permission: p,
                rights: None,
                ..m
            },
            String::new(),
        ),
        None => sharing::unshare(world, d.document, &m.id),
    };
    if let Err(why) = r {
        crate::linked::toast(world, why);
    }
    refresh(world);
}

/// Link sharing on with `permission` (and exporting allowed or not), or off.
fn link_change(world: &mut World, permission: Option<Permission>, export: Option<bool>) {
    let Some(d) = dialog(world) else { return };
    let export = export.unwrap_or_else(|| checked(world, "share-link-export"));
    if let Err(why) = sharing::set_link(world, d.document, permission, export) {
        crate::linked::toast(world, why);
    }
    refresh(world);
}

fn copy(world: &mut World) {
    let Some(d) = dialog(world) else { return };
    if let Some(link) = sharing::link_for(world, d.document) {
        super::copy_link(world, &link);
        crate::linked::toast(world, tr("shell-share-link-copied"));
    }
}

/// **Share**: the person typed or picked, with the permission and checkboxes chosen.
fn confirm(world: &mut World) {
    let Some(d) = dialog(world) else { return };
    let id = field(world, "share-id").trim().to_string();
    if id.is_empty() {
        crate::linked::toast(world, tr("shell-share-id-needed"));
        return;
    }
    let permission = Permission::ALL[selected(world, "share-permission").min(1)];
    let [copy, export, share, comment, link] = RIGHTS.map(|(n, _)| checked(world, n));
    let rights = Rights {
        copy,
        export,
        share,
        comment,
        link,
    };
    let name = world
        .get_resource::<ContactBook>()
        .and_then(|b| b.0.name(&id).map(str::to_string))
        .unwrap_or_else(|| id.chars().take(10).collect());
    let member = Member {
        id: id.clone(),
        name: name.clone(),
        permission,
        rights: (rights != Rights::of(permission)).then_some(rights),
        delivered: None,
    };
    let message = field(world, "share-message").trim().to_string();
    let r = if d.owner {
        sharing::share_with(world, d.document, member, message)
            .map(|()| tr_with("shell-share-shared", &[("name", name.as_str())]))
    } else {
        sharing::ask_owner_to_share(world, d.document, member)
            .map(|()| tr_with("shell-share-asked-owner", &[("name", name.as_str())]))
    };
    match r {
        Ok(text) => {
            set_field(world, "share-id", "");
            set_field(world, "share-message", "");
            crate::linked::toast(world, text);
        }
        Err(why) => crate::linked::toast(world, why),
    }
    refresh(world);
}

pub fn close(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<ShareDialog>>();
    let roots: Vec<Entity> = q.iter(world).collect();
    for e in roots {
        world.trigger(DialogClose { entity: e });
    }
}

fn on_segment(ev: On<SegmentedSelect>, q: Query<&Name>, mut nodes: Query<(&Name, &mut Node)>) {
    if q.get(ev.entity).map(Name::as_str) != Ok("share-tabs") {
        return;
    }
    for (n, mut node) in &mut nodes {
        let show = match n.as_str() {
            "share-individuals" => ev.index == 0,
            "share-link" => ev.index == 1,
            "share-public" => ev.index == 2,
            // Share is for the people added; the Public tab has its own buttons.
            "share-confirm" => ev.index != 2,
            _ => continue,
        };
        node.display = if show { Display::Flex } else { Display::None };
    }
}

fn on_select(ev: On<SelectChange>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity).map(|n| n.as_str().to_string()) else {
        return;
    };
    let index = ev.index;
    match name.as_str() {
        // A contact: their ID in the field.
        "share-contact" if index > 0 => commands.queue(move |w: &mut World| {
            let id = w
                .get_resource::<ContactBook>()
                .and_then(|b| b.0.0.get(index - 1).map(|c| c.id.clone()));
            if let Some(id) = id {
                set_field(w, "share-id", &id);
            }
        }),
        // The checkboxes follow the permission.
        "share-permission" => commands.queue(move |w: &mut World| {
            let p = Permission::ALL[index.min(1)];
            for ((n, _), on) in RIGHTS.into_iter().zip(rights_list(Rights::of(p))) {
                set_checked(w, n, on);
            }
        }),
        "share-link-permission" => commands
            .queue(move |w: &mut World| link_change(w, Some(Permission::ALL[index.min(1)]), None)),
        n => {
            if let Some(i) = n
                .strip_prefix("share-member-")
                .and_then(|r| r.strip_suffix("-permission"))
                .and_then(|i| i.parse::<usize>().ok())
            {
                let p = Permission::ALL[index.min(1)];
                commands.queue(move |w: &mut World| member_change(w, i, Some(p)));
            }
        }
    }
}

fn on_checkbox(ev: On<CheckboxChange>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(Name::as_str) != Ok("share-link-export") {
        return;
    }
    let on = ev.checked;
    commands.queue(move |w: &mut World| {
        let Some(d) = dialog(w) else { return };
        let link = sharing::read(w, d.document).and_then(|(_, m)| m.sharing?.link);
        if let Some(p) = link {
            link_change(w, Some(p), Some(on));
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Transfer ownership

/// The Transfer ownership dialog's root: which document.
#[derive(Component, Debug, Clone, Copy)]
pub struct TransferDialog(pub DocumentId);

/// Transfer ownership… (the documents page's menu): to whom, among the people it is shared
/// with. It takes effect at once, as Onshape's does; the old owner keeps Can edit.
pub fn open_transfer(world: &mut World, id: DocumentId) {
    let Some((doc, meta)) = sharing::read(world, id) else {
        return;
    };
    let members: Vec<(String, String)> = meta
        .sharing
        .iter()
        .flat_map(|s| s.members.iter())
        .map(|m| (m.id.clone(), m.name.clone()))
        .collect();
    if members.is_empty() {
        crate::linked::toast(world, tr("shell-share-transfer-share-first"));
        return;
    }
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    world.commands().spawn((
        Dialog::new("transfer-dialog")
            .title(tr_with(
                "shell-share-transfer-title",
                &[("name", doc.name.as_str())],
            ))
            .width(460.0)
            .body(move |b| {
                let t = &tb;
                let mut select = Select::new("transfer-to").width(Val::Px(260.0));
                for (id, name) in &members {
                    let short: String = id.chars().take(10).collect();
                    select = select.option(format!("{name} · {short}"), true);
                }
                b.spawn(form_row(
                    t,
                    "transfer-to-row",
                    tr("shell-share-new-owner"),
                    110.0,
                ))
                .with_child(select.selected(0).build(t));
                b.spawn((
                    t.text(
                        tr("shell-share-transfer-hint"),
                        t.font_sm,
                        FontWeight::NORMAL,
                        t.muted_foreground,
                    ),
                    Translated("shell-share-transfer-hint"),
                    Node {
                        width: Val::Px(420.0),
                        margin: UiRect::top(Val::Px(8.0)),
                        ..default()
                    },
                ))
                .insert(TextLayout::default());
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("transfer-confirm")
                        .label(tr("shell-share-transfer"))
                        .primary()
                        .build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(confirm_transfer);
                    }),
                ));
                f.spawn((
                    Button::new("transfer-cancel")
                        .label(tr("common-cancel"))
                        .build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(close_transfer);
                    }),
                ));
            })
            .build(&theme),
        TransferDialog(id),
    ));
    world.flush();
}

fn close_transfer(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<TransferDialog>>();
    let roots: Vec<Entity> = q.iter(world).collect();
    for e in roots {
        world.trigger(DialogClose { entity: e });
    }
}

fn confirm_transfer(world: &mut World) {
    let mut q = world.query::<&TransferDialog>();
    let Some(TransferDialog(id)) = q.iter(world).next().copied() else {
        return;
    };
    let i = selected(world, "transfer-to");
    let to =
        sharing::read(world, id).and_then(|(_, m)| m.sharing?.members.get(i).map(|m| m.id.clone()));
    close_transfer(world);
    let Some(to) = to else { return };
    match sharing::transfer(world, id, &to) {
        Ok(()) => crate::linked::toast(world, tr("shell-share-transferring")),
        Err(why) => crate::linked::toast(world, why),
    }
}
