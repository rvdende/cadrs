//! Scenario set-up commands (`Custom("…")` steps, see `cadrs_ui::ScriptCommand`) for states
//! that would take hundreds of clicks to reach. Everything still goes through the command
//! layer.
//!
//! - `populate-sketch N`: fills the sketch being edited with a grid of N/5 cells, each a
//!   constrained rectangle (4 lines) with a circle inside (N curves in all), and gives every
//!   fifth rectangle a width dimension. Used by the `perf_500` scenario.
//! - `control-arm`: adds the course's Control Arm (PS6, [`cadrs_core::samples`]) to the active
//!   Part Studio: Sketch 1 on Top, Extrude 1 (hub ring, right web, right eye ring, 40 mm) and
//!   Extrude 2 (left web and eye ring, 25 mm), both New. Used by the P3.2 scenarios.
//!   `control-arm add`: the same with Extrude 2 Add, one part, as the course makes it (P3.5).
//! - `control-arm-sketch`: only the Control Arm's Sketch 1 on Top, fully defined (its
//!   dimensions and constraints as the course's drawing gives them), for `course_ps6_control_arm`
//!   to extrude through the dialog.
//! - `sketch <plane> <shape>…`: adds a sketch on `top`, `front` or `right` with the shapes
//!   `rect x0 y0 x1 y1` (a rectangle with its constraints), `tri x0 y0 x1 y1 x2 y2`,
//!   `chain x0 y0 x1 y1 …` (an open polyline), `poly x0 y0 x1 y1 …` (a closed one, P3.5),
//!   `arc cx cy sx sy ex ey` (counter-clockwise from s to e, P3I.2),
//!   `circle cx cy r`, `centreline x0 y0 x1 y1` (a construction line, P3.4) and `point x y`
//!   (P3.8), in the plane's
//!   coordinates
//!   (mm). For scenarios whose subject is what comes after the sketch (P3.3).
//! - `gear-cover`: opens the Jackhammer Gear Cover stand-in (P3.6, PS17,
//!   [`cadrs_core::samples::gear_cover`], the document `fixtures/gear_cover_standin.cadrs`) in the
//!   active document: named "Jackhammer (stand-in)", its Part Studio "Gear Cover", mm and kg, the
//!   six base features in their closed "Base Features" folder, Aluminum - 380 on the part.
//! - `drawing-bracket`: builds the drawing-view bracket (P3C.2, [`cadrs_core::samples::drawing_bracket`]:
//!   a plate with four holes and rounded corners and an upright with a hole) in the active Part
//!   Studio; its part is "Bracket".
//! - `ujoint-flange`: opens the Universal Joint Flange stand-in (P3C.3, D8,
//!   [`cadrs_core::samples::ujoint`], the document `fixtures/ujoint_flange_standin.cadrs`) in the
//!   active document, as the course's "Make a copy" gives it: named "Universal Joint Drawing
//!   (stand-in)", its Part Studio "Universal Joint", inches; the part is "Universal Joint Flange".
//! - `hand-brake`: opens the Hand Brake stand-in (P3C.6, D14, [`cadrs_core::samples::hand_brake`],
//!   the document `fixtures/hand_brake_standin.cadrs`) in the active document, as the course's
//!   "Make a copy" gives it: named "Hand Brake Update (stand-in)", mm, its Part Studio "Handle"
//!   (the Handle Plate and the Handle Grip before the exercise's edits), the finished "Hand
//!   Brake Drawing" tab (sheets Assembly, Handle and Grip), up to date, and (P3C.5) the
//!   Hydraulic Brake Unit assembly with its studios Master Cylinder, Enclosures and Hardware.
//! - `bar-drawing`: the P3C.8 bar and its drawing tab (Front, Top, Right at 1:1), shown.
//! - `ujoint-drawing`: `ujoint-flange` plus the finished Ex1 drawing tab (P3C.7,
//!   [`cadrs_core::samples::ujoint_drawing`]), shown: for the export and insert scenarios.
//! - `reflector`: opens the Rocket Guidance Reflector stand-in (P3.8, PS27,
//!   [`cadrs_core::samples::reflector`], the document `fixtures/reflector_standin.cadrs`) in the
//!   active document: named "Rocket Guidance System (stand-in)", its Part Studio "Reflector", mm
//!   and kg, the plate's four base features in their closed "Reflector Surface Features" folder,
//!   the Pattern Axis connector, the Feature Sketch, Aluminum - 1060 on the part.
//! - `bracket`: builds the P3.9 feature-list bracket ([`cadrs_core::samples::bracket`]: a plate,
//!   ten bosses in a "Bosses" folder and two fillets) in the active Part Studio.
//! - `view AZ EL [X Y Z SCALE]` (P3.8): turns the 3D view to azimuth `AZ` and elevation `EL`
//!   (degrees, as the view cube's camera: azimuth from the Front view toward the Right view),
//!   optionally centred on the point `X Y Z` at `SCALE` mm per pixel, as orbiting would; for
//!   picking edges only visible from one side (a pocket's corners).
//! - `fixture <name>`: opens the stand-in document `fixtures/<name>.cadrs` as a fresh copy in
//!   the scenario's document store (the course's "Make a copy", P3B.1), with an empty undo
//!   history: e.g. `fixture motor_mount_standin` (Ex1 of the assemblies course).
//! - `inspection` (P3D.1, P3D.2): the broken inspection stand-in
//!   ([`cadrs_core::samples::inspection`]): Sketch 1 / Extrude 1 (a ring), Fillet 1, and
//!   Sketch 2 / Extrude 2 (a tab) with Sketch 2 unsolvable (Equal 1) and open (a spur and a
//!   0.508 mm gap), so Extrude 2 is red.
//! - `pcb-board <folder>/<name>` (P3H.2): imports the IDF pair `fixtures/idf/<folder>/<name>.emn`
//!   / `.emp` into the active Part Studio as parts ([`cadrs_pcb::sample`]): "Board [<name>]",
//!   the place keep-outs and keep-ins, one part per component ("U1 QFP100_600MIL"), coloured by
//!   class (green board, components by package kind, keep areas dark translucent). The document
//!   and the Part Studio are named after the board, in mm. Until the PCB Studio tab (P3H.3) this
//!   is how the board geometry is shown.
//! - `landing-reload` (P3E.1): the documents page reads the document store again (after a
//!   `linked-fixture store …` set-up).
//! - `step-fixture <path>` (P3F.2): writes the two-bracket STEP assembly
//!   ([`cadrs_core::samples::bracket_pair`]: 2 parts, 3 instances) to `path` (relative to the
//!   working folder), for the import scenarios to pick.
//! - `design-intent` (P3F.4): the course's hydraulic cylinder body driven by `#piston_d` and
//!   `#clearance` ([`cadrs_core::samples::design_intent`]) in the active Part Studio.
//! - `simulation-beam` (P3F.5): the simulation's cantilever, 100 × 10 × 10 mm in Steel - A36
//!   ([`cadrs_core::samples::simulation`]), in the active Part Studio.
//! - `clear-dir <path>` (P3F.2 judge): empties a folder under `target/` (an export scenario's
//!   folder, so its files get their plain names).
//! - `tab-fixture [N]` (P3E.2): stores and opens a document of N light tabs (60 by default,
//!   [`crate::tab_folders::fixture`]).
//! - `scale-fixture [N]` (P3F.3): opens a document of N tabs (40 by default,
//!   [`cadrs_core::samples::scale::document`]): "Plates", a Part Studio of 250 features and 10
//!   parts, then small Part Studios and Assemblies.
//! - `rebuild-budget <ms|off>` (P3F.3): how long a frame waits for a rebuild (scripted runs
//!   wait until it's done; `30` is the interactive setting).
//! - `scale-report <label>` (P3F.3): writes the finished rebuilds since the last report to
//!   `perf-<label>-rebuilds.txt` in the output folder.
//! - `part-look <n> <#rrggbb[aa]|-> [material]` (P3F.6): gives the n-th part (1-based) of the
//!   active Part Studio an appearance (with an opacity `aa`; `-` keeps its own) and a library
//!   material, as Edit appearance and Assign material do: `part-look 1 - Aluminum - 6061`.
//! - `sim-hold <on|off>` (P3F.5 judge): a simulation solve pauses once meshed until `off`.
//! - `render-hold <N|off>` (P3F.6): a final render pauses after N samples until `off`, for a
//!   screenshot of its progress.
//! - `selection-readout`: shows the selection readout (off by default, see
//!   [`crate::selection_readout`]).
//! - `linked-block …` (P3G.1, [`cadrs_core::samples::linked_block`]): `linked-block` writes
//!   document A, "Block source" (the Part Studio "Block", 50 × 30 × 25), into the scenario's
//!   store with its history and version V1, in the library folder "Linked parts", and opens
//!   document B, "Block consumer", with an empty "Assembly 1". `linked-block edit` edits A's
//!   workspace (Extrude 1 25 → 40) and makes V2, as the course edits the source in its own
//!   document. `linked-block trash` moves A to the trash, `linked-block purge` deletes it for
//!   good. `linked-block versions` opens "Block versions" (the studio and an assembly in one
//!   document, for references to a version of the same document); `linked-block height 40`
//!   sets the open document's Block depth through the command layer. `linked-block cycle` writes
//!   "Cycle partner", whose assembly "Top" holds B's Assembly 1 at B's newest version (for the
//!   circular-insert refusal).
//!   P3G.2: `linked-block edit 55` edits to another depth (a third version, V3);
//!   `linked-block consumer 3` inserts three instances of A's newest version into the
//!   open document's Assembly 1 (one step); `linked-block chain` writes "Block sub" (its
//!   assembly "Sub" holds A@V1; version V1) and inserts Sub@V1 into the open document's
//!   Assembly 1 (A → B → C); `linked-block open-sub` opens Block sub; `linked-block drawing`
//!   adds "Drawing 1" with a Front view of the open document's first linked copy of A;
//!   `linked-block versions-instance` inserts the Block part (workspace) into Block versions'
//!   assembly and `linked-block versions-drawing` adds a drawing of the Block studio (workspace).
//!   `linked-block mate` fastens Block versions' V1 instance onto its workspace instance, and
//!   `linked-block part-number BLK-002` sets the Block part's part number in the workspace.

