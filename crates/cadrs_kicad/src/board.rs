//! `.kicad_pcb` → [`cadrs_eda::board`].
//!
//! KiCad boards have Y down; angles are counter-clockwise as seen, so they carry over as they
//! are once Y is flipped. Inside a footprint, positions are footprint-local but pad and text
//! angles are absolute (the footprint's angle included), and a bottom-side footprint is stored
//! already mirrored with bottom layers: it is turned back into its top-side definition.

use crate::common::{YAxis, at_angle, effects, fill, pts, stroke, uuid};
use crate::sexpr::Sexp;
use crate::Error;
use cadrs_eda::board::*;
use cadrs_eda::footprint::*;
use cadrs_eda::graphics::{Geom, Shape, Text};
use cadrs_eda::layer::{Layer, LayerSet, Side};
use cadrs_eda::units::{Pt, Size, mm, normalize_deg};
use std::collections::{BTreeMap, BTreeSet};

/// KiCad's layer name → a layer.
pub fn layer(name: &str) -> Option<Layer> {
    Some(match name {
        "F.Cu" => Layer::TopCopper,
        "B.Cu" => Layer::BottomCopper,
        "F.SilkS" | "F.Silkscreen" => Layer::TopSilk,
        "B.SilkS" | "B.Silkscreen" => Layer::BottomSilk,
        "F.Mask" => Layer::TopMask,
        "B.Mask" => Layer::BottomMask,
        "F.Paste" => Layer::TopPaste,
        "B.Paste" => Layer::BottomPaste,
        "F.Adhes" | "F.Adhesive" => Layer::TopAdhesive,
        "B.Adhes" | "B.Adhesive" => Layer::BottomAdhesive,
        "F.CrtYd" | "F.Courtyard" => Layer::TopCourtyard,
        "B.CrtYd" | "B.Courtyard" => Layer::BottomCourtyard,
        "F.Fab" => Layer::TopFab,
        "B.Fab" => Layer::BottomFab,
        "Edge.Cuts" => Layer::Outline,
        "Margin" => Layer::Margin,
        "Dwgs.User" | "User.Drawings" => Layer::Drawings,
        "Cmts.User" | "User.Comments" => Layer::Comments,
        "Eco1.User" | "User.Eco1" => Layer::Eco1,
        "Eco2.User" | "User.Eco2" => Layer::Eco2,
        _ => {
            if let Some(n) = name.strip_prefix("In").and_then(|r| r.strip_suffix(".Cu")) {
                return n.parse().ok().map(Layer::Inner);
            }
            if let Some(n) = name.strip_prefix("User.") {
                return n.parse().ok().map(Layer::User);
            }
            return None;
        }
    })
}

/// A `(layers …)` list, wildcards included (`*.Cu`, `F&B.Cu`, `*.Mask`).
fn layer_set(s: &Sexp) -> LayerSet {
    let mut set = LayerSet::EMPTY;
    let Some(l) = s.find("layers") else {
        if let Some(one) = s.get("layer").and_then(layer) {
            set.insert(one);
        }
        return set;
    };
    for name in l.items().iter().skip(1).filter_map(Sexp::text) {
        match name {
            "*.Cu" => set = set.union(LayerSet::ALL_COPPER),
            "F&B.Cu" => set = set.union(LayerSet::of(&[Layer::TopCopper, Layer::BottomCopper])),
            n if n.starts_with("*.") => {
                for side in ["F", "B"] {
                    if let Some(x) = layer(&format!("{side}{}", &n[1..])) {
                        set.insert(x);
                    }
                }
            }
            n => {
                if let Some(x) = layer(n) {
                    set.insert(x);
                }
            }
        }
    }
    set
}

