//! The component editors' dialogs (GS23, GS24, GS26): pin properties, symbol properties, pad
//! properties, footprint properties, a rectangle's corners, the 3D model. Lengths take
//! expressions with units (`-200 mil`, `1.62+2*0.3`).
//!
//! Names: `eda-pin-dialog` (`eda-pin-name`, `eda-pin-number`, `eda-pin-type`,
//! `eda-pin-orientation`, `eda-pin-x`, `eda-pin-y`, `eda-pin-length`, `eda-pin-ok`),
//! `eda-sym-dialog` (`eda-sym-name`, `eda-sym-ref`, `eda-sym-value`, `eda-sym-footprint`,
//! `eda-sym-keywords`, `eda-sym-pin-names`, `eda-sym-pin-numbers`, `eda-sym-ok`),
//! `eda-pad-dialog` (`eda-pad-number`, `eda-pad-type`, `eda-pad-shape`, `eda-pad-x`, `eda-pad-y`,
//! `eda-pad-w`, `eda-pad-h`, `eda-pad-drill`, `eda-pad-ok`), `eda-fp-dialog` (`eda-fp-name`,
//! `eda-fp-value`, `eda-fp-type`, `eda-fp-ok`), `eda-rect-dialog` (`eda-rect-x0` … `eda-rect-y1`,
//! `eda-rect-width`, `eda-rect-ok`), `eda-model-dialog` (`eda-model-path`, `eda-model-offset-x`…,
//! `eda-model-rot-z`, `eda-model-scale`, `eda-model-opacity`, `eda-model-ok`).

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_eda::expr::{Unit, eval};
use cadrs_eda::footprint::{Drill, MountKind, PadKind, PadShape};
use cadrs_eda::graphics::Geom;
use cadrs_eda::layer::{Layer, LayerSet};
use cadrs_eda::lib_edit::{Orientation, PinProps};
use cadrs_eda::symbol::{PinType, fields};
use cadrs_eda::units::{MIL, Nm, Pt, Size, to_mm};
use cadrs_ui::checkbox::CheckboxState;
use cadrs_ui::dialog_fields::{Select, SelectState};
use cadrs_ui::prelude::*;
use cadrs_ui::{Checkbox, Dialog};

use super::part_tools::{commit, current};
use super::ui;
use crate::AppState;

#[derive(Component)]
struct PartDialog;

/// Which pin, pad or shape a dialog edits.
#[derive(Resource, Default, Clone, Copy)]
struct Editing(Option<usize>);

fn close_all(w: &mut World) {
    let mut q = w.query_filtered::<Entity, With<PartDialog>>();
    let es: Vec<Entity> = q.iter(w).collect();
    for e in es {
        w.entity_mut(e).despawn();
    }
}

fn spawn_dialog(w: &mut World, d: Dialog) {
    let theme = w.resource::<Theme>().clone();
    w.spawn((d.build(&theme), PartDialog, DespawnOnExit(AppState::Document)));
}

fn row(p: &mut ChildSpawner, t: &Theme, label: &str, f: impl FnOnce(&mut ChildSpawner)) {
    p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(10.0), height: Val::Px(32.0), ..default() }).with_children(|r| {
        r.spawn((t.text(label, t.font_base, FontWeight::MEDIUM, t.muted_foreground), Node { width: Val::Px(150.0), ..default() }));
        f(r);
    });
}

fn text_row(p: &mut ChildSpawner, t: &Theme, label: &str, name: &'static str, value: String, width: f32) {
    row(p, t, label, |r| {
        r.spawn(TextInput::new(name).value(value).width(Val::Px(width)).height(26.0).build(t));
    });
}

fn select_row(p: &mut ChildSpawner, t: &Theme, label: &str, name: &'static str, options: &[&str], selected: usize) {
    row(p, t, label, |r| {
        let mut s = Select::new(name).bordered().width(Val::Px(200.0));
        for o in options {
            s = s.option(*o, true);
        }
        r.spawn(s.selected(selected).build(t));
    });
}