use bevy::prelude::*;
use cadrs_core::commands::EditSketch;
use cadrs_sketch::constraint::rectangle_constraints;
use cadrs_sketch::{Dimension, DimensionKind, Sketch, SketchOp, Vec2 as SVec2};
use cadrs_ui::ScriptCommand;

use crate::ActiveDocument;
use crate::sketch::SketchSession;

pub struct ScriptPlugin;

impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, run_script_commands);
    }
}

fn run_script_commands(mut msgs: MessageReader<ScriptCommand>, mut commands: Commands) {
    for m in msgs.read() {
        if let Some(rest) = m.0.strip_prefix("sketch ") {
            let spec = rest.to_string();
            commands.queue(move |world: &mut World| add_sketch(world, &spec));
            continue;
        }
        // P3E.1: the documents page reads the store again.
        if m.0.trim() == "landing-reload" {
            commands.queue(crate::landing::reload_library);
            continue;
        }
        if let Some(path) = m.0.strip_prefix("step-fixture ") {
            step_fixture(path.trim());
            continue;
        }
        // P3G.1: the linked-documents block (see [`linked_block`]).
        if let Some(rest) = m.0.strip_prefix("linked-block") {
            let arg = rest.trim().to_string();
            commands.queue(move |world: &mut World| linked_block(world, &arg));
            continue;
        }
        // P3I.7: flat pattern drawings (see [`crate::drawing::flat_views::script`]).
        if let Some(rest) = m.0.strip_prefix("flat-drawing ") {
            let arg = rest.trim().to_string();
            commands.queue(move |world: &mut World| crate::drawing::flat_views::script(world, &arg));
            continue;
        }
        // P3I.9: the sheet metal loft and form set-ups (see [`crate::sheetmetal_p3i9_ui::script`]).
        if let Some(rest) = m.0.strip_prefix("sm9 ") {
            let arg = rest.trim().to_string();
            commands.queue(move |world: &mut World| crate::sheetmetal_p3i9_ui::script(world, &arg));
            continue;
        }
        // P3G.4: the Derived feature's set-ups (see [`crate::derived_ui::script`]).
        if let Some(rest) = m.0.strip_prefix("derived ") {
            let arg = rest.trim().to_string();
            commands.queue(move |world: &mut World| crate::derived_ui::script(world, &arg));
            continue;
        }
        // P3G.3: Move to document's stand-in (see [`crate::move_document::script`]).
        if let Some(rest) = m.0.strip_prefix("move-doc") {
            let arg = rest.trim().to_string();
            commands.queue(move |world: &mut World| crate::move_document::script(world, &arg));
            continue;
        }
        // P3G.5: the exercises' stand-ins (see [`crate::linked_exercises`]).
        if let Some(rest) = m.0.strip_prefix("linked-fixture ") {
            let arg = rest.trim().to_string();
            commands.queue(move |world: &mut World| crate::linked_exercises::fixture_script(world, &arg));
            continue;
        }
        if let Some(rest) = m.0.strip_prefix("linked-ex ") {
            let arg = rest.trim().to_string();
            commands.queue(move |world: &mut World| crate::linked_exercises::script(world, &arg));
            continue;
        }
        if let Some(name) = m.0.strip_prefix("fixture ") {
            let name = name.trim().to_string();
            commands.queue(move |world: &mut World| open_fixture(world, &name));
            continue;
        }
        if let Some(rest) = m.0.strip_prefix("pcb-board ") {
            let spec = rest.trim().to_string();
            commands.queue(move |world: &mut World| pcb_board(world, &spec));
            continue;
        }
        // P3F.2 judge: scenarios that export start from an empty folder, so names don't get
        // " (2)" from an earlier run. Only folders under `target/`.
        if let Some(dir) = m.0.strip_prefix("clear-dir ") {
            let dir = std::path::PathBuf::from(dir.trim());
            if dir.starts_with("target") && !dir.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                let _ = std::fs::remove_dir_all(&dir);
                let _ = std::fs::create_dir_all(&dir);
            } else {
                warn!("clear-dir: only folders under target/ ({})", dir.display());
            }
            continue;
        }
        // P3F.3: the scale fixtures and measurements (see `crate::scale_ui`).
        if let Some(label) = m.0.strip_prefix("scale-report ") {
            let label = label.trim().to_string();
            commands.queue(move |world: &mut World| crate::scale_ui::write_report(world, &label));
            continue;
        }
        // P3F.6: a part's appearance and material, through their commands.
        if let Some(rest) = m.0.strip_prefix("part-look ") {
            let rest = rest.trim().to_string();
            commands.queue(move |world: &mut World| part_look(world, &rest));
            continue;
        }
        // P3F.5 judge: pause a solve once meshed (`off` lets it go on).
        if let Some(v) = m.0.strip_prefix("sim-hold ") {
            crate::simulation_ui::SIM_HOLD.store(v.trim() == "on", std::sync::atomic::Ordering::Relaxed);
            continue;
        }
        // P3F.6: pause a final render after N samples (`off` lets it finish).
        if let Some(n) = m.0.strip_prefix("render-hold ") {
            let n = n.trim().parse::<u32>().ok();
            commands.queue(move |world: &mut World| world.resource::<crate::render_ui::RenderState>().set_hold(n));
            continue;
        }
        if let Some(ms) = m.0.strip_prefix("rebuild-budget ") {
            let budget = ms.trim().parse::<u64>().ok().map(std::time::Duration::from_millis);
            commands.insert_resource(crate::parts::RebuildBudget(budget));
            continue;
        }
        // P3E.2: a document of many light tabs (see [`crate::tab_folders::fixture`]).
        if let Some(rest) = m.0.strip_prefix("tab-fixture") {
            let arg = rest.trim().to_string();
            commands.queue(move |world: &mut World| crate::tab_folders::fixture(world, &arg));
            continue;
        }
        if let Some(n) = m.0.strip_prefix("scale-fixture") {
            let tabs = n.trim().parse::<usize>().unwrap_or(40);
            commands.queue(move |world: &mut World| {
                let doc = cadrs_core::samples::scale::document(tabs);
                world.insert_resource(crate::ActiveDocument::new(doc));
            });
            continue;
        }
        if let Some(rest) = m.0.strip_prefix("view ") {
            let v: Vec<f32> = rest.split_whitespace().filter_map(|w| w.parse().ok()).collect();
            commands.queue(move |world: &mut World| {
                let mut view = world.resource_mut::<crate::viewport::ViewportView>();
                let mut to = view.target();
                if let [az, el, ..] = v[..] {
                    to.azimuth = az;
                    to.elevation = el.clamp(-90.0, 90.0);
                    to.roll = 0.0;
                }
                if let [_, _, x, y, z, scale] = v[..] {
                    to.focus = Vec3::new(x, y, z);
                    to.scale = scale;
                }
                view.animate_to(to);
            });
            continue;
        }
        // `rename-part <old name> = <new name>`: renames a part of the active Part Studio.
        if let Some(rest) = m.0.trim().strip_prefix("rename-part ") {
            let (old, new) = rest.split_once('=').map_or((rest.trim(), ""), |(a, b)| (a.trim(), b.trim()));
            let (old, new) = (old.to_string(), new.to_string());
            commands.queue(move |world: &mut World| {
                let part = world.resource::<crate::parts::PartCache>().parts.iter().find(|p| p.name == old).map(|p| p.id);
                let Some(mut doc) = world.get_resource_mut::<crate::ActiveDocument>() else { return };
                let (Some(part), Some(element)) = (part, doc.active) else {
                    warn!("rename-part: no part {old:?}");
                    return;
                };
                if let Err(e) = doc.execute(&cadrs_core::commands::RenamePart { element, part, name: new }) {
                    warn!("rename-part: {e}");
                }
            });
            continue;
        }
        let mut parts = m.0.split_whitespace();
        match (parts.next(), parts.next().and_then(|n| n.parse::<usize>().ok())) {
            (Some("populate-sketch"), Some(n)) => {
                commands.queue(move |world: &mut World| populate_sketch(world, n));
            }
            (Some("control-arm"), None) if m.0.trim() == "control-arm add" => {
                commands.queue(|world: &mut World| control_arm_with(world, true));
            }
            (Some("control-arm"), None) => {
                commands.queue(|world: &mut World| control_arm_with(world, false));
            }
            (Some("control-arm-sketch"), None) => {
                commands.queue(control_arm_sketch);
            }
            (Some("gear-cover"), None) => {
                commands.queue(gear_cover);
            }
            (Some("drawing-bracket"), None) => {
                commands.queue(drawing_bracket);
            }
            (Some("ujoint-flange"), None) => {
                commands.queue(ujoint_flange);
            }
            (Some("hand-brake"), None) => {
                commands.queue(hand_brake);
            }
            (Some("ujoint-drawing"), None) => {
                commands.queue(ujoint_drawing);
            }
            (Some("bar-drawing"), None) => {
                commands.queue(bar_drawing);
            }
            (Some("reflector"), None) => {
                commands.queue(reflector);
            }
            (Some("bracket"), None) => {
                commands.queue(bracket);
            }
            (Some("blocks"), Some(n)) => {
                commands.queue(move |world: &mut World| blocks(world, n));
            }
            (Some("inspection"), None) => {
                commands.queue(inspection);
            }
            (Some("design-intent"), None) => {
                commands.queue(design_intent);
            }
            // P3F.5: the simulation's beam (`cadrs_core::samples::simulation`).
            (Some("simulation-beam"), None) => {
                commands.queue(simulation_beam);
            }
            (Some("selection-readout"), None) => {
                commands.insert_resource(crate::selection_readout::SelectionReadoutEnabled(true));
            }
            // P3H.3: `pcb-studio`, `pcb-import …` and `pcb-choose …` are `crate::pcb`'s.
            _ if crate::pcb::is_script_command(&m.0) => {}
            _ => warn!("unknown script command {:?}", m.0),
        }
    }
}

