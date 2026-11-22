//! The Machine Builder's draft on the CNC machines sample (`cadrs_core::samples::cnc_machines`),
//! without the app: the rows the mounts give, what the edits do and that they reach the
//! document in one undo step, and that a jog is a preview and nothing more.

use cadrs_core::assembly::machine::{self, MachineDef, Side};
use cadrs_core::samples::cnc_machines as cnc;
use cadrs_ui::form::Value;

use super::*;
use crate::ActiveDocument;

/// The sample's document and the draft of one of its mills, refreshed.
fn open(element: ElementId) -> (ActiveDocument, Draft) {
    let doc = ActiveDocument::new(cnc::document().unwrap());
    let mut d = Draft::new(element, stored_def(&doc, element));
    refresh(&doc, &mut d);
    (doc, d)
}

fn refresh(doc: &ActiveDocument, d: &mut Draft) {
    let asm = doc
        .doc
        .element(d.element)
        .and_then(|e| e.assembly_model())
        .unwrap();
    d.refresh(&machine::flat_model(&doc.doc, asm), &machine::stored_frames);
}

/// Whether two previews place the same instances at the same places (to 1e-9).
fn same(a: &[(InstanceId, Pose)], b: &[(InstanceId, Pose)]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|((i, p), (j, q))| {
            i == j
                && p.translation
                    .iter()
                    .chain(p.rotation.iter().flatten())
                    .zip(q.translation.iter().chain(q.rotation.iter().flatten()))
                    .all(|(x, y)| (x - y).abs() < 1e-9)
        })
}

fn names(d: &Draft) -> Vec<&str> {
    d.def.axes.iter().map(|a| a.name.as_str()).collect()
}

#[test]
fn the_mounts_give_the_axis_rows() {
    let (doc, stored) = open(cnc::MILL_5);
    // A definition with only the sample's two mounts: nothing until both are set.
    let mut d = Draft::new(cnc::MILL_5, None);
    d.set_mount(Mount::Tool, stored.def.tool_mount);
    refresh(&doc, &mut d);
    assert!(!d.mounted() && d.def.axes.is_empty() && d.resolved.is_none());
    d.set_mount(Mount::Table, stored.def.table_mount);
    refresh(&doc, &mut d);
    // The tool's side from the base out, then the table's.
    assert_eq!(names(&d), ["Z", "B", "Y", "X", "C"]);
    let sides: Vec<Side> = d.def.axes.iter().map(|a| a.side).collect();
    assert_eq!(
        sides,
        [
            Side::Tool,
            Side::Tool,
            Side::Table,
            Side::Table,
            Side::Table
        ]
    );
    assert_eq!(d.axis_info[1].name, "B");
    assert!(d.axis_info[1].rotary && !d.axis_info[0].rotary);
    assert!(d.resolved.is_some(), "{:?}", d.issues);
    // Where the document has the machine, every axis reads what it reads there, and
    // nothing is previewed.
    assert!(!d.jogged() && d.poses().is_empty());
    assert!(d.changed());
    // B's slider is its travel; C turns without stops.
    let b = d.axis_track(1);
    assert_eq!(
        (b.min, b.max, b.limits),
        (-100.0, 100.0, Some((-100.0, 100.0)))
    );
    assert_eq!(d.axis_track(4).limits, None);
}

#[test]
fn the_magazine_is_left_for_an_actuator() {
    let (doc, stored) = open(cnc::MILL_4);
    assert_eq!(names(&stored), ["Z", "Y", "X", "A"]);
    assert_eq!(stored.def.actuators.len(), 1);
    assert!(stored.free.is_empty() && stored.actuator_info[0].is_some());
    assert_eq!(stored.magazine(), Some(0));
    // Without the actuator its mate is a free joint, which Add as actuator takes.
    let mut d = Draft::new(cnc::MILL_4, None);
    d.set_mount(Mount::Tool, stored.def.tool_mount);
    d.set_mount(Mount::Table, stored.def.table_mount);
    refresh(&doc, &mut d);
    assert_eq!(d.free.len(), 1);
    assert_eq!(d.free[0].name, "Magazine");
    d.add_actuator(0).unwrap();
    refresh(&doc, &mut d);
    assert!(d.free.is_empty());
    assert_eq!(d.def.actuators[0].name, "Magazine");
    // Positions where it is jogged to, the magazine's pockets from them.
    d.add_position_here(0);
    d.jog_actuator(0, 45.0);
    d.add_position_here(0);
    assert_eq!(
        d.def.actuators[0].positions,
        [
            ("Position 1".to_string(), 0.0),
            ("Position 2".to_string(), 45.0)
        ]
    );
    d.set_magazine(Some(0));
    d.pockets_from_positions();
    d.jog_actuator(0, 90.0);
    d.add_pocket_here();
    let pockets: Vec<(u32, f64)> = d
        .def
        .tool_change
        .pockets
        .iter()
        .map(|p| (p.number, p.value))
        .collect();
    assert_eq!(pockets, [(1, 0.0), (2, 45.0), (3, 90.0)]);
    // The definition is one the assembly takes.
    let mut doc = doc;
    doc.execute(&d.command()).unwrap();
    // Removing the actuator takes the magazine and its pockets with it.
    d.remove_actuator(0);
    assert!(d.def.tool_change.magazine.is_none() && d.def.tool_change.pockets.is_empty());
}