fn ok_cancel(f: &mut ChildSpawner, t: &Theme, ok: &'static str, run: fn(&mut World)) {
    f.spawn((
        cadrs_ui::Button::new(ok).label("OK").primary().build(t),
        observe(move |_: On<Activate>, mut commands: Commands| {
            commands.queue(run);
        }),
    ));
    f.spawn((
        cadrs_ui::Button::new(format!("{ok}-cancel")).label("Cancel").build(t),
        observe(|_: On<Activate>, mut commands: Commands| {
            commands.queue(close_all);
        }),
    ));
}

fn select_index(w: &mut World, name: &str) -> usize {
    let mut q = w.query::<(&Name, &SelectState)>();
    q.iter(w).find(|(n, _)| n.as_str() == name).map_or(0, |(_, s)| s.selected)
}

fn checkbox(w: &mut World, name: &str) -> bool {
    let mut q = w.query::<(&Name, &CheckboxState)>();
    q.iter(w).find(|(n, _)| n.as_str() == name).is_some_and(|(_, s)| s.checked)
}

fn length(w: &mut World, name: &str, unit: Unit) -> Result<Nm, String> {
    let s = ui::text_value(w, name);
    eval(&s, unit).map_err(|e| format!("{s}: {e}"))
}

fn mils(v: Nm) -> String {
    format!("{}", (v as f64 / MIL as f64 * 1000.0).round() / 1000.0)
}

fn mm_text(v: Nm) -> String {
    format!("{}", to_mm(v))
}

// ---------------------------------------------------------------------------------------------
// Pins (GS23)

const PIN_TYPES: [(&str, PinType); 12] = [
    ("Input", PinType::Input),
    ("Output", PinType::Output),
    ("Bidirectional", PinType::Bidirectional),
    ("Tri-state", PinType::TriState),
    ("Passive", PinType::Passive),
    ("Free", PinType::Free),
    ("Unspecified", PinType::Unspecified),
    ("Power input", PinType::PowerIn),
    ("Power output", PinType::PowerOut),
    ("Open collector", PinType::OpenCollector),
    ("Open emitter", PinType::OpenEmitter),
    ("Unconnected", PinType::NoConnect),
];
const ORIENTATIONS: [(&str, Orientation); 4] = [("Right", Orientation::Right), ("Left", Orientation::Left), ("Up", Orientation::Up), ("Down", Orientation::Down)];

/// Pin properties: a new pin (`None`) or pin `i`.
pub fn open_pin(w: &mut World, i: Option<usize>) {
    let Some((_, _, c)) = current(w) else { return };
    let s = c.symbol.unwrap();
    let existing = i.and_then(|i| s.pins.get(i).cloned());
    w.insert_resource(Editing(i));
    let props = match &existing {
        Some(p) => {
            let o = ORIENTATIONS.iter().find(|(_, o)| o.angle() == p.angle).map_or(Orientation::Right, |x| x.1);
            PinProps { name: p.name.clone(), number: p.number.clone(), kind: p.kind, shape: p.shape, at: p.at, orientation: o, length: p.length, name_size: p.name_size, number_size: p.number_size, visible: p.visible }
        }
        None => {
            let n = s.pins.len() + 1;
            PinProps::new("~", &n.to_string(), Pt::ZERO, Orientation::Right)
        }
    };
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-pin-dialog")
            .title("Pin properties")
            .width(460.0)
            .body(move |p| {
                text_row(p, &t, "Pin name", "eda-pin-name", props.name.clone(), 160.0);
                text_row(p, &t, "Pin number", "eda-pin-number", props.number.clone(), 160.0);
                let ti = PIN_TYPES.iter().position(|(_, k)| *k == props.kind).unwrap_or(4);
                select_row(p, &t, "Electrical type", "eda-pin-type", &PIN_TYPES.map(|x| x.0), ti);
                let oi = ORIENTATIONS.iter().position(|(_, o)| *o == props.orientation).unwrap_or(0);
                select_row(p, &t, "Orientation", "eda-pin-orientation", &ORIENTATIONS.map(|x| x.0), oi);
                text_row(p, &t, "X position (mils)", "eda-pin-x", mils(props.at.x), 120.0);
                text_row(p, &t, "Y position (mils)", "eda-pin-y", mils(props.at.y), 120.0);
                text_row(p, &t, "Pin length (mils)", "eda-pin-length", mils(props.length), 120.0);
            })
            .footer(move |f| ok_cancel(f, &tf, "eda-pin-ok", accept_pin)),
    );
}

