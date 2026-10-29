//! The account menu (top right, on the documents page and in a document): who the user is to
//! collaborators. The button shows the identity in use, its avatar and name; its menu shows
//! its cadrs ID, switches to another identity, makes a new one and renames this one.
//!
//! Identities are kept on this computer ([`cadrs_core::identity`]), each with its own key, so
//! each has a cadrs ID of its own. The key is the network node's, so switching with the
//! network on starts cadrs again as the other identity (the open document is saved first);
//! without it (`CADRS_NET=0`, scripted runs) the switch is immediate.

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::identity::{DEFAULT_KEY, Identities, Identity};
use cadrs_net::bevy_iroh::Iroh;
use cadrs_ui::avatar::Avatar;
use cadrs_ui::input::{TextInputField, TextSubmit};
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, DialogClose, MenuAction, MenuHeader, MenuItem, open_menu};

use crate::{AppState, DocumentStore, UserProfile};

pub struct IdentityPlugin;

impl Plugin for IdentityPlugin {
    fn build(&self, app: &mut App) {
        // Read now, so the documents page already shows who is working.
        let ids = app
            .world()
            .get_resource::<DocumentStore>()
            .map(|s| s.0.root().to_path_buf());
        let profile = app
            .world()
            .get_resource::<UserProfile>()
            .cloned()
            .unwrap_or_else(UserProfile::system);
        let first = first_identity(&profile);
        let list = match ids {
            Some(root) => Identities::load(&root, first),
            None => Identities::new(first),
        };
        app.insert_resource(profile_of(list.active()))
            .insert_resource(IdentityList(list))
            .add_systems(Update, sync_identity_buttons)
            .add_observer(on_identity)
            .add_observer(on_identity_action)
            .add_observer(on_dialog_submit);
    }
}

/// The identities on this computer, and which is in use.
#[derive(Resource, Debug, Clone)]
pub struct IdentityList(pub Identities);

/// The identities kept in a store root, the OS user's first.
pub fn load(root: &std::path::Path) -> Identities {
    Identities::load(root, first_identity(&UserProfile::system()))
}

/// The identity cadrs always had: the profile's, with the original key.
fn first_identity(p: &UserProfile) -> Identity {
    Identity {
        id: p.id.clone(),
        name: p.display_name.clone(),
        key: DEFAULT_KEY.into(),
    }
}

fn profile_of(i: &Identity) -> UserProfile {
    UserProfile {
        id: i.id.clone(),
        display_name: i.name.clone(),
    }
}

/// The account button.
#[derive(Component)]
struct IdentityButton;

/// The identity dialogs: a new identity, or this one's profile.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum IdentityDialog {
    New,
    Edit,
}

/// The account button: the avatar and name of the identity in use, and a caret.
pub fn spawn_identity_button(r: &mut ChildSpawnerCommands, t: &Theme, profile: &UserProfile) {
    let name = profile.display_name.clone();
    r.spawn((
        Button::new("identity").ghost().tooltip("Account").build(t),
        IdentityButton,
    ))
    .entry::<Node>()
    .and_modify(|mut n| {
        n.height = Val::Px(32.0);
        n.padding = UiRect::new(Val::Px(4.0), Val::Px(8.0), Val::ZERO, Val::ZERO);
        n.column_gap = Val::Px(8.0);
    })
    .entity()
    .with_children(|b| button_content(b, t, &name));
}

fn button_content(b: &mut ChildSpawnerCommands, t: &Theme, name: &str) {
    b.spawn((
        Avatar::new("identity-avatar", name).size(24.0).build(t),
        Pickable::IGNORE,
    ));
    // On a phone, the avatar alone.
    let phone = cadrs_ui::HideBelow(crate::top_bar::Fold::Phone.below());
    b.spawn((
        Name::new("identity-name"),
        t.text(name, t.font_base, FontWeight::MEDIUM, t.foreground),
        cadrs_ui::style::InheritFg,
        Pickable::IGNORE,
        phone,
    ));
    b.spawn((
        icon("caret-down", 12.0, t.muted_foreground),
        cadrs_ui::style::InheritFg,
        Pickable::IGNORE,
        phone,
    ));
}

/// Keeps the account buttons on the identity in use (after a switch or a rename).
fn sync_identity_buttons(
    profile: Res<UserProfile>,
    theme: Res<Theme>,
    q: Query<Entity, With<IdentityButton>>,
    mut commands: Commands,
) {
    if !profile.is_changed() || profile.is_added() {
        return;
    }
    for e in &q {
        commands
            .entity(e)
            .despawn_related::<Children>()
            .with_children(|b| button_content(b, &theme, &profile.display_name));
    }
}