/// Cell size of the populated grid (mm).
const CELL: f64 = 20.0;

/// The edits that add `n` curves in `n / 5` cells: rectangles with their constraints and
/// circles.
pub fn populate_ops(n: usize) -> (SketchOp, Vec<[SVec2; 4]>) {
    let cells = n.div_ceil(5);
    let cols = (cells as f64).sqrt().ceil() as usize;
    let mut ops = Vec::new();
    let mut rects = Vec::new();
    for i in 0..cells {
        let (cx, cy) = ((i % cols) as f64, (i / cols) as f64);
        let o = SVec2::new(cx * CELL, cy * CELL);
        let corners = [
            o,
            o + SVec2::new(14.0, 0.0),
            o + SVec2::new(14.0, 10.0),
            o + SVec2::new(0.0, 10.0),
        ];
        ops.push(SketchOp::AddPolyline {
            points: corners.to_vec(),
            closed: true,
            construction: false,
            label: "Add rectangle",
        });
        ops.push(SketchOp::AddConstraints(rectangle_constraints(corners)));
        ops.push(SketchOp::AddCircle {
            center: o + SVec2::new(7.0, 5.0),
            radius: 2.5,
            construction: false,
        });
        rects.push(corners);
    }
    (SketchOp::Batch(ops), rects)
}

/// Width dimensions for every fifth rectangle.
pub fn dimension_ops(s: &Sketch, rects: &[[SVec2; 4]]) -> SketchOp {
    let mut ops = Vec::new();
    for r in rects.iter().step_by(5) {
        let (Some(a), Some(b)) = (s.point_at(r[0], 1e-6), s.point_at(r[1], 1e-6)) else {
            continue;
        };
        ops.push(SketchOp::SetDimension {
            dimension: Dimension {
                kind: DimensionKind::Horizontal { a, b },
                value: 14.0,
                offset: -3.0,
                along: 0.0,
                driven: false,
            },
            moves: vec![],
            radii: vec![],
        });
    }
    SketchOp::Batch(ops)
}

/// Adds a sketch from a `sketch` set-up command's spec (see the module docs).
fn add_sketch(world: &mut World, spec: &str) {
    use cadrs_core::FeatureId;
    use cadrs_core::commands::AddSketch;
    let mut words = spec.split_whitespace();
    let plane = match words.next() {
        Some("top") => cadrs_sketch::PlaneRef::Top,
        Some("front") => cadrs_sketch::PlaneRef::Front,
        Some("right") => cadrs_sketch::PlaneRef::Right,
        other => {
            warn!("sketch: unknown plane {other:?}");
            return;
        }
    };
    let rest: Vec<&str> = words.collect();
    let mut ops = Vec::new();
    let mut i = 0;
    let num = |w: Option<&&str>| w.and_then(|w| w.parse::<f64>().ok());
    while i < rest.len() {
        match rest[i] {
            "rect" => {
                let (Some(x0), Some(y0), Some(x1), Some(y1)) =
                    (num(rest.get(i + 1)), num(rest.get(i + 2)), num(rest.get(i + 3)), num(rest.get(i + 4)))
                else {
                    warn!("sketch: bad rect in {spec:?}");
                    return;
                };
                let corners = [SVec2::new(x0, y0), SVec2::new(x1, y0), SVec2::new(x1, y1), SVec2::new(x0, y1)];
                ops.push(SketchOp::AddPolyline {
                    points: corners.to_vec(),
                    closed: true,
                    construction: false,
                    label: "Add rectangle",
                });
                ops.push(SketchOp::AddConstraints(rectangle_constraints(corners)));
                i += 5;
            }
            "chain" => {
                // An open polyline: every number after it, as x y pairs.
                let mut v = Vec::new();
                let mut k = i + 1;
                while let Some(x) = num(rest.get(k)) {
                    v.push(x);
                    k += 1;
                }
                if v.len() < 4 || v.len() % 2 != 0 {
                    warn!("sketch: bad chain in {spec:?}");
                    return;
                }
                ops.push(SketchOp::AddPolyline {
                    points: v.chunks(2).map(|p| SVec2::new(p[0], p[1])).collect(),
                    closed: false,
                    construction: false,
                    label: "Add polyline",
                });
                i = k;
            }
            "centreline" => {
                // A construction line (a revolve axis): x0 y0 x1 y1.
                let v: Vec<f64> = (1..=4).filter_map(|k| num(rest.get(i + k))).collect();
                if v.len() != 4 {
                    warn!("sketch: bad centreline in {spec:?}");
                    return;
                }
                ops.push(SketchOp::AddPolyline {
                    points: vec![SVec2::new(v[0], v[1]), SVec2::new(v[2], v[3])],
                    closed: false,
                    construction: true,
                    label: "Add line",
                });
                i += 5;
            }
            "poly" => {
                // A closed polygon (P3.5): x0 y0 x1 y1 … up to the next shape name.
                let mut k = i + 1;
                let mut pts = Vec::new();
                while let (Some(x), Some(y)) = (num(rest.get(k)), num(rest.get(k + 1))) {
                    pts.push(SVec2::new(x, y));
                    k += 2;
                }
                if pts.len() < 3 {
                    warn!("sketch: bad poly in {spec:?}");
                    return;
                }
                ops.push(SketchOp::AddPolyline {
                    points: pts,
                    closed: true,
                    construction: false,
                    label: "Add polygon",
                });
                i = k;
            }
            "tri" => {
                let v: Vec<f64> = (1..=6).filter_map(|k| num(rest.get(i + k))).collect();
                if v.len() != 6 {
                    warn!("sketch: bad tri in {spec:?}");
                    return;
                }
                ops.push(SketchOp::AddPolyline {
                    points: vec![SVec2::new(v[0], v[1]), SVec2::new(v[2], v[3]), SVec2::new(v[4], v[5])],
                    closed: true,
                    construction: false,
                    label: "Add triangle",
                });
                i += 7;
            }
            "arc" => {
                // P3I.2: an arc about cx cy, counter-clockwise from sx sy to ex ey (its ends join
                // the curves already there).
                let v: Vec<f64> = (1..=6).filter_map(|k| num(rest.get(i + k))).collect();
                if v.len() != 6 {
                    warn!("sketch: bad arc in {spec:?}");
                    return;
                }
                ops.push(SketchOp::AddArc {
                    center: SVec2::new(v[0], v[1]),
                    start: SVec2::new(v[2], v[3]),
                    end: SVec2::new(v[4], v[5]),
                    construction: false,
                });
                i += 7;
            }
            "circle" => {
                let (Some(cx), Some(cy), Some(r)) = (num(rest.get(i + 1)), num(rest.get(i + 2)), num(rest.get(i + 3)))
                else {
                    warn!("sketch: bad circle in {spec:?}");
                    return;
                };
                ops.push(SketchOp::AddCircle {
                    center: SVec2::new(cx, cy),
                    radius: r,
                    construction: false,
                });
                i += 4;
            }
            "point" => {
                // P3.8: a sketch point (a hole's place): x y.
                let (Some(x), Some(y)) = (num(rest.get(i + 1)), num(rest.get(i + 2))) else {
                    warn!("sketch: bad point in {spec:?}");
                    return;
                };
                ops.push(SketchOp::AddPoint { pos: SVec2::new(x, y) });
                i += 3;
            }
            other => {
                warn!("sketch: unknown shape {other:?}");
                return;
            }
        }
    }
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc.active_element().map(|e| e.id) else {
        return;
    };
    let feature = FeatureId::new();
    if let Err(e) = doc.execute(&AddSketch { element, feature, plane: Some(plane) }) {
        warn!("sketch: {e}");
        return;
    }
    if let Err(e) = doc.execute(&EditSketch { element, feature, op: SketchOp::Batch(ops) }) {
        warn!("sketch: {e}");
    }
}

