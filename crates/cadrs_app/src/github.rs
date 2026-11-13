//! cadrs on GitHub: the top bar's GitHub button beside the version, with the repository's star
//! count, opens the repository in the browser (the app menu's GitHub does too). The count is
//! asked of GitHub's API once, when the app starts, and only by a person's own runs
//! ([`FetchStars`], inserted by `cadrs`'s `main` and by the page): scenarios, headless runs and
//! tests never touch the network, and show the button without a count. `CADRS_NO_UPDATE_CHECK=1`
//! keeps this off GitHub too. On a narrow window the button folds into the account menu with the
//! version (`crate::top_bar`).

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use bevy::prelude::*;
use cadrs_ui::Button;
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::prelude::*;

const REPO_URL: &str = "https://api.github.com/repos/rvdende/cadrs";

pub struct GitHubPlugin;

impl Plugin for GitHubPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Stars>()
            .add_systems(Startup, start_fetch)
            .add_systems(Update, (poll_fetch, sync_button).chain());
    }
}

/// Inserted for runs that ask GitHub how many stars the repository has.
#[derive(Resource)]
pub struct FetchStars;

/// The repository's star count, once GitHub has answered.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stars(pub Option<u32>);

impl Stars {
    /// The button's tooltip and the folded menu item's: the count when known.
    pub fn tooltip(self) -> String {
        match self.0 {
            Some(n) => tr_with("shell-top-bar-github-stars", &[("stars", &n.to_string())]),
            None => tr("shell-top-bar-github"),
        }
    }
}

/// The answer on its way.
#[derive(Resource)]
struct Fetch(Mutex<Receiver<Result<u32, String>>>);

/// The top bar's slot for the button, filled by [`sync_button`] (again when the count arrives).
#[derive(Component)]
struct GitHubSlot;

/// Spawns the button's slot in a top bar, hidden with `fold`.
pub fn spawn_slot(r: &mut ChildSpawnerCommands, fold: cadrs_ui::HideBelow) {
    r.spawn((
        Name::new("github-slot"),
        GitHubSlot,
        Node {
            align_items: AlignItems::Center,
            margin: UiRect::right(Val::Px(4.0)),
            ..default()
        },
        fold,
    ));
}

fn start_fetch(mut commands: Commands, fetch: Option<Res<FetchStars>>) {
    if fetch.is_none() || std::env::var_os("CADRS_NO_UPDATE_CHECK").is_some_and(|v| v != "0") {
        return;
    }
    let (tx, rx) = channel();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let started = cadrs_core::background::spawn("github-stars", move || {
            let _ = tx.send(stars());
        });
        if let Err(e) = started {
            info!("cannot ask GitHub for the star count: {e}");
            return;
        }
    }
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(async move {
        let _ = tx.send(stars().await);
    });
    commands.insert_resource(Fetch(Mutex::new(rx)));
}

#[cfg(not(target_arch = "wasm32"))]
fn stars() -> Result<u32, String> {
    let mut r = crate::update::agent(20)
        .get(REPO_URL)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?;
    let body = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
    parse_stars(&body)
}

#[cfg(target_arch = "wasm32")]
async fn stars() -> Result<u32, String> {
    let body = reqwest::Client::new()
        .get(REPO_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    parse_stars(&body)
}

/// The star count in GitHub's repository JSON.
fn parse_stars(json: &str) -> Result<u32, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    v.get("stargazers_count")
        .and_then(|n| n.as_u64())
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| "no stargazers_count".to_string())
}

/// A count as GitHub shows it beside its star: 999, 1.2k, 12k.
fn short(n: u32) -> String {
    if n < 1_000 {
        n.to_string()
    } else if n < 10_000 {
        let tenths = n / 100;
        if tenths % 10 == 0 {
            format!("{}k", tenths / 10)
        } else {
            format!("{}.{}k", tenths / 10, tenths % 10)
        }
    } else {
        format!("{}k", n / 1_000)
    }
}

fn poll_fetch(mut commands: Commands, fetch: Option<Res<Fetch>>, mut stars: ResMut<Stars>) {
    let Some(fetch) = fetch else {
        return;
    };
    let Ok(rx) = fetch.0.lock() else {
        return;
    };
    match rx.try_recv() {
        Ok(Ok(n)) => stars.0 = Some(n),
        Ok(Err(e)) => info!("cannot ask GitHub for the star count: {e}"),
        Err(TryRecvError::Empty) => return,
        Err(TryRecvError::Disconnected) => {}
    }
    drop(rx);
    commands.remove_resource::<Fetch>();
}

/// Fills a top bar's [`GitHubSlot`] when it is spawned, and again when the count arrives.
fn sync_button(
    mut commands: Commands,
    stars: Res<Stars>,
    theme: Res<Theme>,
    slots: Query<(Entity, Option<&Children>), With<GitHubSlot>>,
) {
    for (slot, children) in &slots {
        if children.is_some_and(|c| !c.is_empty()) && !stars.is_changed() {
            continue;
        }
        commands.entity(slot).despawn_children();
        let mut button = Button::new("github-button")
            .icon("github")
            .ghost()
            .small()
            .tooltip(stars.tooltip());
        if let Some(n) = stars.0 {
            button = button.label(short(n));
        }
        commands.entity(slot).with_child((
            button.build(&theme),
            observe(|_: On<Activate>| crate::open_url(crate::GITHUB)),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_read_as_github_shows_them() {
        assert_eq!(short(0), "0");
        assert_eq!(short(999), "999");
        assert_eq!(short(1_000), "1k");
        assert_eq!(short(1_250), "1.2k");
        assert_eq!(short(9_950), "9.9k");
        assert_eq!(short(12_345), "12k");
    }

    #[test]
    fn the_star_count_is_read_from_the_repository_json() {
        assert_eq!(
            parse_stars(r#"{"full_name":"rvdende/cadrs","stargazers_count":42}"#),
            Ok(42)
        );
        assert!(parse_stars(r#"{"message":"Not Found"}"#).is_err());
        assert!(parse_stars("not json").is_err());
    }

    #[test]
    fn the_button_has_its_icon() {
        assert!(cadrs_icons::get("github").is_some());
    }
}