#[test]
fn a_jog_previews_and_writes_nothing() {
    let (doc, mut d) = open(cnc::MILL_5);
    let before = doc.doc.clone();
    let (b, c) = (d.axis_named("B").unwrap(), d.axis_named("C").unwrap());
    d.jog(b, 30.0);
    d.jog(c, 45.0);
    assert!(d.jogged());
    assert_eq!((d.value(b), d.value(c)), (30.0, 45.0));
    let poses = d.poses();
    // The swivel head with what it carries, and the platter: not the base, the column, the
    // table or the Z slide.
    let moved: Vec<_> = poses.iter().map(|p| p.0).collect();
    assert!(!moved.is_empty() && moved.len() < d.resolved.as_ref().unwrap().tree.instances.len());
    for (id, pose) in &poses {
        let asm = doc
            .doc
            .element(cnc::MILL_5)
            .unwrap()
            .assembly_model()
            .unwrap();
        assert_ne!(asm.instance(*id).unwrap().pose, *pose);
    }
    // The tool mount leans by B: its Z was straight down.
    let (tool, _) = d.resolved.as_ref().unwrap().mounts(&d.values());
    let z = tool.unwrap().z;
    assert!((z[2] + 30f64.to_radians().cos()).abs() < 1e-9, "{z:?}");
    // Nothing of it is in the document, and no undo step was made.
    assert_eq!(doc.doc, before);
    assert_eq!(doc.history.undo_len(), 0);
    assert!(!d.changed());
    // Beyond its travel the axis is still jogged there, and named.
    d.jog(b, 130.0);
    assert_eq!(d.beyond_travel(), ["B"]);
    // Home and Reset pose.
    d.go_home();
    assert_eq!(d.values(), d.resolved.as_ref().unwrap().home());
    d.jog(b, 10.0);
    d.reset_pose();
    assert!(!d.jogged() && d.poses().is_empty());
}

#[test]
fn edits_keep_the_machine_where_it_is_jogged() {
    let (doc, mut d) = open(cnc::MILL_5);
    let b = d.axis_named("B").unwrap();
    d.jog(b, 30.0);
    let at = d.poses();
    // Reversed, the axis reads −30 on the same pose, its travel the other way round.
    d.reverse_axis(b);
    refresh(&doc, &mut d);
    assert_eq!(d.value(b), -30.0);
    assert!(same(&d.poses(), &at));
    assert!(d.def.axes[b].reversed != d.stored.as_ref().unwrap().axes[b].reversed);
    d.reverse_axis(b);
    refresh(&doc, &mut d);
    assert!(d.value(b) == 30.0 && same(&d.poses(), &at));
    // Zero here: it reads 0 there, and its travel (the mate's limits) is counted from there.
    d.zero_here(b);
    refresh(&doc, &mut d);
    assert_eq!(d.value(b), 0.0);
    assert!(same(&d.poses(), &at));
    let a = &d.def.axes[b];
    assert!(
        (a.min + 130.0).abs() < 1e-9 && (a.max - 70.0).abs() < 1e-9,
        "{a:?}"
    );
    // A typed travel stops following the mate, until told to again.
    d.set_travel(b, true, 45.0);
    refresh(&doc, &mut d);
    assert!(!d.def.axes[b].limits_from_mate && d.def.axes[b].max == 45.0);
    d.travel_from_mate(b);
    refresh(&doc, &mut d);
    assert!((d.def.axes[b].max - 70.0).abs() < 1e-9);
    // Names: another axis's is refused, and the tool change position follows a rename.
    d.tool_change_here();
    assert!(!d.rename_axis(b, "Z") && !d.rename_axis(b, " "));
    assert!(d.rename_axis(b, "B2"));
    assert!(d.def.tool_change.position.iter().any(|p| p.0 == "B2"));
}