fn accept_pin(w: &mut World) {
    let i = w.resource::<Editing>().0;
    let name = ui::text_value(w, "eda-pin-name");
    let number = ui::text_value(w, "eda-pin-number");
    let kind = PIN_TYPES[select_index(w, "eda-pin-type").min(11)].1;
    let orientation = ORIENTATIONS[select_index(w, "eda-pin-orientation").min(3)].1;
    let (x, y, len) = (length(w, "eda-pin-x", Unit::Mil), length(w, "eda-pin-y", Unit::Mil), length(w, "eda-pin-length", Unit::Mil));
    close_all(w);
    commit(w, if i.is_some() { "Edit pin" } else { "Add pin" }, |c| {
        let mut p = PinProps::new(&name, &number, Pt::new(x?, y?), orientation);
        p.kind = kind;
        p.length = len?;
        let s = c.symbol.as_mut().unwrap();
        match i {
            Some(i) if i < s.pins.len() => cadrs_eda::lib_edit::set_pin(s, i, &p),
            _ => {
                cadrs_eda::lib_edit::add_pin(s, &p);
            }
        }
        let off = cadrs_eda::lib_edit::off_grid_pins(s);
        if !off.is_empty() {
            return Err(format!("Pin {} is off the 50 mil grid: it couldn't connect to wires", off.join(", ")));
        }
        Ok(())
    });
}

// ---------------------------------------------------------------------------------------------
// Symbol properties (GS23, GS25)

pub fn open_symbol_props(w: &mut World) {
    let Some((_, _, c)) = current(w) else { return };
    let s = c.symbol.unwrap();
    let get = |n: &str| s.field(n).map_or(String::new(), |f| f.value().to_string());
    let (name, reference, value, footprint) = (s.name().to_string(), cadrs_eda::sch_edit::prefix(&get(fields::REFERENCE)).to_string(), get(fields::VALUE), get(fields::FOOTPRINT));
    let (keywords, names, numbers) = (s.keywords.clone(), s.show_pin_names, s.show_pin_numbers);
    let lib = ui::project_library_name(&ui::studio_name(w));
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-sym-dialog")
            .title("Symbol properties")
            .width(560.0)
            .body(move |p| {
                text_row(p, &t, "Symbol name", "eda-sym-name", name, 320.0);
                text_row(p, &t, "Reference prefix", "eda-sym-ref", reference, 120.0);
                text_row(p, &t, "Value", "eda-sym-value", value, 320.0);
                text_row(p, &t, "Footprint", "eda-sym-footprint", footprint, 320.0);
                p.spawn(t.text(format!("A footprint of this studio: {lib}:<name>. Empty: the component's own footprint."), t.font_sm, FontWeight::NORMAL, t.muted_foreground));
                text_row(p, &t, "Keywords", "eda-sym-keywords", keywords, 320.0);
                p.spawn(Checkbox::new("eda-sym-pin-numbers").label("Show pin numbers").checked(numbers).build(&t));
                p.spawn(Checkbox::new("eda-sym-pin-names").label("Show pin names").checked(names).build(&t));
            })
            .footer(move |f| ok_cancel(f, &tf, "eda-sym-ok", accept_symbol_props)),
    );
}

fn accept_symbol_props(w: &mut World) {
    let v: Vec<String> = ["eda-sym-name", "eda-sym-ref", "eda-sym-value", "eda-sym-footprint", "eda-sym-keywords"].iter().map(|n| ui::text_value(w, n)).collect();
    let (numbers, names) = (checkbox(w, "eda-sym-pin-numbers"), checkbox(w, "eda-sym-pin-names"));
    close_all(w);
    commit(w, "Symbol properties", |c| {
        let s = c.symbol.as_mut().unwrap();
        if !v[0].trim().is_empty() {
            s.id = v[0].trim().to_string();
        }
        cadrs_eda::lib_edit::set_symbol_field(s, fields::REFERENCE, &format!("{}?", v[1].trim()));
        cadrs_eda::lib_edit::set_symbol_field(s, fields::VALUE, v[2].trim());
        cadrs_eda::lib_edit::set_symbol_field(s, fields::FOOTPRINT, v[3].trim());
        s.keywords = v[4].trim().to_string();
        s.show_pin_numbers = numbers;
        s.show_pin_names = names;
        Ok(())
    });
}

