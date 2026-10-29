//! Debugging aids, each off unless its environment variable is set (so they cost nothing
//! otherwise):
//!
//! - `CADRS_SLOW_FRAMES=<ms>`: prints every frame that took longer than this (wall clock, so
//!   scripted runs' fixed time steps don't hide it) with its slowest schedules, systems and
//!   systems' commands (observers included; those over 1 ms), from the frame profile ([`crate::frame_profile`]; it turns
//!   the system times on, which need the default `profiler` feature). `CADRS_SLOW_FRAMES=1` (or
//!   any value that isn't a number) uses 50 ms.
//! - `CADRS_FRAME_PROFILE=1`: times every system and schedule for the frame panel and
//!   `get_frame_stats`, as the *Time systems* preference does (`0` keeps them off;
//!   [`crate::frame_profile::CpuProfiling`]).
//! - `CADRS_NO_VSYNC=1`: presents frames as fast as they are made (no vsync), to see what a
//!   frame really costs, as *Preferences › Vertical sync* off does, for this run only
//!   (`crate::preferences_ui::apply_vsync`).
//! - `CADRS_UI_CHANGES=1`: about once a second, how many UI nodes changed what Bevy lays them
//!   out from (their `Node`, measured content or render target) in the last frame, and which
//!   (by name, or their nearest named ancestor's). Any such change makes Bevy lay the whole UI
//!   out again, so a node that changes every frame costs a full layout every frame. Built with
//!   Bevy's `track_location` feature, it also says which line of code wrote the changed images.
//!   Also the UI colours changed (each is drawn again), every node by its nearest named
//!   ancestor, the archetypes the nodes are spread over (a query pays for each), what the UI
//!   draws (items, glyphs, GPU textures) and the cameras rendering.
//! - `CADRS_WRITE_TEST=1`: about every two seconds, how long a 700 KB `write_buffer` takes on
//!   average (the UI's vertices are about that, written every frame). Milliseconds, not hundredths,
//!   mean uploads are staged where the CPU writes slowly (`vendor/gpu-allocator/CADRS.md`).
//! - `CADRS_SINGLE_THREADED=Update,PostUpdate,…`: runs those schedules (`First`, `PreUpdate`,
//!   `Update`, `PostUpdate`, `Last`) on the main thread alone, so a profiler's samples of that
//!   thread (`perf record -t <pid>`) hold all their systems, each under its own name.

use web_time::Instant;

use bevy::prelude::*;

use crate::frame_profile::{FrameProfile, Kind};

pub struct DebugToolsPlugin;

impl Plugin for DebugToolsPlugin {
    fn build(&self, app: &mut App) {
        if let Some(ms) = slow_frame_threshold() {
            app.insert_resource(SlowFrames { ms }).add_systems(
                First,
                report_slow_frames.after(crate::frame_profile::take_frame),
            );
        }
        if let Ok(v) = std::env::var("CADRS_SINGLE_THREADED") {
            use bevy::ecs::schedule::SingleThreadedExecutor;
            for name in v.split(',') {
                let st = |s: &mut Schedule| {
                    s.set_executor(SingleThreadedExecutor::new());
                };
                match name {
                    "First" => app.edit_schedule(First, st),
                    "PreUpdate" => app.edit_schedule(PreUpdate, st),
                    "Update" => app.edit_schedule(Update, st),
                    "PostUpdate" => app.edit_schedule(PostUpdate, st),
                    "Last" => app.edit_schedule(Last, st),
                    _ => app,
                };
            }
        }
        if std::env::var_os("CADRS_WRITE_TEST").is_some()
            && let Some(render) = app.get_sub_app_mut(bevy::render::RenderApp)
        {
            render.add_systems(
                bevy::render::Render,
                write_test.in_set(bevy::render::RenderSystems::PrepareBindGroups),
            );
        }
        if std::env::var_os("CADRS_UI_CHANGES").is_some() {
            app.add_systems(
                PostUpdate,
                report_ui_changes.before(bevy::ui::UiSystems::Layout),
            );
            if let Some(render) = app.get_sub_app_mut(bevy::render::RenderApp) {
                render.add_systems(
                    bevy::render::Render,
                    report_extracted_ui.in_set(bevy::render::RenderSystems::PrepareBindGroups),
                );
            }
        }
    }
}