#[test]
fn edits_reach_the_document_in_one_step_and_undo() {
    let (mut doc, mut d) = open(cnc::MILL_5);
    let before = stored_def(&doc, cnc::MILL_5).unwrap();
    let x = d.axis_named("X").unwrap();
    assert!(d.rename_axis(x, "U"));
    d.set_travel(x, false, -200.0);
    d.set_rapid(x, 12_000.0);
    d.home_here(x);
    let c = d.axis_named("C").unwrap();
    d.set_continuous(c, false);
    refresh(&doc, &mut d);
    assert!(d.changed());
    assert_eq!(d.command().label, "Edit machine");
    doc.execute(&d.command()).unwrap();
    assert_eq!(doc.history.undo_len(), 1);
    let now: MachineDef = stored_def(&doc, cnc::MILL_5).unwrap();
    let u = now.axis_named("U").unwrap();
    assert_eq!(
        (u.min, u.rapid, u.limits_from_mate),
        (-200.0, 12_000.0, false)
    );
    let c = now.axis_named("C").unwrap();
    assert!(!c.continuous && (c.min, c.max) == (-360.0, 360.0));
    // What the document has is what a builder opened on it shows.
    let mut again = Draft::new(cnc::MILL_5, Some(now.clone()));
    refresh(&doc, &mut again);
    assert!(!again.changed(), "the stored rows are detect's rows");
    assert_eq!(names(&again), ["Z", "B", "Y", "U", "C"]);
    doc.undo();
    assert_eq!(stored_def(&doc, cnc::MILL_5).unwrap(), before);
    // Remove machine is a step too.
    doc.execute(&d.remove_command()).unwrap();
    assert!(stored_def(&doc, cnc::MILL_5).is_none());
    doc.undo();
    assert_eq!(stored_def(&doc, cnc::MILL_5).unwrap(), before);
}

/// The panel's form fields: an axis's settings by key, the magazine, the controller's own.
#[test]
fn form_fields_edit_the_draft() {
    let (doc, d) = open(cnc::MILL_4);
    let mut b = MachineBuilder::new(cnc::MILL_4, d.stored.clone());
    b.draft = d;
    let a = b.draft.axis_named("A").unwrap();
    let key = |k: &str| format!("mb-ax{}-{k}", a + 1);
    // The settings an expanded row shows are the ones its keys take.
    let shown: Vec<String> = axis_fields(&b.draft, a)
        .iter()
        .filter_map(|f| match f {
            Field::Number { key, .. } | Field::Check { key, .. } | Field::Button { key, .. } => {
                Some(key.clone())
            }
            _ => None,
        })
        .collect();
    for k in ["zero", "home", "home-here", "cont", "rapid", "feed"] {
        assert!(shown.contains(&key(k)), "{k} in {shown:?}");
    }
    assert!(b.draft.def.axes[a].continuous && !shown.contains(&key("min")));
    set_field(&mut b, &key("cont"), &Value::Bool(false));
    refresh(&doc, &mut b.draft);
    set_field(&mut b, &key("max"), &Value::Number(120.0));
    set_field(&mut b, &key("home"), &Value::Number(90.0));
    set_field(&mut b, &key("feed"), &Value::Number(1800.0));
    b.draft.jog(a, 15.0);
    set_field(&mut b, &key("home-here"), &Value::Activate);
    let axis = &b.draft.def.axes[a];
    assert_eq!(
        (axis.continuous, axis.max, axis.home, axis.max_feed),
        (false, 120.0, 15.0, 1800.0)
    );
    // Typed text that is no number changes nothing.
    set_field(&mut b, &key("max"), &Value::Text("far".into()));
    assert_eq!(b.draft.def.axes[a].max, 120.0);
    // The magazine: none, then the actuator again.
    set_field(&mut b, "mb-magazine", &Value::Index(0));
    assert!(b.draft.def.tool_change.magazine.is_none());
    set_field(&mut b, "mb-magazine", &Value::Index(1));
    assert_eq!(b.draft.magazine(), Some(0));
    // The controller's fields are the CAM Machine dialog's, without its travel and rates.
    let keys: Vec<String> = controller_fields(&b)
        .iter()
        .filter_map(|f| match f {
            Field::Number { key, .. } | Field::Check { key, .. } | Field::Select { key, .. } => {
                Some(key.clone())
            }
            _ => None,
        })
        .collect();
    for k in [
        "machine",
        "dialect",
        "max-spindle",
        "changer",
        "tcp",
        "rot-feed",
    ] {
        assert!(keys.iter().any(|x| x == k), "{k} in {keys:?}");
    }
    for k in ["travel-0", "rapid", "max-feed", "indexing"] {
        assert!(!keys.iter().any(|x| x == k), "{k}");
    }
    set_field(&mut b, "dialect", &Value::Index(0));
    set_field(&mut b, "tcp", &Value::Bool(true));
    let c = &b.draft.def.controller;
    assert_eq!(c.dialect, cadrs_core::cam::cadrs_cam::Dialect::Fanuc);
    assert!(c.post.rotary.tcp && c.chain.is_none());
    refresh(&doc, &mut b.draft);
    let mut doc = doc;
    doc.execute(&b.draft.command()).unwrap();
}