// ---------------------------------------------------------------------------------------------
// Pads (GS24)

const PAD_KINDS: [(&str, PadKind); 3] = [("Through-hole", PadKind::ThroughHole), ("SMD", PadKind::Smd), ("NPTH, mechanical", PadKind::NonPlated)];
const PAD_SHAPES: [&str; 4] = ["Circular", "Rectangular", "Oval", "Rounded rectangle"];

pub fn open_pad(w: &mut World, i: usize) {
    let Some((_, _, c)) = current(w) else { return };
    let Some(pad) = c.footprint.unwrap().pads.get(i).cloned() else { return };
    w.insert_resource(Editing(Some(i)));
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-pad-dialog")
            .title("Pad properties")
            .width(460.0)
            .body(move |p| {
                text_row(p, &t, "Pad number", "eda-pad-number", pad.number.clone(), 120.0);
                select_row(p, &t, "Pad type", "eda-pad-type", &PAD_KINDS.map(|x| x.0), PAD_KINDS.iter().position(|(_, k)| *k == pad.kind).unwrap_or(0));
                let si = match pad.shape {
                    PadShape::Circle => 0,
                    PadShape::Rect => 1,
                    PadShape::Oval => 2,
                    _ => 3,
                };
                select_row(p, &t, "Pad shape", "eda-pad-shape", &PAD_SHAPES, si);
                text_row(p, &t, "Position X (mm)", "eda-pad-x", mm_text(pad.at.x), 120.0);
                text_row(p, &t, "Position Y (mm)", "eda-pad-y", mm_text(pad.at.y), 120.0);
                text_row(p, &t, "Size X (mm)", "eda-pad-w", mm_text(pad.size.w), 120.0);
                text_row(p, &t, "Size Y (mm)", "eda-pad-h", mm_text(pad.size.h), 120.0);
                text_row(p, &t, "Hole diameter (mm)", "eda-pad-drill", pad.drill.map_or(String::new(), |d| mm_text(d.size.w)), 120.0);
            })
            .footer(move |f| ok_cancel(f, &tf, "eda-pad-ok", accept_pad)),
    );
}

fn accept_pad(w: &mut World) {
    let Some(i) = w.resource::<Editing>().0 else { return };
    let number = ui::text_value(w, "eda-pad-number");
    let kind = PAD_KINDS[select_index(w, "eda-pad-type").min(2)].1;
    let shape = match select_index(w, "eda-pad-shape") {
        0 => PadShape::Circle,
        1 => PadShape::Rect,
        2 => PadShape::Oval,
        _ => PadShape::RoundRect { ratio: 0.25 },
    };
    let vals: Vec<Result<Nm, String>> = ["eda-pad-x", "eda-pad-y", "eda-pad-w", "eda-pad-h"].iter().map(|n| length(w, n, Unit::Mm)).collect();
    let drill_text = ui::text_value(w, "eda-pad-drill");
    let drill = if drill_text.trim().is_empty() { Ok(None) } else { eval(&drill_text, Unit::Mm).map(Some) };
    close_all(w);
    commit(w, "Pad properties", |c| {
        let f = c.footprint.as_mut().unwrap();
        let p = f.pads.get_mut(i).ok_or("The pad is gone")?;
        let v: Vec<Nm> = vals.into_iter().collect::<Result<_, _>>()?;
        p.number = number;
        p.kind = kind;
        p.shape = shape;
        p.at = Pt::new(v[0], v[1]);
        p.size = Size::new(v[2], v[3]);
        let d = drill?;
        p.drill = if kind == PadKind::Smd { None } else { d.map(|d| Drill { size: Size::new(d, d), offset: Pt::ZERO }) };
        p.layers = match kind {
            PadKind::Smd => LayerSet::of(&[Layer::TopCopper, Layer::TopMask, Layer::TopPaste]),
            PadKind::NonPlated => LayerSet::of(&[Layer::TopMask, Layer::BottomMask]),
            _ => LayerSet::ALL_COPPER.union(LayerSet::of(&[Layer::TopMask, Layer::BottomMask])),
        };
        Ok(())
    });
}

