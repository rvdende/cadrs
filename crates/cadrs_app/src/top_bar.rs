//! The top bars on a narrow window (a phone's browser). Their right side folds into the account
//! menu piece by piece as the window narrows, the version first, until only the account's
//! avatar is left. [`cadrs_ui::HideBelow`] hides each piece below its [`Fold`]'s width (most
//! in a wrapper, [`spawn_folding`], as the network button hides itself at times), and the
//! account menu (`crate::identity`) lists what is folded.

use bevy::prelude::*;
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::{HideBelow, MenuEntry, MenuItem, SchemePreference, Theme};

use crate::network::NetState;
use crate::notifications::Notifications;
use crate::update::UpdateStatus;

pub struct TopBarPlugin;

impl Plugin for TopBarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, open_next);
    }
}

/// A folded piece's menu, to open under the account button (this entity) once the account
/// menu has closed: a pick closes every menu after it is carried out, so it waits a frame.
#[derive(Component, Debug, Clone, Copy)]
enum OpenNext {
    Notifications,
    Network,
}

fn open_next(q: Query<(Entity, &OpenNext)>, mut commands: Commands) {
    for (anchor, next) in &q {
        commands.entity(anchor).remove::<OpenNext>();
        match next {
            OpenNext::Network => crate::network::open_network_menu(&mut commands, anchor),
            OpenNext::Notifications => commands.queue(move |world: &mut World| {
                let theme = world.resource::<Theme>().clone();
                world.resource_scope(|world, mut notes: Mut<Notifications>| {
                    let mut commands = world.commands();
                    crate::notifications::open_notifications(
                        &mut commands,
                        anchor,
                        &theme,
                        &mut notes,
                    );
                });
                world.flush();
            }),
        }
    }
}

/// What folds, in the order it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fold {
    /// The version, and the update button beside it.
    Version,
    /// The light / dark / system toggle.
    Scheme,
    /// The network button.
    Network,
    /// The notifications bell.
    Notifications,
    /// Comments, History and Share, the others' avatars and the account's name: a phone, where
    /// the account button is the avatar alone.
    Phone,
}

impl Fold {
    /// The window width (logical px) it folds below.
    pub const fn below(self) -> f32 {
        match self {
            Fold::Version => 1024.0,
            Fold::Scheme => 900.0,
            Fold::Network => 800.0,
            Fold::Notifications => 700.0,
            Fold::Phone => 600.0,
        }
    }

    pub fn folded(self, width: f32) -> bool {
        HideBelow(self.below()).hides(width)
    }

    fn name(self) -> &'static str {
        match self {
            Fold::Version => "fold-version",
            Fold::Scheme => "fold-scheme",
            Fold::Network => "fold-network",
            Fold::Notifications => "fold-notifications",
            Fold::Phone => "fold-phone",
        }
    }
}

/// Spawns a top bar's piece in a wrapper that hides it below `fold`'s width.
pub fn spawn_folding(
    r: &mut ChildSpawnerCommands,
    fold: Fold,
    piece: impl FnOnce(&mut ChildSpawnerCommands),
) {
    r.spawn((
        Name::new(fold.name()),
        HideBelow(fold.below()),
        Node {
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            ..default()
        },
    ))
    .with_children(piece);
}

/// The light / dark / system choices, as the toggle has them (their labels' catalog keys).
const SCHEMES: [(SchemePreference, &str, &str); 3] = [
    (
        SchemePreference::Light,
        "ui-color-scheme-light",
        "theme-light-medium",
    ),
    (SchemePreference::Dark, "ui-color-scheme-dark", "theme-dark"),
    (
        SchemePreference::System,
        "ui-color-scheme-system",
        "theme-system",
    ),
];

/// The account menu's entries for what is folded on a window `width` wide (none on a wide
/// one), each named `account-…` for [`fold_action`].
pub fn menu_entries(world: &World, width: f32, theme: &Theme) -> Vec<MenuEntry> {
    let mut entries: Vec<MenuEntry> = Vec::new();
    let in_document = world.get_resource::<crate::ActiveDocument>().is_some();
    if Fold::Phone.folded(width) && in_document {
        for (name, label, icon) in [
            ("account-comments", "shell-top-bar-comments", "comments"),
            ("account-history", "shell-top-bar-history", "history"),
            ("account-share", "shell-top-bar-share", "share"),
        ] {
            entries.push(MenuItem::new(name, tr(label)).icon(icon).into());
        }
    }
    if Fold::Notifications.folded(width) {
        let mut item =
            MenuItem::new("account-notifications", tr("shell-notifications")).icon("notifications");
        if world.resource::<Notifications>().unread() {
            item = item.dot(theme.danger);
        }
        entries.push(item.into());
    }
    let net = NetState::of(world);
    if Fold::Network.folded(width) && net != NetState::Unavailable {
        let label = match net {
            NetState::Online { .. } => "shell-top-bar-network-online",
            NetState::Failed => "shell-top-bar-network-failed",
            _ => "shell-top-bar-network-offline",
        };
        entries.push(
            MenuItem::new("account-network", tr(label))
                .icon(net.icon())
                .into(),
        );
    }
    if Fold::Scheme.folded(width) {
        entries.push(MenuEntry::Separator);
        let preference = world.resource::<cadrs_ui::Appearance>().preference;
        for (i, (scheme, label, icon)) in SCHEMES.iter().enumerate() {
            entries.push(
                MenuItem::new(format!("account-scheme-{i}"), tr(label))
                    .icon(*icon)
                    .checked(*scheme == preference)
                    .into(),
            );
        }
    }
    if Fold::Version.folded(width) {
        entries.push(MenuEntry::Separator);
        let item = match world.resource::<UpdateStatus>().release() {
            Some(r) => MenuItem::new(
                "account-update",
                tr_with("shell-top-bar-update", &[("version", r.version.as_str())]),
            )
            .icon("download")
            .tooltip(tr("shell-top-bar-update-tooltip")),
            None => MenuItem::new(
                "account-version",
                concat!("cadrs v", env!("CARGO_PKG_VERSION")),
            )
            .icon("info")
            .disabled(true),
        };
        entries.push(item.into());
    }
    entries
}

/// Carries out a pick of the account menu's folded entries, or of the network or notices
/// menu opened from them under the account button (`anchor`). Whether it was one.
pub fn fold_action(commands: &mut Commands, anchor: Entity, item: &str) -> bool {
    match item {
        "account-comments" => commands.queue(crate::collab::comments::toggle_panel),
        "account-history" => commands.queue(|world: &mut World| {
            world
                .resource_mut::<crate::history_panel::HistoryPanel>()
                .toggle();
        }),
        "account-share" => commands.queue(crate::collab::open_share),
        "account-notifications" => {
            commands.entity(anchor).insert(OpenNext::Notifications);
        }
        "account-network" => {
            commands.entity(anchor).insert(OpenNext::Network);
        }
        "account-update" => commands.queue(crate::update::show_release_notes),
        _ if item.starts_with("network-") => crate::network::network_menu_action(commands, item),
        _ if item.starts_with("notice-") => crate::notifications::notice_action(commands, item),
        _ => {
            let Some((scheme, ..)) = item
                .strip_prefix("account-scheme-")
                .and_then(|i| i.parse::<usize>().ok())
                .and_then(|i| SCHEMES.get(i))
            else {
                return false;
            };
            let scheme = *scheme;
            commands.queue(move |world: &mut World| {
                world.resource_mut::<cadrs_ui::Appearance>().preference = scheme;
            });
        }
    }
    true
}
