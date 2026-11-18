//! The top bar's AI assistants button (`mcp`): a shortcut to the MCP server
//! (`crate::mcp`), which Preferences › AI assistants (MCP) also sets.
//!
//! The button shows a robot: its eyes lit (in the accent) while the server serves, struck
//! through while it is off, red when it could not start. Its menu (`mcp-menu`) says the same as
//! the Preferences' section: whether it is on and the endpoint it serves (the header), **Enable
//! MCP server** (with the Preferences' note as its tooltip), **Port: …** (a popup to type
//! another, `mcp-port-popup`), **Reset port to default**, **Copy endpoint** and
//! **Preferences…**. Every change is saved at once, and the server restarts on the new port.
//!
//! Not in the browser build, which can't serve one.

use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use cadrs_core::preferences::DEFAULT_MCP_PORT;
use cadrs_ui::button::visuals_for;
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::icon::SolidTint;
use cadrs_ui::prelude::*;
use cadrs_ui::{
    ButtonVariant, Icon, MenuAction, MenuHeader, MenuItem, NamePopupCommit, StateColors, Visuals,
    open_menu,
};

use crate::mcp::McpServer;
use crate::preferences_ui::LocalPreferences;

pub struct McpButtonPlugin;

impl Plugin for McpButtonPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_buttons)
            .add_observer(on_button)
            .add_observer(on_menu_action)
            .add_observer(on_port_commit);
    }
}

/// Where the MCP server stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpState {
    Off,
    /// Serving at this endpoint.
    On(String),
    /// Turned on, but it could not start (the port taken): why.
    Failed(String),
}

impl McpState {
    pub fn of(world: &World) -> Self {
        let server = world.resource::<McpServer>();
        if let Some(url) = server.url() {
            return McpState::On(url);
        }
        match server.error() {
            Some(e) if world.resource::<LocalPreferences>().0.mcp.enabled => {
                McpState::Failed(e.to_string())
            }
            _ => McpState::Off,
        }
    }

    /// The cadrs_icons icon that shows it.
    pub fn icon(&self) -> &'static str {
        match self {
            McpState::On(_) => "assistant",
            McpState::Off | McpState::Failed(_) => "assistant-off",
        }
    }

    /// The button's tooltip (and its entry in the account menu).
    pub fn label(&self) -> String {
        match self {
            McpState::On(_) => tr("shell-mcp-tooltip-on"),
            McpState::Off => tr("shell-mcp-tooltip-off"),
            McpState::Failed(_) => tr("shell-mcp-tooltip-failed"),
        }
    }
}

/// The top bar's AI assistants button.
#[derive(Component)]
struct McpButton;

/// The state a button shows.
#[derive(Component, Clone, PartialEq)]
struct Shows(McpState);

/// The AI assistants button, for a top bar's right side.
pub fn spawn_button(r: &mut ChildSpawnerCommands, t: &Theme) {
    r.spawn((
        IconButton::new("mcp", McpState::Off.icon())
            .icon_size(16.0)
            .build(t),
        McpButton,
    ))
    .entry::<Node>()
    .and_modify(|mut n| {
        n.width = Val::Px(32.0);
        n.height = Val::Px(32.0);
        n.flex_shrink = 0.0;
    });
}

/// Puts the server's state on every AI assistants button: icon, tooltip and colour.
#[allow(clippy::type_complexity)]
fn sync_buttons(
    world: &mut World,
    q: &mut QueryState<(Entity, Option<&Shows>, &Children), With<McpButton>>,
) {
    let state = McpState::of(world);
    let stale: Vec<(Entity, Vec<Entity>)> = q
        .iter(world)
        .filter(|(_, shows, _)| shows.map(|s| &s.0) != Some(&state))
        .map(|(e, _, children)| (e, children.iter().collect()))
        .collect();
    if stale.is_empty() {
        return;
    }
    let theme = world.resource::<Theme>().clone();
    let mut visuals: Visuals = visuals_for(&theme, ButtonVariant::Ghost);
    match state {
        McpState::Failed(_) => visuals.foreground = StateColors::all(theme.danger),
        // On, as the network button reads online: the head in a softer accent, the eyes full.
        McpState::On(_) => visuals.foreground = StateColors::all(theme.primary.with_alpha(0.65)),
        McpState::Off => {}
    }
    for (button, children) in stale {
        world.entity_mut(button).insert((
            Shows(state.clone()),
            visuals.clone(),
            Tooltip::new(state.label()),
        ));
        for child in children {
            let Some(mut icon) = world.get_mut::<Icon>(child) else {
                continue;
            };
            if icon.name != state.icon() {
                icon.name = state.icon().into();
            }
            let mut e = world.entity_mut(child);
            if matches!(state, McpState::On(_)) {
                e.remove::<SolidTint>();
            } else {
                e.insert(SolidTint);
            }
        }
    }
}

fn on_button(a: On<Activate>, q: Query<(), With<McpButton>>, mut commands: Commands) {
    if q.contains(a.entity) {
        open_mcp_menu(&mut commands, a.entity);
    }
}

/// The endpoint the server serves, or would, on `port`.
pub fn endpoint(port: u16) -> String {
    cadrs_mcp::url(std::net::SocketAddr::from((
        std::net::Ipv4Addr::LOCALHOST,
        port,
    )))
}