impl cadrs_core::samples::gear_cover::Studio for ActiveDocument {
    fn run(&mut self, c: &dyn cadrs_core::Command) -> Result<(), cadrs_core::CommandError> {
        self.execute(c)
    }
    fn document(&self) -> &cadrs_core::Document {
        &self.doc
    }
}

/// Opens the Gear Cover stand-in in the active document (the course's "Make a copy", PS17.1).
fn gear_cover(world: &mut World) {
    use cadrs_core::commands::{RenameDocument, RenameElement, SetUnits};
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc.active_element().map(|e| e.id) else {
        return;
    };
    let units = cadrs_sketch::units::Units {
        length: cadrs_sketch::units::LengthUnit::Millimeter,
        mass: cadrs_sketch::units::MassUnit::Kilogram,
        ..doc.doc.units
    };
    let steps: [&dyn cadrs_core::Command; 3] = [
        &RenameDocument { name: "Jackhammer (stand-in)".into() },
        &RenameElement { id: element, name: "Gear Cover".into() },
        &SetUnits { units },
    ];
    for c in steps {
        if let Err(e) = doc.execute(c) {
            warn!("gear-cover: {e}");
        }
    }
    if let Err(e) = cadrs_core::samples::gear_cover::build_in(&mut *doc, element) {
        warn!("gear-cover: {e}");
    }
}

/// Opens the Universal Joint Flange stand-in in the active document (the course's "Make a
/// copy", D8.1).
/// The stand-in documents of the drawings course open in the scenario's new document, which
/// has Onshape's default "Assembly 1" tab (`Document::new`); the course's tab bars don't show one
/// (`ex3-step8.png`), so an empty default assembly is deleted (through the command layer).
fn drop_default_assembly(doc: &mut ActiveDocument, what: &str) {
    let empty: Vec<cadrs_core::ElementId> = doc
        .doc
        .elements
        .iter()
        .filter(|e| e.name == "Assembly 1" && e.assembly_model().is_some_and(|a| a.is_empty()))
        .map(|e| e.id)
        .collect();
    for id in empty {
        if let Err(e) = doc.execute(&cadrs_core::commands::DeleteElement { id }) {
            warn!("{what}: {e}");
        }
    }
}

fn ujoint_flange(world: &mut World) {
    use cadrs_core::commands::{RenameDocument, RenameElement, SetUnits};
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc.active_element().map(|e| e.id) else {
        return;
    };
    let units = cadrs_sketch::units::Units {
        length: cadrs_sketch::units::LengthUnit::Inch,
        ..doc.doc.units
    };
    let steps: [&dyn cadrs_core::Command; 3] = [
        &RenameDocument { name: "Universal Joint Drawing (stand-in)".into() },
        &RenameElement { id: element, name: "Universal Joint".into() },
        &SetUnits { units },
    ];
    for c in steps {
        if let Err(e) = doc.execute(c) {
            warn!("ujoint-flange: {e}");
        }
    }
    drop_default_assembly(&mut doc, "ujoint-flange");
    if let Err(e) = cadrs_core::samples::ujoint::build_in(&mut *doc, element) {
        warn!("ujoint-flange: {e}");
    }
}

/// The Universal Joint Flange stand-in with its finished Ex1 drawing tab (P3C.7, for the export
/// and insert scenarios), the drawing shown.
fn ujoint_drawing(world: &mut World) {
    use cadrs_core::commands::InsertElement;
    ujoint_flange(world);
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(studio) = doc.active_element().map(|e| e.id) else {
        return;
    };
    match cadrs_core::samples::ujoint_drawing::drawing(&doc.doc, studio) {
        Ok(d) => {
            let el = cadrs_core::Element::drawing(cadrs_core::samples::ujoint_drawing::DRAWING_NAME, d);
            let id = el.id;
            let after = Some(studio);
            if let Err(e) = doc.execute(&InsertElement { element: el, after, label: "Create Drawing".into() }) {
                warn!("ujoint-drawing: {e}");
            }
            doc.set_active(id);
        }
        Err(e) => warn!("ujoint-drawing: {e}"),
    }
}

/// The P3C.8 bar (200 × 20 × 10 with a Ø6 hole, an M6 tapped hole, a chamfered and a rounded
/// end) in the active Part Studio, with its drawing tab shown.
fn bar_drawing(world: &mut World) {
    use cadrs_core::commands::{InsertElement, RenameDocument, RenameElement, SetUnits};
    use cadrs_core::samples::drawing_bar as bar;
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc.active_element().map(|e| e.id) else {
        return;
    };
    let units = cadrs_sketch::units::Units { length: cadrs_sketch::units::LengthUnit::Millimeter, ..doc.doc.units };
    let steps: [&dyn cadrs_core::Command; 3] = [
        &RenameDocument { name: "Bar (P3C.8)".into() },
        &RenameElement { id: element, name: bar::STUDIO_NAME.into() },
        &SetUnits { units },
    ];
    for c in steps {
        if let Err(e) = doc.execute(c) {
            warn!("bar-drawing: {e}");
        }
    }
    drop_default_assembly(&mut doc, "bar-drawing");
    if let Err(e) = bar::build_in(&mut *doc, element) {
        warn!("bar-drawing: {e}");
        return;
    }
    match bar::views::drawing(&doc.doc, element) {
        Ok(d) => {
            let el = cadrs_core::Element::drawing(bar::DRAWING_NAME, d);
            let id = el.id;
            if let Err(e) = doc.execute(&InsertElement { element: el, after: Some(element), label: "Create Drawing".into() }) {
                warn!("bar-drawing: {e}");
            }
            doc.set_active(id);
        }
        Err(e) => warn!("bar-drawing: {e}"),
    }
}