// ---------------------------------------------------------------------------------------------
// Footprint properties (GS24)

const MOUNTS: [(&str, MountKind); 3] = [("Through hole", MountKind::ThroughHole), ("SMD", MountKind::Smd), ("Unspecified", MountKind::Unspecified)];

pub fn open_footprint_props(w: &mut World) {
    let Some((_, _, c)) = current(w) else { return };
    let f = c.footprint.unwrap();
    let (name, value, mount) = (f.name().to_string(), f.field(fields::VALUE).map_or(String::new(), |v| v.text.text.text.clone()), f.attrs.mount);
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-fp-dialog")
            .title("Footprint properties")
            .width(560.0)
            .body(move |p| {
                text_row(p, &t, "Footprint name", "eda-fp-name", name, 340.0);
                text_row(p, &t, "Value", "eda-fp-value", value, 340.0);
                select_row(p, &t, "Component type", "eda-fp-type", &MOUNTS.map(|x| x.0), MOUNTS.iter().position(|(_, m)| *m == mount).unwrap_or(0));
            })
            .footer(move |f| ok_cancel(f, &tf, "eda-fp-ok", accept_footprint_props)),
    );
}

fn accept_footprint_props(w: &mut World) {
    let name = ui::text_value(w, "eda-fp-name");
    let value = ui::text_value(w, "eda-fp-value");
    let mount = MOUNTS[select_index(w, "eda-fp-type").min(2)].1;
    close_all(w);
    commit(w, "Footprint properties", |c| {
        let f = c.footprint.as_mut().unwrap();
        if !name.trim().is_empty() {
            f.id = name.trim().to_string();
        }
        if let Some(v) = f.field_mut(fields::VALUE) {
            v.text.text.text = value.trim().to_string();
        }
        f.attrs.mount = mount;
        Ok(())
    });
}

// ---------------------------------------------------------------------------------------------
// A rectangle's corners (GS24's courtyard)

pub fn open_rect(w: &mut World, i: usize) {
    let Some((_, _, c)) = current(w) else { return };
    let Some(s) = c.footprint.unwrap().shapes.get(i).cloned() else { return };
    let Geom::Rect { a, b } = s.shape.geom else { return };
    w.insert_resource(Editing(Some(i)));
    let width = s.shape.stroke.width;
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-rect-dialog")
            .title("Rectangle properties")
            .width(420.0)
            .body(move |p| {
                text_row(p, &t, "Start X (mm)", "eda-rect-x0", mm_text(a.x), 120.0);
                text_row(p, &t, "Start Y (mm)", "eda-rect-y0", mm_text(a.y), 120.0);
                text_row(p, &t, "End X (mm)", "eda-rect-x1", mm_text(b.x), 120.0);
                text_row(p, &t, "End Y (mm)", "eda-rect-y1", mm_text(b.y), 120.0);
                text_row(p, &t, "Line width (mm)", "eda-rect-width", mm_text(width), 120.0);
            })
            .footer(move |f| ok_cancel(f, &tf, "eda-rect-ok", accept_rect)),
    );
}

fn accept_rect(w: &mut World) {
    let Some(i) = w.resource::<Editing>().0 else { return };
    let v: Vec<Result<Nm, String>> = ["eda-rect-x0", "eda-rect-y0", "eda-rect-x1", "eda-rect-y1", "eda-rect-width"].iter().map(|n| length(w, n, Unit::Mm)).collect();
    close_all(w);
    commit(w, "Rectangle properties", |c| {
        let v: Vec<Nm> = v.into_iter().collect::<Result<_, _>>()?;
        let s = c.footprint.as_mut().unwrap().shapes.get_mut(i).ok_or("The shape is gone")?;
        s.shape.geom = Geom::Rect { a: Pt::new(v[0], v[1]), b: Pt::new(v[2], v[3]) };
        s.shape.stroke.width = v[4];
        Ok(())
    });
}

// ---------------------------------------------------------------------------------------------
// 3D model (GS26)