/// A cadrs ID shortened for the menu: its start and end.
fn short_id(id: &str) -> String {
    if id.len() <= 14 {
        id.to_string()
    } else {
        format!("{}…{}", &id[..6], &id[id.len() - 6..])
    }
}

fn on_identity(
    a: On<Activate>,
    q: Query<&bevy::ui::ComputedUiRenderTargetInfo, With<IdentityButton>>,
    mut commands: Commands,
) {
    let Ok(target) = q.get(a.entity) else {
        return;
    };
    // On a narrow window it also holds what the top bar folded (`crate::top_bar`).
    let width = target.logical_size().x;
    let anchor = a.entity;
    commands.queue(move |world: &mut World| {
        let theme = world.resource::<Theme>().clone();
        let menu = identity_menu(world, width, &theme);
        open_menu(&mut world.commands(), anchor, menu.build(&theme));
        world.flush();
    });
}

fn identity_menu(world: &World, width: f32, theme: &Theme) -> cadrs_ui::Menu {
    let ids = &world.resource::<IdentityList>().0;
    let cadrs_id = cadrs_net::cadrs_id(world.get_resource::<Iroh>());
    let subtitle = cadrs_id.as_deref().map_or_else(
        || "Offline: on this computer only".to_string(),
        |id| format!("cadrs ID {}", short_id(id)),
    );
    let mut menu = cadrs_ui::Menu::new("identity-menu")
        .align_end()
        .min_width(240.0)
        .header(
            MenuHeader::new(ids.active().name.clone())
                .subtitle(subtitle)
                .avatar(),
        )
        .separator();
    if ids.list.len() > 1 {
        for (i, ident) in ids.list.iter().enumerate() {
            menu = menu.item(
                MenuItem::new(format!("identity-use-{i}"), ident.name.clone())
                    .dot(Avatar::color_for(&ident.name))
                    .checked(i == ids.active),
            );
        }
        menu = menu.separator();
    }
    menu = menu
        .item(MenuItem::new("identity-new", "New identity…").icon("plus"))
        .item(MenuItem::new("identity-edit", "Edit profile…").icon("user"))
        .item(if cadrs_id.is_some() {
            MenuItem::new("identity-copy-id", "Copy cadrs ID").icon("copy")
        } else {
            MenuItem::new("identity-copy-id", "Copy cadrs ID")
                .icon("copy")
                .disabled(true)
                .tooltip("cadrs is offline: no cadrs ID to share")
        })
        .separator();
    for entry in crate::top_bar::menu_entries(world, width, theme) {
        menu = menu.entry(entry);
    }
    menu.separator()
        .item(MenuItem::new("identity-preferences", "Preferences…").icon("settings"))
}

fn on_identity_action(
    ev: On<MenuAction>,
    q: Query<(), With<IdentityButton>>,
    mut commands: Commands,
) {
    if !q.contains(ev.entity) || crate::top_bar::fold_action(&mut commands, ev.entity, &ev.item) {
        return;
    }
    let item = ev.item.clone();
    commands.queue(move |world: &mut World| match item.as_str() {
        "identity-new" => open_dialog(world, IdentityDialog::New),
        "identity-edit" => open_dialog(world, IdentityDialog::Edit),
        "identity-copy-id" => copy_id(world),
        "identity-preferences" => crate::preferences_ui::open_preferences(world),
        i => {
            if let Some(n) = i
                .strip_prefix("identity-use-")
                .and_then(|n| n.parse::<usize>().ok())
            {
                switch_to(world, n);
            }
        }
    });
}

fn copy_id(world: &mut World) {
    let Some(id) = cadrs_net::cadrs_id(world.get_resource::<Iroh>()) else {
        return;
    };
    if let Some(mut clip) = world.get_resource_mut::<bevy::clipboard::Clipboard>() {
        let _ = clip.set_text(id);
    }
    crate::linked::toast(world, "Copied your cadrs ID");
}