/// Opens the Hand Brake stand-in in the active document (the course's "Make a copy", D14.1).
fn hand_brake(world: &mut World) {
    use cadrs_core::commands::{InsertElement, RenameDocument, RenameElement, SetUnits};
    use cadrs_core::samples::hand_brake as hb;
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc.active_element().map(|e| e.id) else {
        return;
    };
    let units = cadrs_sketch::units::Units {
        length: cadrs_sketch::units::LengthUnit::Millimeter,
        ..doc.doc.units
    };
    let steps: [&dyn cadrs_core::Command; 3] = [
        &RenameDocument { name: hb::DOCUMENT_NAME.into() },
        &RenameElement { id: element, name: hb::STUDIO_NAME.into() },
        &SetUnits { units },
    ];
    for c in steps {
        if let Err(e) = doc.execute(c) {
            warn!("hand-brake: {e}");
        }
    }
    drop_default_assembly(&mut doc, "hand-brake");
    if let Err(e) = hb::build_in(&mut *doc, element) {
        warn!("hand-brake: {e}");
        return;
    }
    // P3C.5: the Hydraulic Brake Unit and its studios, after the Handle.
    if let Err(e) = hb::assembly::build_in(&mut *doc, element, element) {
        warn!("hand-brake: {e}");
        return;
    }
    match hb::drawing::drawing(&doc.doc, element, Some(hb::assembly::ASSEMBLY)) {
        Ok(d) => {
            let el = cadrs_core::Element::drawing(hb::DRAWING_NAME, d);
            let after = Some(element);
            if let Err(e) = doc.execute(&InsertElement { element: el, after, label: "Create Drawing".into() }) {
                warn!("hand-brake: {e}");
            }
            doc.set_active(element);
        }
        Err(e) => warn!("hand-brake: {e}"),
    }
}

fn drawing_bracket(world: &mut World) {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc.active_element().map(|e| e.id) else {
        return;
    };
    drop_default_assembly(&mut doc, "drawing-bracket");
    if let Err(e) = cadrs_core::samples::drawing_bracket::build_in(&mut *doc, element) {
        warn!("drawing-bracket: {e}");
    }
}

fn reflector(world: &mut World) {
    use cadrs_core::commands::{RenameDocument, RenameElement, SetUnits};
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc.active_element().map(|e| e.id) else {
        return;
    };
    let units = cadrs_sketch::units::Units {
        length: cadrs_sketch::units::LengthUnit::Millimeter,
        mass: cadrs_sketch::units::MassUnit::Kilogram,
        ..doc.doc.units
    };
    let steps: [&dyn cadrs_core::Command; 3] = [
        &RenameDocument { name: "Rocket Guidance System (stand-in)".into() },
        &RenameElement { id: element, name: "Reflector".into() },
        &SetUnits { units },
    ];
    for c in steps {
        if let Err(e) = doc.execute(c) {
            warn!("reflector: {e}");
        }
    }
    if let Err(e) = cadrs_core::samples::reflector::build_in(&mut *doc, element) {
        warn!("reflector: {e}");
    }
}

/// The P3.9 bracket (`cadrs_core::samples::bracket`): a plate, ten bosses in a "Bosses"
/// folder and two fillets, for the feature-list scenarios.
fn bracket(world: &mut World) {
    use cadrs_core::commands::{RenameDocument, RenameElement};
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc.active_element().map(|e| e.id) else {
        return;
    };
    let steps: [&dyn cadrs_core::Command; 2] = [
        &RenameDocument { name: "Bracket (stand-in)".into() },
        &RenameElement { id: element, name: "Bracket".into() },
    ];
    for c in steps {
        if let Err(e) = doc.execute(c) {
            warn!("bracket: {e}");
        }
    }
    if let Err(e) = cadrs_core::samples::bracket::build_in(&mut *doc, element) {
        warn!("bracket: {e}");
    }
}

/// `n` separate blocks, each a sketch and a New extrude
/// ([`cadrs_core::samples::bracket::blocks_in`]): a long feature list and Parts list, for scrolling.
fn blocks(world: &mut World, n: usize) {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc.active_element().map(|e| e.id) else {
        return;
    };
    if let Err(e) = cadrs_core::samples::bracket::blocks_in(&mut *doc, element, n) {
        warn!("blocks: {e}");
    }
}

/// P3D.1 / P3D.2: the broken inspection stand-in ([`cadrs_core::samples::inspection`]): a
/// ring with a fillet, and a tab whose Sketch 2 can't be solved and doesn't close.
fn inspection(world: &mut World) {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc.active_element().map(|e| e.id) else {
        return;
    };
    if let Err(e) = cadrs_core::samples::inspection::build_in(&mut *doc, element) {
        warn!("inspection: {e}");
    }
}

/// P3F.4: the course's design-intent model ([`cadrs_core::samples::design_intent`]) in the
/// active Part Studio.
fn design_intent(world: &mut World) {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc.active_element().map(|e| e.id) else {
        return;
    };
    if let Err(e) = cadrs_core::samples::design_intent::build_in(&mut *doc, element) {
        warn!("design-intent: {e}");
    }
}

/// P3F.5: the simulation's beam, 100 × 10 × 10 mm in Steel - A36
/// ([`cadrs_core::samples::simulation::beam_in`]), in the active Part Studio.
fn simulation_beam(world: &mut World) {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc.active_element().map(|e| e.id) else {
        return;
    };
    if let Err(e) = cadrs_core::samples::simulation::beam_in(&mut *doc, element) {
        warn!("simulation-beam: {e}");
    }
}

/// Adds the Control Arm's Sketch 1 on Top through the command layer, fully defined.
fn control_arm_sketch(world: &mut World) {
    use cadrs_core::FeatureId;
    use cadrs_core::commands::AddSketch;
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc.active_element().map(|e| e.id) else {
        return;
    };
    let sketch = FeatureId::new();
    let run = |doc: &mut ActiveDocument, c: &dyn cadrs_core::Command| {
        if let Err(e) = doc.execute(c) {
            warn!("control-arm-sketch: {e}");
        }
    };
    run(&mut doc, &AddSketch { element, feature: sketch, plane: Some(cadrs_sketch::PlaneRef::Top) });
    run(&mut doc, &EditSketch { element, feature: sketch, op: cadrs_core::samples::control_arm_geometry() });
    let g = doc
        .active_element()
        .and_then(|e| e.feature(sketch))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .unwrap_or_default();
    run(&mut doc, &EditSketch { element, feature: sketch, op: cadrs_core::samples::control_arm_constraints(&g) });
}

/// Adds the Control Arm's sketch and two extrudes through the command layer; with `add`, the
/// second extrude is Add, as the course makes it (one part, P3.5).
fn control_arm_with(world: &mut World, add: bool) {
    use cadrs_core::FeatureId;
    use cadrs_core::commands::{AddExtrude, AddSketch, SetExtrude};
    use cadrs_core::samples;
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc.active_element().map(|e| e.id) else {
        return;
    };
    let sketch = FeatureId::new();
    let run = |doc: &mut ActiveDocument, c: &dyn cadrs_core::Command| {
        if let Err(e) = doc.execute(c) {
            warn!("control-arm: {e}");
        }
    };
    run(&mut doc, &AddSketch { element, feature: sketch, plane: Some(cadrs_sketch::PlaneRef::Top) });
    run(&mut doc, &EditSketch { element, feature: sketch, op: samples::control_arm_geometry() });
    let geometry = |doc: &ActiveDocument| {
        doc.active_element()
            .and_then(|e| e.feature(sketch))
            .and_then(|f| f.sketch())
            .map(|s| s.geometry.clone())
            .unwrap_or_default()
    };
    if let Some(op) = samples::eye_hole_dimension(&geometry(&doc)) {
        run(&mut doc, &EditSketch { element, feature: sketch, op });
    }
    for (seeds, depth) in [
        (&samples::EXTRUDE_1_SEEDS[..], samples::EXTRUDE_1_DEPTH),
        (&samples::EXTRUDE_2_SEEDS[..], samples::EXTRUDE_2_DEPTH),
    ] {
        let feature = FeatureId::new();
        run(&mut doc, &AddExtrude { element, feature, extrude: cadrs_core::ExtrudeFeature::default() });
        let regions = samples::region_refs(sketch, &geometry(&doc), seeds);
        let mut extrude = samples::extrude_of(regions, depth);
        if add && depth == samples::EXTRUDE_2_DEPTH {
            extrude.op = cadrs_core::BooleanOp::Add;
        }
        run(&mut doc, &SetExtrude { element, feature, extrude, label: "Extrude".into() });
    }
}

fn part_look(world: &mut World, spec: &str) {
    let mut words = spec.splitn(3, ' ');
    let (Some(n), Some(colour)) = (words.next().and_then(|n| n.parse::<usize>().ok()), words.next()) else {
        warn!("part-look: expected `<n> <#rrggbb|-> [material]`");
        return;
    };
    let material = words.next().map(str::trim).filter(|m| !m.is_empty());
    let Some(part) = n.checked_sub(1).and_then(|i| world.resource::<crate::parts::PartCache>().parts.get(i).map(|p| p.id)) else {
        warn!("part-look: no part {n}");
        return;
    };
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    let Some(element) = doc.active_element().map(|e| e.id) else { return };
    // `#rrggbbaa`: with an opacity (a see-through part).
    let (colour, alpha) = match colour.trim_start_matches('#') {
        h if h.len() == 8 => (&h[..6], u8::from_str_radix(&h[6..], 16).ok()),
        _ => (colour, None),
    };
    if let Some(a) = cadrs_core::Appearance::from_hex(colour).map(|a| alpha.map_or(a, |x| a.with_alpha(x))) {
        let _ = doc.execute(&cadrs_core::commands::SetPartAppearance { element, parts: vec![part], appearance: Some(a) });
    }
    if let Some(m) = material {
        match cadrs_core::material::library(m) {
            Some(mat) => {
                let _ = doc.execute(&cadrs_core::commands::SetPartMaterial { element, parts: vec![part], material: Some(mat) });
            }
            None => warn!("part-look: no material {m:?}"),
        }
    }
}