/// Reads a board file.
pub fn read_board(text: &str, warnings: &mut Vec<String>) -> Result<Board, Error> {
    let root = crate::sexpr::parse(text)?;
    if root.head() != "kicad_pcb" {
        return Err(Error::Format(format!("not a board: ({} …)", root.head())));
    }
    let y = YAxis::DOWN;
    let mut b = Board::default();
    if let Some(t) = root.find("general").and_then(|g| g.get_f64("thickness")) {
        b.thickness = mm(t);
    }
    if let Some(ls) = root.find("layers") {
        let copper = ls
            .children()
            .filter(|l| l.str_arg(0).is_some_and(|n| n.ends_with(".Cu")) && !matches!(l.str_arg(1), Some("user")))
            .count();
        b.copper_layers = (copper.max(2) as u8 + 1) & !1;
    }
    // Net numbers → names.
    let nets: BTreeMap<i64, String> =
        root.all("net").filter_map(|n| Some((n.f64_arg(0)? as i64, n.str_arg(1)?.to_string()))).collect();
    let net = |s: &Sexp| -> String {
        let Some(n) = s.find("net") else { return String::new() };
        match (n.str_arg(1), n.f64_arg(0)) {
            (Some(name), _) => name.to_string(),
            (None, Some(code)) => nets.get(&(code as i64)).cloned().unwrap_or_default(),
            // KiCad 10 writes the net by name only.
            (None, None) => n.str_arg(0).unwrap_or_default().to_string(),
        }
    };

    let mut skipped: BTreeMap<&str, usize> = BTreeMap::new();
    for item in root.children() {
        let layer_of = |s: &Sexp| s.get("layer").and_then(layer);
        match item.head() {
            "footprint" | "module" => b.footprints.push(read_footprint(item, &net, warnings)),
            "segment" | "arc" => {
                let (Some(a), Some(e), Some(l)) = (y.child(item, "start"), y.child(item, "end"), layer_of(item)) else {
                    continue;
                };
                b.tracks.push(Track {
                    id: uuid(item),
                    a,
                    mid: if item.head() == "arc" { y.child(item, "mid") } else { None },
                    b: e,
                    width: item.get_f64("width").map(mm).unwrap_or(mm(0.2)),
                    layer: l,
                    net: net(item),
                    locked: item.flag("locked") == Some(true),
                });
            }
            "via" => {
                let ls: Vec<Layer> = item.find("layers").map(|l| l.items().iter().skip(1).filter_map(|x| x.text().and_then(layer)).collect()).unwrap_or_default();
                b.vias.push(Via {
                    id: uuid(item),
                    at: y.child(item, "at").unwrap_or_default(),
                    diameter: item.get_f64("size").map(mm).unwrap_or(mm(0.6)),
                    drill: item.get_f64("drill").map(mm).unwrap_or(mm(0.3)),
                    kind: if item.has_atom("micro") {
                        ViaKind::Micro
                    } else if item.has_atom("blind") {
                        ViaKind::Blind
                    } else {
                        ViaKind::Through
                    },
                    from: ls.first().copied().unwrap_or(Layer::TopCopper),
                    to: ls.last().copied().unwrap_or(Layer::BottomCopper),
                    net: net(item),
                    locked: item.flag("locked") == Some(true),
                    tented: item.find("tenting").map(|t| (t.has_atom("front"), t.has_atom("back"))),
                });
            }
            "zone" => b.zones.push(read_zone(item, y, &net)),
            "gr_line" | "gr_rect" | "gr_circle" | "gr_arc" | "gr_poly" | "gr_curve" => {
                let (Some(shape), Some(l)) = (read_shape(item, y), layer_of(item)) else { continue };
                b.shapes.push(BoardShape { id: uuid(item), shape, layer: l, locked: item.flag("locked") == Some(true), net: net(item) });
            }
            "gr_text" => {
                let Some(l) = layer_of(item) else { continue };
                let (style, visible) = effects(item);
                b.texts.push(BoardText {
                    id: uuid(item),
                    text: Text { text: item.str_arg(0).unwrap_or_default().into(), at: y.child(item, "at").unwrap_or_default(), angle: at_angle(item), style, visible },
                    layer: l,
                    locked: item.flag("locked") == Some(true),
                    knockout: item.find("layer").is_some_and(|l| l.has_atom("knockout")),
                });
            }
            "group" => b.groups.push(Group {
                id: uuid(item),
                name: item.str_arg(0).unwrap_or_default().into(),
                members: item.find("members").map(|m| m.items().iter().skip(1).filter_map(|x| uuid::Uuid::parse_str(x.text()?).ok()).collect()).unwrap_or_default(),
                locked: item.flag("locked") == Some(true),
            }),
            "version" | "generator" | "generator_version" | "general" | "paper" | "title_block" | "layers" | "setup"
            | "net" | "embedded_fonts" | "property" => {}
            other => *skipped.entry(other).or_default() += 1,
        }
    }
    for (what, n) in skipped {
        warnings.push(format!("board: {n} × `{what}` not imported yet"));
    }
    let mut all: BTreeSet<String> = nets.into_values().collect();
    for f in &b.footprints {
        all.extend(f.footprint.pads.iter().filter_map(|p| p.net.clone()));
    }
    all.remove("");
    b.nets = all.into_iter().collect();
    Ok(b)
}