/// Opens the AI assistants menu under `anchor`: the button, or the account button while the
/// button is folded into its menu (`crate::top_bar`). Its picks come to `anchor`, for
/// [`mcp_menu_action`].
pub(crate) fn open_mcp_menu(commands: &mut Commands, anchor: Entity) {
    commands.queue(move |world: &mut World| {
        let state = McpState::of(world);
        let mcp = world.resource::<LocalPreferences>().0.mcp;
        let subtitle = match &state {
            McpState::On(url) => tr_with("shell-mcp-serving", &[("url", url)]),
            McpState::Off => tr_with("shell-mcp-off", &[("url", &endpoint(mcp.port))]),
            McpState::Failed(why) => format!(
                "{}\n{why}",
                tr_with("shell-mcp-failed", &[("port", &mcp.port.to_string())])
            ),
        };
        let port = mcp.port.to_string();
        let default = DEFAULT_MCP_PORT.to_string();
        let menu = cadrs_ui::Menu::new("mcp-menu")
            .align_end()
            .min_width(280.0)
            .header(MenuHeader::new(tr("shell-prefs-mcp-heading")).subtitle(subtitle))
            .separator()
            .item(
                MenuItem::new("mcp-enabled", tr("shell-mcp-enable"))
                    .checked(mcp.enabled)
                    .tooltip(tr("shell-prefs-mcp-note")),
            )
            .item(
                MenuItem::new("mcp-port", tr_with("shell-mcp-port", &[("port", &port)]))
                    .icon("edit")
                    .tooltip(tr("shell-mcp-port-tooltip")),
            )
            .item(
                MenuItem::new(
                    "mcp-port-reset",
                    tr_with("shell-mcp-port-reset", &[("port", &default)]),
                )
                .icon("undo")
                .disabled(mcp.port == DEFAULT_MCP_PORT),
            )
            .item(
                MenuItem::new("mcp-copy-endpoint", tr("shell-mcp-copy-endpoint"))
                    .icon("copy")
                    .tooltip(endpoint(mcp.port)),
            )
            .separator()
            .item(
                MenuItem::new("mcp-preferences", tr("shell-preferences-ellipsis")).icon("settings"),
            );
        let theme = world.resource::<Theme>().clone();
        open_menu(&mut world.commands(), anchor, menu.build(&theme));
        world.flush();
    });
}

fn on_menu_action(ev: On<MenuAction>, q: Query<(), With<McpButton>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        mcp_menu_action(&mut commands, ev.entity, &ev.item);
    }
}

/// Carries out a pick from the AI assistants menu (`mcp-…`) opened under `anchor`.
pub(crate) fn mcp_menu_action(commands: &mut Commands, anchor: Entity, item: &str) {
    match item {
        "mcp-enabled" => commands.queue(|w: &mut World| {
            let on = !w.resource::<LocalPreferences>().0.mcp.enabled;
            crate::preferences_ui::set_mcp_enabled(w, on);
        }),
        "mcp-port" => commands.queue(move |w: &mut World| open_port_popup(w, anchor)),
        "mcp-port-reset" => commands.queue(|w: &mut World| {
            crate::preferences_ui::set_mcp_port(w, DEFAULT_MCP_PORT);
        }),
        "mcp-copy-endpoint" => commands.queue(|w: &mut World| {
            let url = endpoint(w.resource::<LocalPreferences>().0.mcp.port);
            info!("MCP endpoint: {url}");
            if let Some(mut clip) = w.get_resource_mut::<bevy::clipboard::Clipboard>()
                && let Err(e) = clip.set_text(url.clone())
            {
                warn!("clipboard: {e:?}");
            }
            crate::linked::toast(w, tr_with("shell-mcp-copied", &[("url", &url)]));
        }),
        "mcp-preferences" => commands.queue(crate::preferences_ui::open_preferences),
        _ => {}
    }
}

/// The popup that asks for the port.
#[derive(Component)]
struct PortPopup;

/// Opens the port popup under `anchor`'s right end.
fn open_port_popup(world: &mut World, anchor: Entity) {
    const WIDTH: f32 = 150.0;
    let at = world
        .get::<ComputedNode>(anchor)
        .zip(world.get::<bevy::ui::UiGlobalTransform>(anchor))
        .map(|(n, t)| {
            let s = n.inverse_scale_factor();
            let (center, size) = (t.translation * s, n.size() * s);
            Vec2::new(
                (center.x + size.x / 2.0 - WIDTH).max(4.0),
                center.y + size.y / 2.0 + 4.0,
            )
        })
        .unwrap_or(Vec2::new(120.0, 48.0));
    let port = world.resource::<LocalPreferences>().0.mcp.port;
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    let popup = cadrs_ui::NamePopup::new("mcp-port-popup", tr("shell-mcp-port-title"), at)
        .value(port.to_string())
        .spawn(&mut commands, &theme);
    commands.entity(popup).insert(PortPopup);
    world.flush();
}

/// A port as typed: 1 to 65535.
pub fn parse_port(text: &str) -> Option<u16> {
    text.trim().parse::<u16>().ok().filter(|&p| p > 0)
}

fn on_port_commit(ev: On<NamePopupCommit>, q: Query<(), With<PortPopup>>, mut commands: Commands) {
    if !q.contains(ev.entity) {
        return;
    }
    let text = ev.value.clone();
    commands.queue(move |w: &mut World| match parse_port(&text) {
        Some(port) => crate::preferences_ui::set_mcp_port(w, port),
        None => crate::linked::toast(w, tr_with("shell-mcp-port-invalid", &[("text", &text)])),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_parse_from_1_to_65535() {
        assert_eq!(parse_port(" 7690 "), Some(7690));
        assert_eq!(parse_port("65535"), Some(65535));
        assert_eq!(parse_port("0"), None);
        assert_eq!(parse_port("65536"), None);
        assert_eq!(parse_port("http"), None);
    }

    #[test]
    fn every_icon_exists() {
        for state in [
            McpState::Off,
            McpState::On(String::new()),
            McpState::Failed(String::new()),
        ] {
            assert!(cadrs_icons::get(state.icon()).is_some(), "{}", state.icon());
        }
    }
}