/// `CADRS_SLOW_FRAMES` in ms, if set.
fn slow_frame_threshold() -> Option<f32> {
    let v = std::env::var("CADRS_SLOW_FRAMES").ok()?;
    Some(match v.trim().parse::<f32>() {
        Ok(ms) if ms > 1.0 => ms,
        _ => 50.0,
    })
}

#[derive(Resource)]
struct SlowFrames {
    ms: f32,
}

fn report_slow_frames(
    slow: Res<SlowFrames>,
    profile: Res<FrameProfile>,
    mut last: Local<Option<Instant>>,
) {
    let now = Instant::now();
    let Some(ms) = last.replace(now).map(|t| (now - t).as_secs_f32() * 1000.0) else {
        return;
    };
    if ms < slow.ms {
        return;
    }
    let spans = |kind: Kind| {
        let mut v: Vec<_> = profile
            .last()
            .map(|f| {
                f.cpu
                    .iter()
                    .filter(|(k, _, t, _)| *k == kind && *t >= 1.0)
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by(|a, b| b.2.total_cmp(&a.2));
        v
    };
    let mut out = format!("slow frame: {ms:.0} ms");
    // The schedules show where time outside any system went (applying commands, waiting).
    for (label, kind, n) in [
        ("schedule", Kind::Schedule, 6),
        ("system", Kind::System, 12),
        ("commands of", Kind::Commands, 6),
    ] {
        for (_, name, t, runs) in spans(kind).iter().take(n) {
            out.push_str(&format!("\n  {t:8.1} ms  x{runs}  {label} {name}"));
        }
    }
    warn!("{out}");
}

/// `CADRS_UI_CHANGES`: the UI nodes whose layout inputs changed since the last frame, the colours
/// changed, and how the UI's nodes and archetypes are spread.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn report_ui_changes(
    changed: Query<
        (
            Entity,
            Ref<Node>,
            Option<Ref<bevy::ui::ContentSize>>,
            Option<Ref<bevy::ui::ComputedUiRenderTargetInfo>>,
        ),
        Or<(
            Changed<Node>,
            Changed<bevy::ui::ContentSize>,
            Changed<bevy::ui::ComputedUiRenderTargetInfo>,
        )>,
    >,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    images: Query<Ref<ImageNode>>,
    texts: Query<Ref<Text>>,
    all: Query<(Entity, &InheritedVisibility), With<Node>>,
    cameras: Query<(
        &Camera,
        Option<&Name>,
        Option<&Msaa>,
        Has<Camera3d>,
        Option<&bevy::core_pipeline::tonemapping::Tonemapping>,
        Option<&bevy::camera::Hdr>,
    )>,
    fills: Query<Ref<BackgroundColor>>,
    borders: Query<Ref<BorderColor>>,
    archetypes: &bevy::ecs::archetype::Archetypes,
    components: &bevy::ecs::component::Components,
    mut frames: Local<u32>,
) {
    *frames += 1;
    if *frames % 60 != 0 {
        return;
    }
    let (mut node, mut content, mut target, mut added) = (0, 0, 0, 0);
    let mut by_name = std::collections::BTreeMap::<String, u32>::new();
    for (e, n, c, t) in &changed {
        if n.is_added() {
            added += 1;
            continue;
        }
        node += u32::from(n.is_changed());
        content += u32::from(c.is_some_and(|c| c.is_changed()));
        target += u32::from(t.is_some_and(|t| t.is_changed()));
        let named = std::iter::successors(Some(e), |&e| parents.get(e).ok().map(ChildOf::parent))
            .find_map(|e| names.get(e).ok())
            .map_or("(unnamed)", |n| n.as_str());
        *by_name.entry(named.to_string()).or_default() += 1;
    }
    let (mut image_sized, mut text_sized) = (0, 0);
    for (e, n, c, _) in &changed {
        if !n.is_added() && c.is_some_and(|c| c.is_changed()) {
            image_sized += u32::from(images.contains(e));
            text_sized += u32::from(texts.contains(e));
        }
    }
    let images_changed = images
        .iter()
        .filter(|i| i.is_changed() && !i.is_added())
        .count();
    let texts_changed = texts
        .iter()
        .filter(|t| t.is_changed() && !t.is_added())
        .count();
    // Where the changed images were written, when Bevy tracks it (its `track_location` feature).
    let mut writers = std::collections::BTreeMap::<String, u32>::new();
    for i in images.iter().filter(|i| i.is_changed() && !i.is_added()) {
        if let Some(at) = i.changed_by().into_option() {
            *writers.entry(at.to_string()).or_default() += 1;
        }
    }
    // Every node, hidden ones apart, by its nearest named ancestor's name.
    let (mut nodes, mut hidden) = (0, 0);
    let mut owners = std::collections::BTreeMap::<String, (u32, u32)>::new();
    for (e, vis) in &all {
        nodes += 1;
        hidden += u32::from(!vis.get());
        let named = std::iter::successors(parents.get(e).ok().map(ChildOf::parent), |&e| {
            parents.get(e).ok().map(ChildOf::parent)
        })
        .find_map(|e| names.get(e).ok())
        .map_or("(unnamed)", |n| n.as_str());
        let o = owners.entry(named.to_string()).or_default();
        o.0 += 1;
        o.1 += u32::from(!vis.get());
    }
    // The characters of shown texts by owner: each is a glyph drawn (and uploaded) each frame.
    let mut chars = std::collections::BTreeMap::<String, usize>::new();
    for (e, vis) in &all {
        let Ok(t) = texts.get(e) else { continue };
        if !vis.get() {
            continue;
        }
        let named = std::iter::successors(Some(e), |&e| parents.get(e).ok().map(ChildOf::parent))
            .find_map(|e| names.get(e).ok())
            .map_or("(unnamed)", |n| n.as_str());
        *chars.entry(named.to_string()).or_default() += t.0.chars().count();
    }
    let mut chars: Vec<_> = chars.into_iter().collect();
    chars.sort_by_key(|c| std::cmp::Reverse(c.1));
    info!(
        "UI text shown: {} characters; most under: {:?}",
        chars.iter().map(|c| c.1).sum::<usize>(),
        &chars[..chars.len().min(25)]
    );
    let mut owners: Vec<_> = owners.into_iter().collect();
    owners.sort_by_key(|o| std::cmp::Reverse(o.1.0));
    info!(
        "UI nodes: {nodes} ({hidden} hidden); most under: {:?}",
        &owners[..owners.len().min(25)]
    );
    // Colours don't lay the UI out again, but each one changed is drawn again.
    let mut colours = std::collections::BTreeMap::<String, u32>::new();
    let mut count = |changed: bool, added: bool, at: Option<String>| {
        if changed && !added {
            *colours.entry(at.unwrap_or_default()).or_default() += 1;
        }
    };
    for c in &fills {
        count(
            c.is_changed(),
            c.is_added(),
            c.changed_by().into_option().map(|a| format!("fill {a}")),
        );
    }
    for c in &borders {
        count(
            c.is_changed(),
            c.is_added(),
            c.changed_by().into_option().map(|a| format!("border {a}")),
        );
    }
    info!("UI colours changed: {colours:?}");
    // The cameras that render: each runs its passes every frame.
    let active: Vec<String> = cameras
        .iter()
        .filter(|c| c.0.is_active)
        .map(|(c, name, msaa, three_d, tm, hdr)| {
            format!(
                "{} (order {}, {}{:?}{}{})",
                name.map_or("?", |n| n.as_str()),
                c.order,
                if three_d { "3d, " } else { "" },
                msaa.copied().unwrap_or_default(),
                tm.map(|t| format!(", {t:?}")).unwrap_or_default(),
                if hdr.is_some() { ", hdr" } else { "" },
            )
        })
        .collect();
    info!("Cameras rendering: {active:?}");
    // How the UI's entities are spread: each table a query visits costs it, however few rows.
    let node_id = components.component_id::<Node>();
    let ui_archetypes: Vec<_> = archetypes
        .iter()
        .filter(|a| node_id.is_some_and(|id| a.contains(id)) && !a.is_empty())
        .collect();
    let mut tables: Vec<_> = ui_archetypes
        .iter()
        .map(|a| a.table_id().as_usize())
        .collect();
    tables.sort();
    tables.dedup();
    // The components that split them most: in how many of those archetypes each one is.
    let mut by_component = std::collections::BTreeMap::<String, u32>::new();
    for a in &ui_archetypes {
        for c in a.components() {
            if let Some(info) = components.get_info(*c) {
                *by_component.entry(info.name().to_string()).or_default() += 1;
            }
        }
    }
    let mut by_component: Vec<_> = by_component.into_iter().collect();
    by_component.sort_by_key(|c| std::cmp::Reverse(c.1));
    let n = ui_archetypes.len() as u32;
    info!(
        "UI archetypes: {n} with nodes, in {} tables; components in some but not all: {:?}",
        tables.len(),
        by_component
            .iter()
            .filter(|c| c.1 < n)
            .take(40)
            .collect::<Vec<_>>()
    );
    let mut top: Vec<_> = by_name.into_iter().collect();
    top.sort_by_key(|t| std::cmp::Reverse(t.1));
    info!(
        "UI changes: {node} Node, {content} content size ({image_sized} images, {text_sized} \
         texts), {target} render target, {added} added; {images_changed} ImageNode and \
         {texts_changed} Text changed (written at {writers:?}); {} archetypes; by name: {:?}",
        archetypes.len(),
        &top[..top.len().min(15)]
    );
}

/// `CADRS_WRITE_TEST`: how long a 700 KB `write_buffer` takes (the UI's vertices are about that).
fn write_test(
    device: Res<bevy::render::renderer::RenderDevice>,
    queue: Res<bevy::render::renderer::RenderQueue>,
    mut buffer: Local<Option<bevy::render::render_resource::Buffer>>,
    mut frames: Local<u32>,
    mut total: Local<f64>,
) {
    use bevy::render::render_resource::{BufferDescriptor, BufferUsages};
    const SIZE: usize = 700 * 1024;
    let b = buffer.get_or_insert_with(|| {
        device.create_buffer(&BufferDescriptor {
            label: Some("write-test"),
            size: SIZE as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    });
    let data = vec![(*frames % 251) as u8; SIZE];
    let t = Instant::now();
    queue.write_buffer(b, 0, &data);
    *total += t.elapsed().as_secs_f64();
    *frames += 1;
    if *frames % 120 == 0 {
        info!(
            "write_buffer of 700 KB: {:.3} ms on average",
            *total / 120.0 * 1e3
        );
        *total = 0.0;
    }
}

/// `CADRS_UI_CHANGES`, in the render world: the UI items drawn (glyphs one each), by node.
fn report_extracted_ui(
    extracted: Res<bevy::ui_render::ExtractedUiNodes>,
    gpu_images: Res<bevy::render::render_asset::RenderAssets<bevy::render::texture::GpuImage>>,
    phases: Res<bevy::render::render_phase::ViewSortedRenderPhases<bevy::ui_render::TransparentUi>>,
    mut frames: Local<u32>,
) {
    *frames += 1;
    if *frames % 60 != 0 {
        return;
    }
    let mut sizes: Vec<(usize, Entity)> = extracted
        .uinodes
        .iter()
        .map(|(e, (_, items))| (items.len(), e.id()))
        .collect();
    sizes.sort_by_key(|s| std::cmp::Reverse(s.0));
    let views: Vec<usize> = phases.values().map(|p| p.items.len()).collect();
    let glyphs: usize = extracted
        .uinodes
        .values()
        .flat_map(|(_, items)| items.values())
        .map(|n| match &n.item {
            bevy::ui_render::ExtractedUiItem::Glyphs { glyphs } => glyphs.len(),
            _ => 0,
        })
        .sum();
    info!(
        "UI glyphs drawn: {glyphs}; {} images on the GPU (wgpu tracks each in every submission)",
        gpu_images.iter().count()
    );
    info!(
        "UI drawn: {views:?} items by view; {} extracted from {} nodes; most: {:?}",
        sizes.iter().map(|s| s.0).sum::<usize>(),
        sizes.len(),
        &sizes[..sizes.len().min(12)]
    );
}
