//! Whether cadrs is online for sharing and collaboration, and the top bar's network button
//! (issue #148).
//!
//! cadrs goes online (peer to peer, [`cadrs_net`]) only when the user asks: the button's
//! **Connect**, the **Connect** of a toast when sharing or joining needs the network, or at
//! start with **Connect when cadrs starts** (`Preferences::autoconnect_p2p`, off by default;
//! the button's menu and the Preferences dialog both set it). **Disconnect** goes offline
//! again: the open shared document's session ends and every room is left.
//!
//! The button (`network`) shows the state with icon-rs's network icons: struck through while
//! offline, plain arrows while online, the up arrow lit while sending and the down arrow while
//! receiving (from the once-a-second sample of the wire), an exclamation mark in red when going
//! online failed. Its menu is `network-menu`. It is only shown in runs that can go online (not
//! in scenarios, headless runs or with `CADRS_NET=0`).

use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use cadrs_net::CanConnect;
use cadrs_net::bevy_iroh::{Iroh, NetStats, StartError};
use cadrs_ui::button::visuals_for;
use cadrs_ui::icon::SolidTint;
use cadrs_ui::prelude::*;
use cadrs_ui::{
    ButtonVariant, Icon, MenuAction, MenuHeader, MenuItem, StateColors, Visuals, open_menu,
};

use crate::identity::IdentityList;
use crate::preferences_ui::LocalPreferences;
use crate::{DocumentStore, UserProfile};

/// Bytes a second, either way, that count as traffic: less is keep-alives.
const TRAFFIC: f64 = 64.0;

pub struct NetworkPlugin;

impl Plugin for NetworkPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_buttons)
            .add_observer(on_button)
            .add_observer(on_menu_action);
    }
}

/// Where the network stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetState {
    /// This run never goes online.
    Unavailable,
    Offline,
    /// The last try to go online failed.
    Failed,
    /// Online; `rx` and `tx` while traffic flows in and out.
    Online {
        rx: bool,
        tx: bool,
    },
}

impl NetState {
    pub fn of(world: &World) -> Self {
        if !world.contains_resource::<CanConnect>() {
            return NetState::Unavailable;
        }
        if world.contains_resource::<Iroh>() {
            let stats = world.get_resource::<NetStats>().and_then(|s| s.0.as_ref());
            return NetState::Online {
                rx: stats.is_some_and(|s| s.rx_rate > TRAFFIC),
                tx: stats.is_some_and(|s| s.tx_rate > TRAFFIC),
            };
        }
        if world.contains_resource::<StartError>() {
            NetState::Failed
        } else {
            NetState::Offline
        }
    }

    /// The icon-rs icon that shows it.
    pub fn icon(self) -> &'static str {
        match self {
            NetState::Unavailable | NetState::Offline => "network-disconnected",
            NetState::Failed => "network-error",
            NetState::Online {
                rx: false,
                tx: false,
            } => "network",
            NetState::Online {
                rx: true,
                tx: false,
            } => "network-rx",
            NetState::Online {
                rx: false,
                tx: true,
            } => "network-tx",
            NetState::Online { rx: true, tx: true } => "network-rx-tx",
        }
    }

    fn tooltip(self) -> &'static str {
        match self {
            NetState::Unavailable | NetState::Offline => "Offline: nothing is sent to the network",
            NetState::Failed => "Could not go online",
            NetState::Online { .. } => "Online for sharing (peer to peer)",
        }
    }
}

/// Takes cadrs online as the identity in use, with a toast if that fails. True when online.
pub fn connect(world: &mut World) -> bool {
    let root = world
        .get_resource::<DocumentStore>()
        .map(|s| s.0.root().to_path_buf());
    let (key, name) = match world.get_resource::<IdentityList>() {
        Some(list) => (
            root.map(|r| list.0.key_path(&r)),
            list.0.active().name.clone(),
        ),
        None => (None, world.resource::<UserProfile>().display_name.clone()),
    };
    match cadrs_net::start(world, key, name) {
        Ok(()) => true,
        Err(why) => {
            crate::linked::toast(world, format!("Cannot go online: {why}"));
            false
        }
    }
}

/// Takes cadrs offline: the open shared document's session ends first, so its room hears
/// goodbye.
pub fn disconnect(world: &mut World) {
    crate::collab::go_offline(world);
    cadrs_net::stop(world);
}

/// A toast saying `text` (what needs the network), with **Connect**, which goes online and
/// then does `then`. Without a way online in this run (`CADRS_NET=0`) the toast says so.
pub fn offer_connect(world: &mut World, text: &str, then: fn(&mut World)) {
    if !world.contains_resource::<CanConnect>() {
        crate::linked::toast(
            world,
            format!("{text}, and the network is off (CADRS_NET=0)"),
        );
        return;
    }
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    let toast = cadrs_ui::show_notification(
        &mut commands,
        &theme,
        cadrs_ui::Notification::info(format!("{text}, and cadrs is offline"))
            .name("network-offer-toast")
            .seconds(10.0),
    );
    let link = cadrs_ui::toast_action(
        &mut commands,
        &theme,
        toast,
        "network-offer-connect",
        "Connect",
    );
    commands
        .entity(link)
        .observe(move |_: On<Activate>, mut commands: Commands| {
            commands.queue(move |w: &mut World| {
                cadrs_ui::close_toasts(w);
                if connect(w) {
                    then(w);
                }
            });
        });
    world.flush();
}

