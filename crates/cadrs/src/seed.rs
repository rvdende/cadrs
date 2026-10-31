//! `cadrs --public-seed [--data-dir <dir>] [--peer <cadrs ID>]…`: a seed peer for public
//! documents ([`cadrs_net::public`]), with no window. It stays online in the public room, keeps
//! every listing, star and published revision it hears of, and hands them to whoever asks, so
//! a newcomer always has someone to start from and a document stays reachable while its owner
//! is offline.
//!
//! Its identity and what it keeps live in its data dir: `--data-dir`, else `CADRS_DATA_DIR`,
//! else `seed` next to the app's documents (`~/.local/share/cadrs/seed`), never the app's own,
//! so a seed beside the app on one machine is a peer of its own. It is the same peer from run
//! to run: it prints its cadrs ID, which others add as a seed (Preferences ▸ Collaboration, or
//! [`cadrs_net::public::DEFAULT_SEEDS`]). `--peer` names other seeds to start from.

use std::path::PathBuf;
use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use cadrs_core::Store;
use cadrs_net::bevy_iroh::Iroh;
use cadrs_net::public::{self, KeepAll, PublicPeers, RoomState, Shared};

/// How often the seed says how it is doing.
const REPORT_EVERY: f64 = 300.0;

pub struct Seed {
    dir: PathBuf,
    peers: Vec<String>,
}

/// `Some` when the arguments ask for a seed.
pub fn parse(args: &[String]) -> Option<Result<Seed, String>> {
    if !args.iter().any(|a| a == "--public-seed") {
        return None;
    }
    let mut dir = std::env::var_os("CADRS_DATA_DIR").map(PathBuf::from);
    let mut peers = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--public-seed" => {}
            "--data-dir" => match it.next() {
                Some(d) => dir = Some(PathBuf::from(d)),
                None => return Some(Err("--data-dir needs a folder".into())),
            },
            "--peer" => match it.next() {
                Some(p) if p.parse::<cadrs_net::bevy_iroh::iroh::EndpointId>().is_ok() => {
                    peers.push(p.clone())
                }
                Some(p) => return Some(Err(format!("--peer {p}: not a cadrs ID"))),
                None => return Some(Err("--peer needs a cadrs ID".into())),
            },
            other => {
                return Some(Err(format!(
                    "{other}: a seed takes only --data-dir and --peer"
                )));
            }
        }
    }
    let dir = dir.or_else(|| {
        Store::default_root().map(|r| r.parent().map_or(r.clone(), |p| p.join("seed")))
    });
    Some(match dir {
        Some(dir) => Ok(Seed { dir, peers }),
        None => Err("No data dir: give one with --data-dir".into()),
    })
}

pub fn run(seed: Seed) -> AppExit {
    if let Err(e) = std::fs::create_dir_all(&seed.dir) {
        eprintln!("cannot use {}: {e}", seed.dir.display());
        return AppExit::from_code(2);
    }
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / 30.0,
        ))),
        bevy::log::LogPlugin::default(),
    ));
    let shared = Shared::default();
    shared.open(seed.dir.join("public"), public::unix_now());
    app.insert_resource(shared);
    app.add_plugins(cadrs_net::NetPlugin {
        identity: Some(seed.dir.join(cadrs_net::IDENTITY_FILE)),
        display_name: "cadrs seed".into(),
        relays: Default::default(),
        autostart: true,
    });
    app.insert_resource(KeepAll)
        .insert_resource(PublicPeers(PublicPeers::seeds(&seed.peers)))
        .add_systems(Update, report);
    match cadrs_net::cadrs_id(app.world().get_resource::<Iroh>()) {
        Some(id) => {
            println!("cadrs public seed, keeping {}", seed.dir.display());
            println!("cadrs ID: {id}");
        }
        None => {
            eprintln!(
                "cannot go online: {}",
                app.world()
                    .get_resource::<cadrs_net::bevy_iroh::StartError>()
                    .map_or("unknown error", |e| e.0.as_str())
            );
            return AppExit::from_code(1);
        }
    }
    app.run()
}

/// Says now and then what the seed keeps and who is in the room.
fn report(world: &mut World, mut last: Local<Option<f64>>) {
    let now = world
        .resource::<Time<bevy::time::Real>>()
        .elapsed_secs_f64();
    if last.is_some_and(|t| now - t < REPORT_EVERY) {
        return;
    }
    *last = Some(now);
    let peers = match public::room_state(world) {
        RoomState::Joined(n) => n.to_string(),
        RoomState::Joining => "joining".into(),
        RoomState::Offline | RoomState::Off => "offline".into(),
    };
    let listed = world.resource::<Shared>().lock().directory.len();
    info!("public seed: {listed} documents listed; peers in the room: {peers}");
}