/// A board-level or footprint-level drawn shape (`gr_*` / `fp_*`).
fn read_shape(s: &Sexp, y: YAxis) -> Option<Shape> {
    let kind = s.head().split_once('_').map(|(_, k)| k)?;
    let geom = match kind {
        "line" => Geom::Line { a: y.child(s, "start")?, b: y.child(s, "end")? },
        "rect" => Geom::Rect { a: y.child(s, "start")?, b: y.child(s, "end")? },
        "circle" => {
            let c = y.child(s, "center")?;
            Geom::Circle { center: c, radius: c.dist(y.child(s, "end")?).round() as i64 }
        }
        "arc" => match y.child(s, "mid") {
            Some(mid) => Geom::Arc { start: y.child(s, "start")?, mid, end: y.child(s, "end")? },
            // KiCad 5: (start = centre) (end = arc start) (angle sweep, clockwise as seen).
            None => {
                let c = y.child(s, "start")?;
                let a = y.child(s, "end")?;
                let sweep = s.get_f64("angle")?;
                let r = a - c;
                Geom::Arc { start: a, mid: c + r.rotated(sweep / 2.0), end: c + r.rotated(sweep) }
            }
        },
        "poly" => {
            let p = pts(s, y);
            if p.len() < 2 {
                return None;
            }
            Geom::Polyline { pts: p, closed: true }
        }
        "curve" => {
            let p = pts(s, y);
            if p.len() != 4 {
                return None;
            }
            Geom::Bezier { pts: [p[0], p[1], p[2], p[3]] }
        }
        _ => return None,
    };
    Some(Shape { geom, stroke: stroke(s), fill: fill(s) })
}

fn read_zone(z: &Sexp, y: YAxis, net: &impl Fn(&Sexp) -> String) -> Zone {
    let keepout = z.find("keepout").map(|k| {
        let no = |name: &str| k.get(name) == Some("not_allowed");
        Keepout { tracks: no("tracks"), vias: no("vias"), pads: no("pads"), copper_pour: no("copperpour"), footprints: no("footprints") }
    });
    let mut fill = ZoneFill::default();
    if let Some(c) = z.find("connect_pads") {
        fill.pad_connection = match c.str_arg(0) {
            Some("yes") => ZoneConnection::Solid,
            Some("no") => ZoneConnection::None,
            _ => ZoneConnection::Thermal,
        };
        if let Some(v) = c.get_f64("clearance") {
            fill.clearance = mm(v);
        }
    }
    if let Some(v) = z.get_f64("min_thickness") {
        fill.min_width = mm(v);
    }
    if let Some(f) = z.find("fill") {
        if let Some(v) = f.get_f64("thermal_gap") {
            fill.thermal_gap = mm(v);
        }
        if let Some(v) = f.get_f64("thermal_bridge_width") {
            fill.thermal_spoke = mm(v);
        }
        fill.remove_islands = f.get("island_removal_mode") != Some("1");
        if f.get("mode") == Some("hatch") {
            fill.hatched = Some((f.get_f64("hatch_thickness").map(mm).unwrap_or(mm(1.0)), f.get_f64("hatch_gap").map(mm).unwrap_or(mm(1.5))));
        }
    }
    let mut filled: Vec<(Layer, Vec<Polygon>)> = vec![];
    for fp in z.all("filled_polygon") {
        let Some(l) = fp.get("layer").and_then(layer) else { continue };
        let poly = Polygon { outer: pts(fp, y), holes: vec![] };
        match filled.iter_mut().find(|(x, _)| *x == l) {
            Some((_, v)) => v.push(poly),
            None => filled.push((l, vec![poly])),
        }
    }
    Zone {
        id: uuid(z),
        name: z.get("name").unwrap_or_default().into(),
        net: z.get("net_name").map(str::to_string).unwrap_or_else(|| net(z)),
        layers: layer_set(z),
        priority: z.get_f64("priority").unwrap_or(0.0) as u32,
        outline: z.all("polygon").map(|p| pts(p, y)).collect(),
        fill,
        keepout,
        locked: z.flag("locked") == Some(true),
        filled,
    }
}

