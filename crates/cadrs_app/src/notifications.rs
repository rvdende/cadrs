//! The bell at the top right (the documents page's bar and a document's): what cadrs has to
//! tell, with a dot while something is unread.
//!
//! - A newer release (from [`crate::update`]): opens its release notes.
//! - "What's new" once per edition ([`WHATS_NEW`]): where things moved in the new look. Opening
//!   the bell's menu marks it read, which the preferences remember.
//!
//! Its menu (`notifications-menu`) lists them newest first (`notice-<id>`), unread ones with a
//! dot; with none, "You're all caught up".

use bevy::prelude::*;
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::prelude::*;

use crate::update::UpdateStatus;

/// The edition of "What's new" this build announces: bump it (and its text) to announce
/// something new.
pub const WHATS_NEW: u32 = 1;

pub struct NotificationsPlugin;

impl Plugin for NotificationsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Notifications>()
            .add_systems(Update, (collect, sync_dots).chain())
            .add_observer(on_bell)
            .add_observer(on_bell_action);
    }
}

/// Something to tell.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub id: String,
    pub icon: &'static str,
    pub title: String,
    /// More, in the item's tooltip.
    pub body: String,
    pub unread: bool,
}

/// The notices, newest first.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct Notifications {
    pub items: Vec<Notice>,
}

impl Notifications {
    pub fn unread(&self) -> bool {
        self.items.iter().any(|n| n.unread)
    }

    /// Adds a notice, unless one with its id is there.
    pub fn push(&mut self, notice: Notice) {
        if !self.items.iter().any(|n| n.id == notice.id) {
            self.items.insert(0, notice);
        }
    }
}

/// The bell's unread dot.
#[derive(Component)]
struct BellDot;

/// The bell for a top bar: a plain icon button, as the top bar's others are.
pub fn spawn_bell(r: &mut ChildSpawnerCommands, t: &Theme) {
    r.spawn((
        Name::new("notifications-frame"),
        Node {
            flex_shrink: 0.0,
            ..default()
        },
    ))
    .with_children(|f| {
        f.spawn(
            IconButton::new("notifications", "notifications")
                .icon_size(16.0)
                .tooltip(tr("shell-notifications"))
                .build(t),
        )
        .entry::<Node>()
        .and_modify(|mut n| {
            n.width = Val::Px(32.0);
            n.height = Val::Px(32.0);
        })
        .entity()
        .with_child((
            Name::new("notifications-dot"),
            BellDot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(2.0),
                right: Val::Px(3.0),
                width: Val::Px(8.0),
                height: Val::Px(8.0),
                border: UiRect::all(Val::Px(1.5)),
                border_radius: BorderRadius::MAX,
                display: Display::None,
                ..default()
            },
            BackgroundColor(t.danger),
            BorderColor::all(t.background),
            Pickable::IGNORE,
        ));
    });
}

/// Turns what the app knows into notices.
fn collect(
    mut notes: ResMut<Notifications>,
    status: Res<UpdateStatus>,
    prefs: Option<Res<crate::preferences_ui::LocalPreferences>>,
    mut asked: Local<bool>,
) {
    if !*asked && let Some(prefs) = prefs {
        *asked = true;
        if prefs.0.whats_new_seen < WHATS_NEW {
            notes.push(Notice {
                id: "whats-new".into(),
                icon: "idea",
                title: tr("shell-notice-whats-new-title"),
                body: tr("shell-notice-whats-new-body"),
                unread: true,
            });
        }
    }
    if status.is_changed()
        && let UpdateStatus::Available(r) = &*status
    {
        notes.push(Notice {
            id: "update".into(),
            icon: "download",
            title: tr_with(
                "shell-notice-update-title",
                &[("version", r.version.as_str())],
            ),
            body: tr("shell-notice-update-body"),
            unread: true,
        });
    }
}

fn sync_dots(notes: Res<Notifications>, mut q: Query<(&mut Node, Ref<BellDot>)>) {
    let want = if notes.unread() {
        Display::Flex
    } else {
        Display::None
    };
    for (mut n, dot) in &mut q {
        if (notes.is_changed() || dot.is_added()) && n.display != want {
            n.display = want;
        }
    }
}

fn on_bell(
    a: On<Activate>,
    q: Query<&Name>,
    theme: Res<Theme>,
    mut notes: ResMut<Notifications>,
    mut commands: Commands,
) {
    if q.get(a.entity).map(|n| n.as_str()) == Ok("notifications") {
        open_notifications(&mut commands, a.entity, &theme, &mut notes);
    }
}

/// Opens the notices under `anchor` (the bell, or the account button while the bell is folded
/// into its menu, `crate::top_bar`) and marks them read. Picks come to `anchor`, for
/// [`notice_action`].
pub(crate) fn open_notifications(
    commands: &mut Commands,
    anchor: Entity,
    theme: &Theme,
    notes: &mut Notifications,
) {
    let mut menu = Menu::new("notifications-menu").align_end().min_width(260.0);
    if notes.items.is_empty() {
        menu = menu.item(MenuItem::new("notice-none", tr("shell-notice-none")).disabled(true));
    }
    for n in &notes.items {
        let mut item = MenuItem::new(format!("notice-{}", n.id), n.title.clone())
            .icon(n.icon)
            .tooltip(n.body.clone());
        if n.unread {
            item = item.dot(theme.primary);
        }
        menu = menu.item(item);
    }
    open_menu(commands, anchor, menu.build(theme));
    // Seen: the dot goes, and "What's new" won't come back.
    if notes.unread() {
        if notes.items.iter().any(|n| n.id == "whats-new" && n.unread) {
            commands
                .queue(|w: &mut World| crate::preferences_ui::mark_whats_new_seen(w, WHATS_NEW));
        }
        for n in &mut notes.items {
            n.unread = false;
        }
    }
}

fn on_bell_action(ev: On<MenuAction>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(|n| n.as_str()) == Ok("notifications") {
        notice_action(&mut commands, &ev.item);
    }
}

/// Carries out a pick from the notices (`notice-…`).
pub(crate) fn notice_action(commands: &mut Commands, item: &str) {
    if item == "notice-update" {
        commands.queue(crate::update::show_release_notes);
    }
}