fn populate_sketch(world: &mut World, n: usize) {
    let Some(s) = world.get_resource::<SketchSession>().cloned() else {
        warn!("populate-sketch needs an open sketch");
        return;
    };
    let start = std::time::Instant::now();
    let (op, rects) = populate_ops(n);
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let edit = |op| EditSketch {
        element: s.element,
        feature: s.feature,
        op,
    };
    if let Err(e) = doc.execute(&edit(op)) {
        warn!("populate-sketch: {e}");
        return;
    }
    let dims = crate::sketch_tools::session_sketch(Some(&s), Some(&doc))
        .map(|sk| dimension_ops(sk, &rects));
    if let Some(op) = dims
        && let Err(e) = doc.execute(&edit(op))
    {
        warn!("populate-sketch dimensions: {e}");
    }
    let t = start.elapsed().as_secs_f64() * 1000.0;
    info!("populate-sketch {n}: {t:.1} ms");
    println!("perf populate-sketch {n}: {t:.1} ms (two commands, each solving the sketch)");
}

/// `pcb-board <folder>/<name>` (see the module docs).
fn pcb_board(world: &mut World, spec: &str) {
    use cadrs_core::commands::{RenameDocument, RenameElement, SetUnits};
    let dir = fixtures_dir().join("idf");
    let read = |ext: &str| std::fs::read_to_string(dir.join(format!("{spec}.{ext}")));
    let pcb = match (read("emn"), read("emp")) {
        (Ok(emn), Ok(emp)) => match cadrs_pcb::PcbBoard::read(&emn, &emp) {
            Ok(b) => b,
            Err(e) => {
                warn!("pcb-board {spec}: {e}");
                return;
            }
        },
        (Err(e), _) | (_, Err(e)) => {
            warn!("pcb-board {spec}: {e}");
            return;
        }
    };
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc.active_element().map(|e| e.id) else {
        return;
    };
    let units = cadrs_sketch::units::Units { length: cadrs_sketch::units::LengthUnit::Millimeter, ..doc.doc.units };
    let steps: [&dyn cadrs_core::Command; 3] = [
        &RenameDocument { name: format!("{} (IDF)", pcb.name()) },
        &RenameElement { id: element, name: pcb.name().to_string() },
        &SetUnits { units },
    ];
    for c in steps {
        if let Err(e) = doc.execute(c) {
            warn!("pcb-board: {e}");
        }
    }
    drop_default_assembly(&mut doc, "pcb-board");
    if let Err(e) = cadrs_pcb::sample::build_in(&mut *doc, element, &pcb) {
        warn!("pcb-board {spec}: {e}");
    }
}

/// The directory of the stand-in documents: `fixtures/` next to the current directory, else the
/// one in the source tree.
pub(crate) fn fixtures_dir() -> std::path::PathBuf {
    let local = std::path::PathBuf::from("fixtures");
    if local.is_dir() {
        return local;
    }
    std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures"))
}

/// Opens `fixtures/<name>.cadrs` as the active document, saved in the document store.
fn open_fixture(world: &mut World, name: &str) {
    let path = fixtures_dir().join(format!("{name}.cadrs"));
    let file = match cadrs_core::Store::load_path(&path) {
        Ok(f) => f,
        Err(e) => {
            warn!("fixture {}: {e}", path.display());
            return;
        }
    };
    if let Some(store) = world.get_resource::<crate::DocumentStore>()
        && let Err(e) = store.0.save(&file.document, &file.meta)
    {
        warn!("fixture {name}: {e}");
    }
    // P3D.3: a stand-in shipped with its history (`fixtures/<name>.history.ron`).
    let history = fixtures_dir().join(format!("{name}.history.ron"));
    if history.is_file()
        && let Some(store) = world.get_resource::<crate::DocumentStore>()
    {
        match cadrs_core::history_log::HistoryLog::load_path(&history) {
            Ok(log) if log.document == file.document.id => {
                if let Err(e) = log.save(&store.0) {
                    warn!("fixture {name} history: {e}");
                }
            }
            Ok(_) => warn!("fixture {name}: its history is another document's"),
            Err(e) => warn!("fixture {name} history: {e}"),
        }
    }
    world.insert_resource(crate::ActiveDocument::stored(file.document, file.meta));
}

/// `step-fixture <path>`: the bracket pair STEP file, written now (the kernel thread builds it).
fn step_fixture(path: &str) {
    let path = std::path::PathBuf::from(path);
    let bytes = cadrs_core::rebuild::run_on_worker(cadrs_core::samples::bracket_pair::step).wait();
    match bytes {
        Some(Ok(b)) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(e) = std::fs::write(&path, b) {
                warn!("step-fixture {}: {e}", path.display());
            }
        }
        Some(Err(e)) => warn!("step-fixture: {e}"),
        None => warn!("step-fixture: the kernel thread stopped"),
    }
}

/// A thumbnail of `doc`'s first Part Studio (the store's placeholder otherwise).
pub(crate) fn studio_thumbnail(doc: &cadrs_core::Document) -> Option<image::RgbaImage> {
    let el = doc.elements.iter().find(|e| matches!(e.kind, cadrs_core::ElementKind::PartStudio { .. }))?;
    let build = cadrs_core::rebuild::build(el.features());
    let list: Vec<(&cadrs_core::Solid, [u8; 3])> =
        build.parts.iter().map(|p| (&*p.solid, cadrs_core::appearance::part_appearance(p, el.part_props()).rgb)).collect();
    (!list.is_empty()).then(|| cadrs_core::assembly::thumb::render(&list, 96))
}

/// P3G.2: adds "Drawing 1" to the open document, with a 1:1 Front view of the block `element`
/// (a Part Studio or its linked copy) on an ANSI A sheet, through the command layer.
fn add_front_view_drawing(world: &mut World, element: cadrs_core::ElementId) {
    add_front_view_drawing_of(world, element, Some(cadrs_core::samples::linked_block::PART), [110.0, 140.0]);
}

/// P3G.3: as [`add_front_view_drawing`], of `part` of `element` (the whole studio with `None`),
/// the view centred at `at` on the sheet.
pub(crate) fn add_front_view_drawing_of(world: &mut World, element: cadrs_core::ElementId, part_id: Option<cadrs_core::PartId>, at: [f64; 2]) {
    use cadrs_drawing::{Drawing, DrawingOp, NamedView, ObjectRef, Scale, View, template};
    let doc = world.resource::<crate::ActiveDocument>().doc.clone();
    let Some(t) = template::builtin("ANSI_A_MM.dwt") else { return };
    let mut d = Drawing::from_template(&t, None);
    let sheet = d.sheets[0].id;
    let Some(src) = cadrs_core::drawing_source::live_source(&doc, element) else { return };
    let part = ObjectRef { element: element.0, part: cadrs_core::drawing_source::part_key(part_id) };
    let mut v = View::base(part, NamedView::Front, Scale::new(1, 1), at);
    v.source_hash = src.hash_of(part.part);
    if d.apply(&DrawingOp::SetSource(src)).is_err() || d.apply(&DrawingOp::InsertView { sheet, view: v }).is_err() {
        return;
    }
    let el = cadrs_core::Element::drawing("Drawing 1", d);
    let id = el.id;
    let mut active = world.resource_mut::<crate::ActiveDocument>();
    if active.execute(&cadrs_core::commands::InsertElement { element: el, after: None, label: "Create Drawing".into() }).is_ok() {
        active.set_active(id);
    }
}