fn read_footprint(f: &Sexp, net: &impl Fn(&Sexp) -> String, warnings: &mut Vec<String>) -> PlacedFootprint {
    let board_y = YAxis::DOWN;
    let at = board_y.child(f, "at").unwrap_or_default();
    let angle = at_angle(f);
    let side = if f.get("layer") == Some("B.Cu") { Side::Bottom } else { Side::Top };
    let bottom = side == Side::Bottom;
    // Local Y down → up; a bottom footprint's stored (mirrored) coordinates mirrored back.
    let y = if bottom { YAxis::UP } else { YAxis::DOWN };
    let unflip = |l: Layer| if bottom { l.flipped() } else { l };
    // An absolute angle in the file → the top-side definition's local angle.
    let local_angle = |a: f64| normalize_deg(if bottom { angle - a } else { a - angle });

    let text_of = |s: &Sexp, value: &str| -> Option<FpText> {
        let l = unflip(s.get("layer").and_then(layer)?);
        let (mut style, visible) = effects(s);
        if bottom {
            style.mirrored = !style.mirrored;
        }
        Some(FpText {
            id: uuid(s),
            text: Text { text: value.into(), at: y.child(s, "at").unwrap_or_default(), angle: local_angle(at_angle(s)), style, visible },
            layer: l,
            keep_upright: s.find("at").is_none_or(|a| !a.has_atom("unlocked")) && s.flag("unlocked") != Some(true),
        })
    };

    let mut fp = Footprint {
        id: f.str_arg(0).unwrap_or_default().into(),
        description: f.get("descr").unwrap_or_default().into(),
        keywords: f.get("tags").unwrap_or_default().into(),
        fields: vec![],
        attrs: FootprintAttrs::default(),
        pads: vec![],
        shapes: vec![],
        texts: vec![],
        models: vec![],
        zones: vec![],
    };
    if let Some(a) = f.find("attr") {
        fp.attrs = FootprintAttrs {
            mount: if a.has_atom("smd") {
                MountKind::Smd
            } else if a.has_atom("through_hole") {
                MountKind::ThroughHole
            } else {
                MountKind::Unspecified
            },
            board_only: a.has_atom("board_only"),
            exclude_from_pos: a.has_atom("exclude_from_pos_files"),
            exclude_from_bom: a.has_atom("exclude_from_bom"),
            dnp: a.has_atom("dnp"),
            allow_missing_courtyard: a.has_atom("allow_missing_courtyard"),
        };
    } else {
        fp.attrs.mount = MountKind::Unspecified;
    }
    for c in f.children() {
        match c.head() {
            "property" => {
                let name = c.str_arg(0).unwrap_or_default();
                if let Some(t) = text_of(c, c.str_arg(1).unwrap_or_default()) {
                    fp.fields.push(FpField { name: name.into(), text: t });
                } else if !name.starts_with("ki_") {
                    // Fields without a layer (KiCad 8 "Sheetfile" …) are kept hidden on Fab.
                    let mut t = FpText { id: uuid(c), text: Text::new(c.str_arg(1).unwrap_or_default(), Pt::ZERO), layer: Layer::TopFab, keep_upright: true };
                    t.text.visible = false;
                    fp.fields.push(FpField { name: name.into(), text: t });
                }
            }
            "fp_text" => {
                let kind = c.str_arg(0).unwrap_or_default();
                let Some(t) = text_of(c, c.str_arg(1).unwrap_or_default()) else { continue };
                match kind {
                    "reference" => fp.fields.push(FpField { name: "Reference".into(), text: t }),
                    "value" => fp.fields.push(FpField { name: "Value".into(), text: t }),
                    _ => fp.texts.push(t),
                }
            }
            "fp_line" | "fp_rect" | "fp_circle" | "fp_arc" | "fp_poly" | "fp_curve" => {
                let (Some(shape), Some(l)) = (read_shape(c, y), c.get("layer").and_then(layer)) else { continue };
                fp.shapes.push(FpShape { id: uuid(c), shape, layer: unflip(l) });
            }
            "pad" => fp.pads.push(read_pad(c, y, bottom, &local_angle, net)),
            "model" => fp.models.push(Model3d {
                source: c.str_arg(0).unwrap_or_default().into(),
                blob: None,
                offset: xyz(c, "offset").unwrap_or([0.0; 3]),
                rotation: xyz(c, "rotate").unwrap_or([0.0; 3]),
                scale: xyz(c, "scale").unwrap_or([1.0; 3]),
                visible: c.flag("hide") != Some(true),
                opacity: c.get_f64("opacity").unwrap_or(1.0),
            }),
            "zone" => {
                // Footprint zones are stored in board coordinates.
                let mut z = read_zone(c, board_y, net);
                let to_local = |p: Pt| {
                    let l = (p - at).rotated(-angle);
                    if bottom { l.flip_y() } else { l }
                };
                for r in &mut z.outline {
                    r.iter_mut().for_each(|p| *p = to_local(*p));
                }
                z.layers = if bottom { z.layers.flipped() } else { z.layers };
                z.filled.clear();
                fp.zones.push(z);
            }
            "fp_text_box" | "dimension" | "group" | "image" => warnings.push(format!("footprint {}: `{}` not imported yet", fp.id, c.head())),
            _ => {}
        }
    }
    PlacedFootprint {
        id: uuid(f),
        symbol: f.get("path").and_then(|p| p.rsplit('/').next()).and_then(|u| uuid::Uuid::parse_str(u).ok()),
        footprint: fp,
        placement: FootprintPlacement { at, angle: normalize_deg(angle), side },
        locked: f.flag("locked") == Some(true),
    }
}