pub fn open_model(w: &mut World) {
    let Some((_, _, c)) = current(w) else { return };
    let f = c.footprint.unwrap();
    let m = f.models.first().cloned().unwrap_or_else(|| cadrs_eda::footprint::Model3d::file(&format!("{}.step", f.name())));
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-model-dialog")
            .title("3D model")
            .width(560.0)
            .body(move |p| {
                text_row(p, &t, "Model file", "eda-model-path", m.source.clone(), 340.0);
                let tb = t.clone();
                row(p, &t, "", move |r| {
                    r.spawn((
                        cadrs_ui::Button::new("eda-model-browse").label("Browse…").build(&tb),
                        observe(|_: On<Activate>, mut commands: Commands| {
                            commands.queue(browse_model);
                        }),
                    ));
                });
                for (k, axis) in ["x", "y", "z"].iter().enumerate() {
                    let name: &'static str = ["eda-model-offset-x", "eda-model-offset-y", "eda-model-offset-z"][k];
                    text_row(p, &t, &format!("Offset {axis} (mm)"), name, format!("{}", m.offset[k]), 100.0);
                }
                for (k, axis) in ["x", "y", "z"].iter().enumerate() {
                    let name: &'static str = ["eda-model-rot-x", "eda-model-rot-y", "eda-model-rot-z"][k];
                    text_row(p, &t, &format!("Rotation {axis} (°)"), name, format!("{}", m.rotation[k]), 100.0);
                }
                text_row(p, &t, "Scale", "eda-model-scale", format!("{}", m.scale[0]), 100.0);
                text_row(p, &t, "Opacity", "eda-model-opacity", format!("{}", m.opacity), 100.0);
            })
            .footer(move |f| ok_cancel(f, &tf, "eda-model-ok", accept_model)),
    );
}

fn accept_model(w: &mut World) {
    let path = ui::text_value(w, "eda-model-path");
    let num = |w: &mut World, n: &str| ui::text_value(w, n).trim().parse::<f64>().map_err(|_| format!("{n}: not a number"));
    let off = [num(w, "eda-model-offset-x"), num(w, "eda-model-offset-y"), num(w, "eda-model-offset-z")];
    let rot = [num(w, "eda-model-rot-x"), num(w, "eda-model-rot-y"), num(w, "eda-model-rot-z")];
    let (scale, opacity) = (num(w, "eda-model-scale"), num(w, "eda-model-opacity"));
    close_all(w);
    commit(w, "3D model", |c| {
        let [ox, oy, oz] = off;
        let [rx, ry, rz] = rot;
        let s = scale?;
        cadrs_eda::lib_edit::set_model(c.footprint.as_mut().unwrap(), path.trim(), [ox?, oy?, oz?], [rx?, ry?, rz?], [s, s, s], opacity?.clamp(0.0, 1.0));
        Ok(())
    });
}

/// Browse… in the 3D model dialog: a STEP or VRML file.
fn browse_model(w: &mut World) {
    let theme = w.resource::<Theme>().clone();
    let dir = std::env::current_dir().unwrap_or_default();
    let mut c = w.commands();
    cadrs_ui::file_picker::open_file_picker(&mut c, &theme, "eda-model-picker", "Choose a 3D model", "eda-model-file", dir, &["step", "stp", "wrl"]);
    w.flush();
}

/// A picked model file is stored with the document (a blob) and linked to the footprint's
/// model at once; the dialog's offset and rotation still apply on OK.
fn on_model_picked(mut msgs: MessageReader<cadrs_ui::file_picker::FilePicked>, mut commands: Commands) {
    for m in msgs.read() {
        if m.tag != "eda-model-file" {
            continue;
        }
        let path = m.path.clone();
        commands.queue(move |w: &mut World| {
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(e) => {
                    ui::toast(w, &format!("Couldn't read {}: {e}", path.display()));
                    return;
                }
            };
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let hash = cadrs_core::blobs::insert(bytes);
            let linked = commit(w, "3D model file", |c| {
                let f = c.footprint.as_mut().unwrap();
                if f.models.is_empty() {
                    f.models.push(cadrs_eda::footprint::Model3d::file(&name));
                }
                let m = &mut f.models[0];
                m.source = name.clone();
                m.blob = Some(hash);
                Ok(())
            });
            if linked {
                ui::set_text_value(w, "eda-model-path", &name);
            }
        });
    }
}

pub fn register(app: &mut App) {
    app.add_systems(Update, on_model_picked.run_if(in_state(AppState::Document)));
}