/// P3G.1 set-ups on the linked-documents block (see the module docs).
fn linked_block(world: &mut World, arg: &str) {
    use cadrs_core::history_log::{HistoryLog, Origin};
    use cadrs_core::samples::linked_block as lb;
    let store = world.resource::<crate::DocumentStore>().0.clone();
    let now = world.resource::<crate::AppClock>().now();
    let user = world.resource::<crate::UserProfile>().id.clone();
    let mut words = arg.split_whitespace();
    match words.next() {
        None => {
            let Ok(a) = lb::document() else { return };
            let mut meta = cadrs_core::DocumentMeta::new(&user, now - 7_200);
            meta.last_opened = Some(now - 3_600);
            // Last modified when V1 was made (P3E.2, P3E.1 judge: Modified read earlier than V1).
            meta.modified = now - 7_000;
            // The library folder "Linked parts" holds it (ER1.2 locations).
            let (lib, _) = store.list();
            let folder = cadrs_core::FolderId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0f01);
            if lib.folders.iter().all(|f| f.id != folder) {
                let mut after = lib.clone();
                after.folders.push(cadrs_core::FolderEntry { id: folder, name: "Linked parts".into(), created: now - 7_200, owned_by: user.clone() });
                if let Err(e) = store.sync(&lib, &after) {
                    warn!("linked-block: {e}");
                }
            }
            meta.folder = Some(folder);
            if let Err(e) = store.create(&a, &meta) {
                warn!("linked-block: {e}");
                return;
            }
            if let Some(img) = studio_thumbnail(&a) {
                let _ = store.write_thumbnail(a.id, &img);
            }
            let mut log = HistoryLog::start(&a, now - 7_200, &user);
            log.create_version("V1", "The block, 50 × 30 × 25", now - 7_000, &user);
            if let Err(e) = log.save(&store) {
                warn!("linked-block: {e}");
            }
            // A copy of it with no versions yet (DV1.8: Create version in the browser).
            let mut copy = a.clone();
            copy.id = cadrs_core::DocumentId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0400);
            copy.name = "Block copy".into();
            let mut cmeta = cadrs_core::DocumentMeta::new(&user, now - 5_000);
            cmeta.folder = Some(folder);
            if store.create(&copy, &cmeta).is_ok()
                && let Some(img) = studio_thumbnail(&copy)
            {
                let _ = store.write_thumbnail(copy.id, &img);
            }
            let b = lb::consumer();
            let meta = cadrs_core::DocumentMeta::new(&user, now - 600);
            if let Err(e) = store.create(&b, &meta) {
                warn!("linked-block: {e}");
            }
            let mut doc = crate::ActiveDocument::stored(b, meta);
            doc.set_active(lb::CONSUMER_ASSEMBLY);
            world.insert_resource(doc);
        }
        Some("edit") => {
            world.resource_mut::<crate::linked::LinkStatus>().invalidate();
            let Ok(file) = store.load(lb::DOCUMENT) else { return };
            let mut a = file.document;
            let mut h = cadrs_core::History::default();
            // `linked-block edit 55`: another depth (P3G.2); 40 by default.
            let depth: f64 = words.next().and_then(|w| w.parse().ok()).unwrap_or(lb::EDITED_HEIGHT);
            if let Err(e) = lb::set_height(&mut cadrs_core::samples::gear_cover::DocHistory(&mut a, &mut h), lb::STUDIO, depth) {
                warn!("linked-block edit: {e}");
                return;
            }
            let mut meta = file.meta;
            meta.modified = now;
            let _ = store.save(&a, &meta);
            if let Some(img) = studio_thumbnail(&a) {
                let _ = store.write_thumbnail(a.id, &img);
            }
            if let Ok(Some(mut log)) = HistoryLog::load(&store, lb::DOCUMENT) {
                log.record(&a, Origin::Command("Edit Extrude 1".into()), now - 60, &user);
                log.create_version("", &format!("Depth {depth}"), now - 30, &user);
                let _ = log.save(&store);
            }
        }
        Some("trash") => {
            if let Ok(mut file) = store.load(lb::DOCUMENT) {
                file.meta.trashed = Some(now);
                let _ = store.save(&file.document, &file.meta);
            }
            world.resource_mut::<crate::linked::LinkStatus>().invalidate();
        }
        Some("purge") => {
            let (lib, _) = store.list();
            let mut after = lib.clone();
            after.entries.retain(|e| e.id != lb::DOCUMENT);
            if let Err(e) = store.sync(&lib, &after) {
                warn!("linked-block purge: {e}");
            }
            world.resource_mut::<crate::linked::LinkStatus>().invalidate();
        }
        // P3G.1 (DV1.5): "Cycle partner", whose assembly "Top" holds B's Assembly 1 at B's newest
        // version, with a version of its own: inserting it into B's Assembly 1 is circular.
        Some("cycle") => {
            use cadrs_core::external::{InsertLinked, SourceRef, snapshot};
            let Ok(Some(log)) = HistoryLog::load(&store, lb::CONSUMER) else {
                warn!("linked-block cycle: B has no versions");
                return;
            };
            let Some(v) = log.versions().last().cloned() else { return };
            let Some(b) = log.document_at_version(v.id()) else { return };
            let r = SourceRef::version(Some(lb::CONSUMER), lb::CONSUMER_ASSEMBLY, v.id());
            let Ok(snap) = snapshot(&b, r, v.name()) else { return };
            let mut c = cadrs_core::Document::empty("Cycle partner");
            c.id = cadrs_core::DocumentId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0500);
            let top = cadrs_core::Element::assembly("Top");
            let top_id = top.id;
            c.elements.push(top);
            let inst = cadrs_core::assembly::Instance::new(
                cadrs_core::assembly::InstanceId::new(),
                cadrs_core::assembly::InstanceSource::Assembly { element: snap.root },
                cadrs_core::assembly::Pose::IDENTITY,
            );
            let mut h = cadrs_core::History::default();
            if let Err(e) = h.execute(&mut c, &InsertLinked { element: top_id, snapshot: snap, instances: vec![inst], reference: r }) {
                warn!("linked-block cycle: {e}");
                return;
            }
            let meta = cadrs_core::DocumentMeta::new(&user, now - 300);
            if let Err(e) = store.create(&c, &meta) {
                warn!("linked-block cycle: {e}");
                return;
            }
            let mut clog = HistoryLog::start(&c, now - 300, &user);
            clog.create_version("V1", "", now - 200, &user);
            let _ = clog.save(&store);
        }
        Some("versions") => {
            let Ok(d) = lb::versions_document() else { return };
            let meta = cadrs_core::DocumentMeta::new(&user, now - 600);
            if let Err(e) = store.create(&d, &meta) {
                warn!("linked-block versions: {e}");
            }
            let mut doc = crate::ActiveDocument::stored(d, meta);
            doc.set_active(lb::VERSIONS_STUDIO);
            world.insert_resource(doc);
        }
        Some("height") => {
            let depth: f64 = words.next().and_then(|w| w.parse().ok()).unwrap_or(lb::EDITED_HEIGHT);
            let Some(mut doc) = world.get_resource_mut::<crate::ActiveDocument>() else { return };
            let studio = if doc.doc.element(lb::VERSIONS_STUDIO).is_some() { lb::VERSIONS_STUDIO } else { lb::STUDIO };
            if let Err(e) = lb::set_height(&mut *doc, studio, depth) {
                warn!("linked-block height: {e}");
            }
        }
        Some("consumer") => {
            use cadrs_core::external::{InsertLinked, SourceRef};
            let n: usize = words.next().and_then(|w| w.parse().ok()).unwrap_or(3);
            let Some(v) = crate::linked::resolver(world).0.latest(lb::DOCUMENT) else { return };
            let r = SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, v.id());
            let doc = world.resource::<crate::ActiveDocument>().doc.clone();
            let snap = match crate::linked::resolver(world).0.resolve(r, &doc, None) {
                Ok(s) => s,
                Err(e) => {
                    warn!("linked-block consumer: {e}");
                    return;
                }
            };
            let mut next = doc.element(lb::CONSUMER_ASSEMBLY).and_then(|e| e.assembly_model()).map(|a| a.instances.len() as f64).unwrap_or(0.0);
            let instances: Vec<cadrs_core::assembly::Instance> = (0..n)
                .map(|_| {
                    let x = next * 70.0;
                    next += 1.0;
                    cadrs_core::assembly::Instance::new(
                        cadrs_core::assembly::InstanceId::new(),
                        cadrs_core::assembly::InstanceSource::Part { element: snap.root, part: lb::PART },
                        cadrs_core::assembly::Pose::translation([x, 0.0, 0.0]),
                    )
                })
                .collect();
            if let Err(e) = world.resource_mut::<crate::ActiveDocument>().execute(&InsertLinked { element: lb::CONSUMER_ASSEMBLY, snapshot: snap, instances, reference: r }) {
                warn!("linked-block consumer: {e}");
            }
        }
        Some("chain") => {
            use cadrs_core::external::{InsertLinked, SourceRef, snapshot};
            let Ok(Some(alog)) = HistoryLog::load(&store, lb::DOCUMENT) else { return };
            let Some(va) = alog.versions().first().cloned() else { return };
            let Some(a) = alog.document_at_version(va.id()) else { return };
            let ra = SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, va.id());
            let Ok(snap) = snapshot(&a, ra, va.name()) else { return };
            let mut b = cadrs_core::Document::empty("Block sub");
            b.id = cadrs_core::DocumentId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0600);
            let mut sub = cadrs_core::Element::assembly("Sub");
            sub.id = cadrs_core::ElementId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0601);
            let sub_id = sub.id;
            b.elements.push(sub);
            let inst = cadrs_core::assembly::Instance::new(
                cadrs_core::assembly::InstanceId::new(),
                cadrs_core::assembly::InstanceSource::Part { element: snap.root, part: lb::PART },
                cadrs_core::assembly::Pose::IDENTITY,
            );
            let mut h = cadrs_core::History::default();
            if let Err(e) = h.execute(&mut b, &InsertLinked { element: sub_id, snapshot: snap, instances: vec![inst], reference: ra }) {
                warn!("linked-block chain: {e}");
                return;
            }
            let meta = cadrs_core::DocumentMeta::new(&user, now - 1_800);
            if let Err(e) = store.create(&b, &meta) {
                warn!("linked-block chain: {e}");
                return;
            }
            let mut blog = HistoryLog::start(&b, now - 1_800, &user);
            let vb = blog.create_version("V1", "Sub with the block", now - 1_700, &user);
            let _ = blog.save(&store);
            let rb = SourceRef::version(Some(b.id), sub_id, vb);
            let Ok(snap_b) = snapshot(&b, rb, "V1") else { return };
            let inst = cadrs_core::assembly::Instance::new(
                cadrs_core::assembly::InstanceId::new(),
                cadrs_core::assembly::InstanceSource::Assembly { element: snap_b.root },
                cadrs_core::assembly::Pose::IDENTITY,
            );
            if let Err(e) = world.resource_mut::<crate::ActiveDocument>().execute(&InsertLinked { element: lb::CONSUMER_ASSEMBLY, snapshot: snap_b, instances: vec![inst], reference: rb }) {
                warn!("linked-block chain: {e}");
            }
        }
        // P3G.2 (DV1.6): "Block user 2", a stored document whose Assembly 1 links A's first
        // version (a second consumer for Where used).
        Some("second-consumer") => {
            use cadrs_core::external::{InsertLinked, SourceRef, snapshot};
            let Ok(Some(alog)) = HistoryLog::load(&store, lb::DOCUMENT) else { return };
            let Some(va) = alog.versions().first().cloned() else { return };
            let Some(a) = alog.document_at_version(va.id()) else { return };
            let ra = SourceRef::version(Some(lb::DOCUMENT), lb::STUDIO, va.id());
            let Ok(snap) = snapshot(&a, ra, va.name()) else { return };
            let mut d = cadrs_core::Document::empty("Block user 2");
            d.id = cadrs_core::DocumentId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0700);
            let asm = cadrs_core::Element::assembly("Assembly 1");
            let asm_id = asm.id;
            d.elements.push(asm);
            let inst = cadrs_core::assembly::Instance::new(
                cadrs_core::assembly::InstanceId::new(),
                cadrs_core::assembly::InstanceSource::Part { element: snap.root, part: lb::PART },
                cadrs_core::assembly::Pose::IDENTITY,
            );
            let mut h = cadrs_core::History::default();
            if h.execute(&mut d, &InsertLinked { element: asm_id, snapshot: snap, instances: vec![inst], reference: ra }).is_ok()
                && let Err(e) = store.create(&d, &cadrs_core::DocumentMeta::new(&user, now - 900))
            {
                warn!("linked-block second-consumer: {e}");
            }
        }
        Some("open-sub") => {
            let Ok(file) = store.load(cadrs_core::DocumentId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0600)) else { return };
            let mut doc = crate::ActiveDocument::stored(file.document, file.meta);
            doc.set_active(cadrs_core::ElementId::from_u128(0x3a61_0000_0000_0000_0000_0000_0000_0601));
            world.insert_resource(doc);
        }
        Some("drawing") => {
            let doc = world.resource::<crate::ActiveDocument>().doc.clone();
            let Some(copy) = doc.linked.iter().find(|l| l.source.element == lb::STUDIO).map(|l| l.id()) else { return };
            add_front_view_drawing(world, copy);
        }
        Some("versions-instance") => {
            let inst = cadrs_core::assembly::Instance::new(
                cadrs_core::assembly::InstanceId::new(),
                cadrs_core::assembly::InstanceSource::Part { element: lb::VERSIONS_STUDIO, part: lb::PART },
                cadrs_core::assembly::Pose::IDENTITY,
            );
            if let Err(e) = world.resource_mut::<crate::ActiveDocument>().execute(&cadrs_core::assembly::commands::InsertInstance { element: lb::VERSIONS_ASSEMBLY, instance: inst }) {
                warn!("linked-block versions-instance: {e}");
            }
        }
        Some("versions-drawing") => add_front_view_drawing(world, lb::VERSIONS_STUDIO),
        // P3G.2 (DV2.3, ER6.6): in Block versions' Assembly 1, the V1 instance (<1>) Fastened with
        // its bottom-face centre on the workspace instance's (<2>) top-face centre, which is fixed.
        Some("mate") => {
            use cadrs_core::assembly::commands::{AddMateFeature, SetInstancesFixed};
            use cadrs_core::assembly::connector::{ConnectorFrame, MateConnector};
            use cadrs_core::assembly::mate::{Mate, MateFeature, MateId, MateKind, MateType};
            let doc = world.resource::<crate::ActiveDocument>().doc.clone();
            let Some(model) = doc.element(lb::VERSIONS_ASSEMBLY).and_then(|e| e.assembly_model()).cloned() else { return };
            let (Some(v1), Some(ws)) = (model.instances.iter().find(|i| i.link.is_some()), model.instances.iter().find(|i| i.link.is_none())) else { return };
            let (v1, ws) = (v1.id, ws.id);
            let top = doc.element(lb::VERSIONS_STUDIO).and_then(|e| e.feature(lb::EXTRUDE)).and_then(|f| f.extrude()).map(|e| e.depth).unwrap_or(lb::HEIGHT);
            let ws_pose = model.instance(ws).map(|i| i.pose).unwrap_or(cadrs_core::assembly::Pose::IDENTITY);
            let mut active = world.resource_mut::<crate::ActiveDocument>();
            let _ = active.execute(&SetInstancesFixed { element: lb::VERSIONS_ASSEMBLY, instances: vec![ws], fixed: true });
            let bottom = ConnectorFrame { origin: [25.0, 15.0, 0.0], ..ConnectorFrame::default() };
            let top = ConnectorFrame { origin: [25.0, 15.0, top], ..ConnectorFrame::default() };
            let m = Mate::new(MateType::Fastened, MateConnector::at(v1, bottom), MateConnector::at(ws, top));
            let f = MateFeature::new(MateId::new(), "Fastened 1", MateKind::Mate(m));
            // The V1 block lands on the workspace block: its pose is the workspace one moved up.
            let mut pose = ws_pose;
            pose.translation[2] += top.origin[2];
            if let Err(e) = active.execute(&AddMateFeature { element: lb::VERSIONS_ASSEMBLY, feature: f, poses: vec![(v1, pose)] }) {
                warn!("linked-block mate: {e}");
            }
        }
        // P3G.2 (DV2.3): the Block part's part number in the open document's workspace.
        Some("part-number") => {
            use cadrs_core::properties::{PropertyKey, PropertyOwner, PropertyValue, SetProperties};
            let n = words.next().unwrap_or("BLK-002").to_string();
            let studio = if world.resource::<crate::ActiveDocument>().doc.element(lb::VERSIONS_STUDIO).is_some() { lb::VERSIONS_STUDIO } else { lb::STUDIO };
            let cmd = SetProperties { owners: vec![PropertyOwner::Part { element: studio, part: lb::PART }], values: vec![(PropertyKey::PartNumber, PropertyValue::Text(n))], label: "Set part number".into() };
            if let Err(e) = world.resource_mut::<crate::ActiveDocument>().execute(&cmd) {
                warn!("linked-block part-number: {e}");
            }
        }
        Some(other) => warn!("linked-block: unknown {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn populating_makes_n_curves() {
        let mut s = Sketch::new();
        let (op, rects) = populate_ops(500);
        op.apply(&mut s).unwrap();
        assert_eq!(s.curves.len(), 500);
        assert_eq!(rects.len(), 100);
        dimension_ops(&s, &rects).apply(&mut s).unwrap();
        assert_eq!(s.dimensions.len(), 20);
    }
}