fn open_dialog(world: &mut World, kind: IdentityDialog) {
    let theme = world.resource::<Theme>().clone();
    let (t, tb, tf) = (theme.clone(), theme.clone(), theme.clone());
    let (name, title, ok, note) = match kind {
        IdentityDialog::New => (
            String::new(),
            "New identity",
            "Create",
            "A new identity has a cadrs ID of its own: collaborators see it as someone else.",
        ),
        IdentityDialog::Edit => (
            world.resource::<UserProfile>().display_name.clone(),
            "Edit profile",
            "Save",
            "Collaborators see this name beside your cursor, comments and versions.",
        ),
    };
    let mut commands = world.commands();
    commands.spawn((
        Dialog::new("identity-dialog")
            .title(title)
            .width(420.0)
            .body(move |b| {
                b.spawn(tb.text("Name", tb.font_sm, FontWeight::MEDIUM, tb.muted_foreground));
                b.spawn(
                    TextInput::new("identity-name")
                        .placeholder("Your name")
                        .value(name)
                        .select_all_on_focus()
                        .autofocus()
                        .build(&tb),
                );
                b.spawn((
                    tb.text(note, tb.font_sm, FontWeight::NORMAL, tb.muted_foreground),
                    Node {
                        max_width: Val::Px(380.0),
                        ..default()
                    },
                ));
            })
            .footer(move |f| {
                f.spawn((
                    Button::new("identity-dialog-ok")
                        .label(ok)
                        .primary()
                        .build(&tf),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(confirm);
                    }),
                ));
                f.spawn((
                    Button::new("identity-dialog-cancel")
                        .label("Cancel")
                        .build(&tf),
                    observe(
                        |_: On<Activate>,
                         q: Query<Entity, With<IdentityDialog>>,
                         mut commands: Commands| {
                            for e in &q {
                                commands.trigger(DialogClose { entity: e });
                            }
                        },
                    ),
                ));
            })
            .build(&t),
        kind,
    ));
    world.flush();
}

/// Enter in the name field is OK.
fn on_dialog_submit(
    ev: On<TextSubmit>,
    q: Query<(), With<IdentityDialog>>,
    q_parent: Query<&ChildOf>,
    mut commands: Commands,
) {
    if q_parent.iter_ancestors(ev.entity).any(|a| q.contains(a)) {
        commands.queue(confirm);
    }
}

fn confirm(world: &mut World) {
    let mut q = world.query::<(Entity, &IdentityDialog)>();
    let Some((dialog, kind)) = q.iter(world).next().map(|(e, k)| (e, *k)) else {
        return;
    };
    let mut fields =
        world.query_filtered::<(&Name, &bevy::text::EditableText), With<TextInputField>>();
    let name = fields
        .iter(world)
        .find(|(n, _)| n.as_str() == "identity-name-field")
        .map(|(_, t)| t.value().to_string().trim().to_string())
        .unwrap_or_default();
    if name.is_empty() {
        return;
    }
    world.trigger(DialogClose { entity: dialog });
    match kind {
        IdentityDialog::New => {
            let n = world.resource_mut::<IdentityList>().0.add(&name);
            save(world);
            switch_to(world, n);
        }
        IdentityDialog::Edit => {
            let mut list = world.resource_mut::<IdentityList>();
            let active = list.0.active;
            list.0.list[active].name = name.clone();
            save(world);
            world.resource_mut::<UserProfile>().display_name = name;
            crate::linked::toast(world, "Profile saved");
        }
    }
}

fn save(world: &mut World) {
    let Some(root) = world
        .get_resource::<DocumentStore>()
        .map(|s| s.0.root().to_path_buf())
    else {
        return;
    };
    if let Err(e) = world.resource::<IdentityList>().0.save(&root) {
        error!("cannot save the identities: {e}");
    }
}

/// Makes identity `index` the one in use: at once without the network, else by starting
/// cadrs again (the key is the network node's).
fn switch_to(world: &mut World, index: usize) {
    if index >= world.resource::<IdentityList>().0.list.len() {
        return;
    }
    // What was edited so far is saved as the identity that edited it.
    if *world.resource::<State<AppState>>().get() == AppState::Document {
        let _ = world.run_system_cached(crate::document::save_on_exit);
    }
    world.resource_mut::<IdentityList>().0.active = index;
    save(world);
    let ident = world.resource::<IdentityList>().0.active().clone();
    *world.resource_mut::<UserProfile>() = profile_of(&ident);
    if world.contains_resource::<Iroh>() {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        match std::env::current_exe()
            .and_then(|exe| std::process::Command::new(exe).args(args).spawn())
        {
            Ok(_) => {
                world.write_message(AppExit::Success);
                return;
            }
            Err(e) => warn!("cannot start cadrs again: {e}"),
        }
        crate::linked::toast(
            world,
            format!("Restart cadrs to go online as {}", ident.name),
        );
        return;
    }
    crate::linked::toast(world, format!("Working as {}", ident.name));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_cadrs_id_is_shortened_in_the_middle() {
        assert_eq!(short_id("abc"), "abc");
        assert_eq!(
            short_id("0123456789abcdef0123456789abcdef"),
            "012345…abcdef"
        );
    }
}