// ---------------------------------------------------------------------------------------------
// The button

/// The top bar's network button.
#[derive(Component)]
struct NetButton;

/// The state a button shows.
#[derive(Component, Clone, Copy, PartialEq)]
struct Shows(NetState);

/// The network button, for a top bar's right side (hidden until [`sync_buttons`] knows the
/// state; for good in runs that can't go online).
pub fn spawn_button(r: &mut ChildSpawnerCommands, t: &Theme) {
    r.spawn((
        IconButton::new("network", NetState::Offline.icon())
            .icon_size(16.0)
            .build(t),
        NetButton,
    ))
    .entry::<Node>()
    .and_modify(|mut n| {
        n.width = Val::Px(32.0);
        n.height = Val::Px(32.0);
        n.flex_shrink = 0.0;
        n.display = Display::None;
    });
}

/// Puts the network's state on every network button: icon, tooltip, and red after a failure.
#[allow(clippy::type_complexity)]
fn sync_buttons(
    world: &mut World,
    q: &mut QueryState<(Entity, Option<&Shows>, &Children), With<NetButton>>,
) {
    let state = NetState::of(world);
    let stale: Vec<(Entity, Vec<Entity>)> = q
        .iter(world)
        .filter(|(_, shows, _)| shows.map(|s| s.0) != Some(state))
        .map(|(e, _, children)| (e, children.iter().collect()))
        .collect();
    if stale.is_empty() {
        return;
    }
    let theme = world.resource::<Theme>().clone();
    let mut visuals: Visuals = visuals_for(&theme, ButtonVariant::Ghost);
    match state {
        NetState::Failed => visuals.foreground = StateColors::all(theme.danger),
        // Online reads as on while quiet too: both arrows in a softer accent, the lit one full.
        NetState::Online { .. } => {
            visuals.foreground = StateColors::all(theme.primary.with_alpha(0.65))
        }
        _ => {}
    }
    for (button, children) in stale {
        let mut e = world.entity_mut(button);
        e.insert((Shows(state), visuals.clone(), Tooltip::new(state.tooltip())));
        if let Some(mut n) = e.get_mut::<Node>() {
            n.display = if state == NetState::Unavailable {
                Display::None
            } else {
                Display::Flex
            };
        }
        for child in children {
            let Some(mut icon) = world.get_mut::<Icon>(child) else {
                continue;
            };
            if icon.name != state.icon() {
                icon.name = state.icon().into();
            }
            // Offline and failed draw wholly in the button's colour: muted, or red; online, the
            // lit arrows keep the accent.
            let mut e = world.entity_mut(child);
            if matches!(state, NetState::Online { .. }) {
                e.remove::<SolidTint>();
            } else {
                e.insert(SolidTint);
            }
        }
    }
}

fn rate(bytes_per_second: f64) -> String {
    if bytes_per_second >= 1e6 {
        format!("{:.1} MB/s", bytes_per_second / 1e6)
    } else if bytes_per_second >= 1e3 {
        format!("{:.1} kB/s", bytes_per_second / 1e3)
    } else {
        format!("{bytes_per_second:.0} B/s")
    }
}