/// The buttons that only change the builder: arming a mount, opening a row, Home.
#[test]
fn buttons_change_the_builder() {
    let (_, d) = open(cnc::MILL_5);
    let mut b = MachineBuilder::new(cnc::MILL_5, d.stored.clone());
    b.draft = d;
    apply(&mut b, Act::PickMount(Mount::Tool));
    assert_eq!(b.picking, Some(Mount::Tool));
    apply(&mut b, Act::PickMount(Mount::Table));
    assert_eq!(b.picking, Some(Mount::Table));
    apply(&mut b, Act::PickMount(Mount::Table));
    assert_eq!(b.picking, None);
    apply(&mut b, Act::Expand(1));
    assert_eq!(b.expanded.len(), 1);
    apply(&mut b, Act::Expand(1));
    assert!(b.expanded.is_empty());
    b.draft.jog(1, 20.0);
    apply(&mut b, Act::ToolChangeSet);
    assert!(
        b.draft
            .def
            .tool_change
            .position
            .contains(&("B".to_string(), 20.0))
    );
    apply(&mut b, Act::ResetPose);
    assert!(!b.draft.jogged());
    apply(&mut b, Act::ToolChangeGo);
    assert_eq!(b.draft.value(1), 20.0);
    apply(&mut b, Act::FlipMount(Mount::Table));
    assert!(b.draft.def.table_mount.unwrap().flip);
    apply(&mut b, Act::ClearMount(Mount::Tool));
    assert!(b.draft.def.tool_mount.is_none() && !b.draft.mounted());
}

/// A mount picked on a part's own connector (or a face) is stored at its frame, so the
/// machine resolves from the document alone, as a CAM job's does.
#[test]
fn a_picked_mount_is_stored_as_a_fixed_frame() {
    use cadrs_core::assembly::connector::{ConnectorAnchor, ConnectorFrame, MateConnector};
    let (doc, stored) = open(cnc::MILL_3);
    let on = stored.def.tool_mount.unwrap();
    let frame = ConnectorFrame::new([1.0, 2.0, 400.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
    let picked = MateConnector {
        anchor: ConnectorAnchor::Explicit {
            feature: cadrs_core::FeatureId::new(),
        },
        ..MateConnector::at(on.instance, frame)
    };
    assert!(
        machine::stored_frames(&picked).is_none(),
        "lost without its part"
    );
    let mount = fixed(picked);
    assert_eq!(machine::stored_frames(&mount), Some(frame));
    assert_eq!(mount.instance, on.instance);
    // A fixed frame and the Origin are kept as they are.
    assert_eq!(fixed(on), on);
    assert_eq!(fixed(MateConnector::origin()), MateConnector::origin());
    // The machine with it resolves on the stored frames.
    let mut d = Draft::new(cnc::MILL_3, stored.stored.clone());
    d.set_mount(Mount::Tool, Some(mount));
    refresh(&doc, &mut d);
    assert!(d.resolved.is_some(), "{:?}", d.issues);
}