fn xyz(m: &Sexp, name: &str) -> Option<[f64; 3]> {
    let x = m.find(name)?.find("xyz")?;
    Some([x.f64_arg(0)?, x.f64_arg(1)?, x.f64_arg(2)?])
}

fn read_pad(p: &Sexp, y: YAxis, bottom: bool, local_angle: &impl Fn(f64) -> f64, net: &impl Fn(&Sexp) -> String) -> Pad {
    let kind = match p.str_arg(1) {
        Some("thru_hole") => PadKind::ThroughHole,
        Some("np_thru_hole") => PadKind::NonPlated,
        Some("connect") => PadKind::Connector,
        _ => PadKind::Smd,
    };
    let size = p.find("size").map(|s| Size::mm(s.f64_arg(0).unwrap_or(0.0), s.f64_arg(1).unwrap_or(0.0))).unwrap_or_default();
    let shape = match p.str_arg(2) {
        Some("rect") => match p.get_f64("chamfer_ratio") {
            // KiCad 5 chamfered rects.
            Some(r) if p.find("chamfer").is_some() => PadShape::Chamfered { ratio: r, corners: corners(p), round_ratio: 0.0 },
            _ => PadShape::Rect,
        },
        Some("oval") => PadShape::Oval,
        Some("roundrect") => match p.find("chamfer") {
            Some(_) => PadShape::Chamfered { ratio: p.get_f64("chamfer_ratio").unwrap_or(0.0), corners: corners(p), round_ratio: p.get_f64("roundrect_rratio").unwrap_or(0.0) },
            None => PadShape::RoundRect { ratio: p.get_f64("roundrect_rratio").unwrap_or(0.25) },
        },
        Some("trapezoid") => PadShape::Trapezoid {
            delta: p.find("rect_delta").map(|d| Size::mm(d.f64_arg(0).unwrap_or(0.0), d.f64_arg(1).unwrap_or(0.0))).unwrap_or_default(),
        },
        Some("custom") => PadShape::Custom {
            anchor_rect: p.find("options").and_then(|o| o.get("anchor")) == Some("rect"),
            shapes: p
                .find("primitives")
                .map(|pr| {
                    pr.children()
                        .filter_map(|c| {
                            let mut s = read_shape(c, y)?;
                            // Primitives fill unless told otherwise (old files say nothing).
                            if c.find("fill").is_none() && matches!(s.geom, Geom::Polyline { .. } | Geom::Circle { .. } | Geom::Rect { .. }) {
                                s.fill = cadrs_eda::graphics::Fill::Outline;
                            }
                            Some(s)
                        })
                        .collect()
                })
                .unwrap_or_default(),
        },
        _ => PadShape::Circle,
    };
    let drill = p.find("drill").map(|d| {
        let oval = d.has_atom("oval");
        let nums: Vec<f64> = d.items().iter().skip(1).filter_map(|x| x.text()?.parse().ok()).collect();
        let w = nums.first().copied().unwrap_or(0.0);
        let h = if oval { nums.get(1).copied().unwrap_or(w) } else { w };
        Drill { size: Size::mm(w, h), offset: d.find("offset").map(|o| y.vec(o.f64_arg(0).unwrap_or(0.0), o.f64_arg(1).unwrap_or(0.0))).unwrap_or_default() }
    });
    let mut shape = shape;
    let mut layers = layer_set(p);
    if bottom {
        layers = layers.flipped();
        unmirror_pad_shape(&mut shape);
    }
    let n = net(p);
    let rules = PadRules {
        clearance: p.get_f64("clearance").map(mm),
        mask_margin: p.get_f64("solder_mask_margin").map(mm),
        paste_margin: p.get_f64("solder_paste_margin").map(mm),
        paste_ratio: p.get_f64("solder_paste_margin_ratio"),
        zone_connection: match p.get_f64("zone_connect").map(|v| v as i64) {
            Some(0) => ZoneConnection::None,
            Some(1) => ZoneConnection::Thermal,
            Some(2) => ZoneConnection::Solid,
            _ => ZoneConnection::Inherit,
        },
        thermal_gap: p.get_f64("thermal_gap").map(mm),
        thermal_spoke: p.get_f64("thermal_bridge_width").or_else(|| p.get_f64("thermal_width")).map(mm),
    };
    Pad {
        id: uuid(p),
        number: p.str_arg(0).unwrap_or_default().into(),
        kind,
        shape,
        at: y.child(p, "at").unwrap_or_default(),
        angle: local_angle(at_angle(p)),
        size,
        drill,
        layers,
        net: (!n.is_empty()).then_some(n),
        pin_function: p.get("pinfunction").unwrap_or_default().into(),
        pin_type: p.get("pintype").unwrap_or_default().into(),
        rules,
        die_length: p.get_f64("die_length").map(mm).unwrap_or(0),
    }
}

/// The chamfered corners as seen (Y down: KiCad's top is our top). Mirrored for a bottom
/// footprint by the caller.
fn corners(p: &Sexp) -> Corners {
    let c = p.find("chamfer");
    let has = |n: &str| c.is_some_and(|c| c.has_atom(n));
    Corners { top_left: has("top_left"), top_right: has("top_right"), bottom_left: has("bottom_left"), bottom_right: has("bottom_right") }
}

/// A bottom footprint's pad, stored mirrored: its corners and trapezoid swapped back.
fn unmirror_pad_shape(s: &mut PadShape) {
    match s {
        PadShape::Chamfered { corners: c, .. } => {
            std::mem::swap(&mut c.top_left, &mut c.bottom_left);
            std::mem::swap(&mut c.top_right, &mut c.bottom_right);
        }
        PadShape::Trapezoid { delta } => delta.h = -delta.h,
        _ => {}
    }
}