fn on_button(a: On<Activate>, q: Query<(), With<NetButton>>, mut commands: Commands) {
    if !q.contains(a.entity) {
        return;
    }
    let anchor = a.entity;
    commands.queue(move |world: &mut World| {
        let state = NetState::of(world);
        // Who else is in the shared document open now: their name, and where they are and
        // what they do.
        let people: Vec<(String, String, String)> = world
            .get_resource::<crate::collab::presence::People>()
            .map(|p| {
                let doc = world.get_resource::<crate::ActiveDocument>();
                p.by_id
                    .iter()
                    .map(|(id, person)| {
                        let doing = p.doing.get(id).map(|a| a.what.as_str());
                        let idle = p.idle_minutes(
                            id,
                            world.resource::<Time<Real>>().elapsed_secs_f64(),
                            world
                                .get_resource::<crate::collab::pointers::Pointers>()
                                .and_then(|ps| ps.moved_at(id)),
                        );
                        let place = doc.map_or_else(String::new, |d| {
                            crate::collab::presence::with_idle(
                                crate::collab::presence::whereabouts(d, &person.presence, doing),
                                idle,
                            )
                        });
                        // After the name, lower case: "Alex · in PCB Studio 1".
                        let place = match place.strip_prefix("In ") {
                            Some(rest) => format!("in {rest}"),
                            None => place,
                        };
                        (id.clone(), person.presence.name.clone(), place)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let (title, subtitle) = match state {
            NetState::Online { .. } => {
                let stats = world.get_resource::<NetStats>().and_then(|s| s.0.clone());
                // The rates on a line of their own.
                // Not while nothing moves (zeros read as a stalled connection).
                let rates = stats
                    .filter(|s| s.tx_rate >= 1.0 || s.rx_rate >= 1.0)
                    .map_or_else(String::new, |s| {
                        format!(
                            "\nSending {} · receiving {}",
                            rate(s.tx_rate),
                            rate(s.rx_rate)
                        )
                    });
                let who = match people.len() {
                    0 => "No one else in this document".to_string(),
                    1 => "1 other person here".to_string(),
                    n => format!("{n} others here"),
                };
                ("Online", format!("{who}{rates}"))
            }
            NetState::Failed => (
                "Could not go online",
                world
                    .get_resource::<StartError>()
                    .map_or_else(String::new, |e| e.0.clone()),
            ),
            _ => ("Offline", "Nothing is sent to the network".to_string()),
        };
        let toggle = if matches!(state, NetState::Online { .. }) {
            MenuItem::new("network-disconnect", "Disconnect").icon("network-disconnected")
        } else {
            MenuItem::new("network-connect", "Connect").icon("network")
        };
        let autoconnect = world.resource::<LocalPreferences>().0.autoconnect_p2p;
        let following = world
            .get_resource::<crate::collab::presence::Following>()
            .map(|f| f.leader.clone());
        let mut menu = cadrs_ui::Menu::new("network-menu")
            .align_end()
            .min_width(260.0)
            .header(MenuHeader::new(title).subtitle(subtitle))
            .separator();
        // Each person: their avatar and where they are; Follow (or Stop following) and Go to.
        for (id, name, place) in &people {
            let following_them = following.as_ref() == Some(id);
            let label = if place.is_empty() {
                name.clone()
            } else {
                format!("{name} · {place}")
            };
            let follow = if following_them {
                MenuItem::new(format!("network-follow-{id}"), "Stop following").icon("close")
            } else {
                MenuItem::new(format!("network-follow-{id}"), format!("Follow {name}"))
                    .icon("visible")
            };
            menu = menu.item(
                MenuItem::new(format!("network-person-{id}"), label)
                    .avatar(name.clone(), crate::collab::presence::color_of(id))
                    .checked(following_them)
                    .submenu(vec![
                        follow.into(),
                        MenuItem::new(format!("network-goto-{id}"), format!("Go to {name}'s view"))
                            .icon("location")
                            .into(),
                    ]),
            );
        }
        // An invitation: the Share dialog (link sharing, or people by their cadrs ID).
        if matches!(state, NetState::Online { .. })
            && world.get_resource::<crate::ActiveDocument>().is_some()
        {
            menu = menu.item(
                MenuItem::new("network-invite", "Invite people…")
                    .icon("share")
                    .tooltip("Share this document: by link, or with people by their cadrs ID"),
            );
        }
        // (A separator twice in a row shows as one.)
        let menu = menu.separator().item(toggle).item(
            MenuItem::new("network-autoconnect", "Connect when cadrs starts").checked(autoconnect),
        );
        let theme = world.resource::<Theme>().clone();
        open_menu(&mut world.commands(), anchor, menu.build(&theme));
        world.flush();
    });
}

fn on_menu_action(ev: On<MenuAction>, q: Query<(), With<NetButton>>, mut commands: Commands) {
    if !q.contains(ev.entity) {
        return;
    }
    match ev.item.as_str() {
        "network-connect" => commands.queue(|w: &mut World| {
            connect(w);
        }),
        "network-disconnect" => commands.queue(disconnect),
        "network-autoconnect" => commands.queue(|w: &mut World| {
            let on = !w.resource::<LocalPreferences>().0.autoconnect_p2p;
            crate::preferences_ui::set_autoconnect_p2p(w, on);
        }),
        "network-invite" => commands.queue(crate::collab::open_share),
        item => {
            if let Some(id) = item
                .strip_prefix("network-follow-")
                .or_else(|| item.strip_prefix("network-person-"))
            {
                let id = id.to_string();
                commands.queue(move |w: &mut World| crate::collab::presence::toggle_follow(w, id));
            } else if let Some(id) = item.strip_prefix("network-goto-") {
                let id = id.to_string();
                commands.queue(move |w: &mut World| crate::collab::presence::go_to(w, &id));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_state_has_its_own_icon_and_every_icon_exists() {
        let states = [
            NetState::Offline,
            NetState::Failed,
            NetState::Online {
                rx: false,
                tx: false,
            },
            NetState::Online {
                rx: true,
                tx: false,
            },
            NetState::Online {
                rx: false,
                tx: true,
            },
            NetState::Online { rx: true, tx: true },
        ];
        let icons: std::collections::HashSet<_> = states.iter().map(|s| s.icon()).collect();
        assert_eq!(icons.len(), states.len());
        for icon in icons {
            assert!(
                icon_rs::get(icon).is_some(),
                "{icon} is not an icon-rs icon"
            );
        }
    }

    #[test]
    fn rates_read_in_bytes_kilobytes_and_megabytes() {
        assert_eq!(rate(12.0), "12 B/s");
        assert_eq!(rate(2_500.0), "2.5 kB/s");
        assert_eq!(rate(3_200_000.0), "3.2 MB/s");
    }
}
