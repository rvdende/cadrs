//! `.kicad_sch` (and the symbols of `.kicad_sym`) → [`cadrs_eda::schematic`].
//!
//! KiCad sheets have Y down from the page's top-left corner; symbol libraries have Y up. Placed
//! symbols' field positions are absolute, their angles relative to the symbol.

use crate::common::{self, YAxis, at_angle, effects, fill, pts, stroke, uuid};
use crate::sexpr::Sexp;
use crate::Error;
use cadrs_eda::graphics::{Geom, Shape, Text};
use cadrs_eda::schematic::*;
use cadrs_eda::symbol::*;
use cadrs_eda::units::{Pt, Size, mm};

/// Reads a schematic file. Sub-sheets are not followed yet (reported in `warnings`).
pub fn read_schematic(text: &str, warnings: &mut Vec<String>) -> Result<Schematic, Error> {
    let root = crate::sexpr::parse(text)?;
    if root.head() != "kicad_sch" {
        return Err(Error::Format(format!("not a schematic: ({} …)", root.head())));
    }
    let lib: Vec<&Sexp> = root.find("lib_symbols").map(|l| l.all("symbol").collect()).unwrap_or_default();
    let symbols: Vec<Symbol> = lib.iter().map(|s| read_symbol(s, &lib)).collect();

    let paper = read_paper(&root);
    let y = YAxis::page(cadrs_eda::units::to_mm(paper.size.h));
    let mut sheet = Sheet { id: uuid(&root), name: "Root".into(), paper, ..Default::default() };
    if let Some(tb) = root.find("title_block") {
        sheet.title_block = TitleBlock {
            title: tb.get("title").unwrap_or_default().into(),
            date: tb.get("date").unwrap_or_default().into(),
            revision: tb.get("rev").unwrap_or_default().into(),
            company: tb.get("company").unwrap_or_default().into(),
            comments: tb.all("comment").map(|c| c.str_arg(1).unwrap_or_default().to_string()).collect(),
        };
    }

    let mut skipped: std::collections::BTreeMap<&str, usize> = Default::default();
    for item in root.children() {
        match item.head() {
            "symbol" => sheet.symbols.push(read_placed(item, y)),
            "wire" | "bus" => {
                let p = pts(item, y);
                for w in p.windows(2) {
                    let wire = Wire { id: uuid(item), a: w[0], b: w[1], stroke: stroke(item) };
                    if item.head() == "wire" { sheet.wires.push(wire) } else { sheet.buses.push(wire) }
                }
            }
            "junction" => sheet.junctions.push(Junction {
                id: uuid(item),
                at: y.child(item, "at").unwrap_or_default(),
                diameter: item.get_f64("diameter").map(mm).unwrap_or(0),
                color: common::color(item),
            }),
            "no_connect" => sheet.no_connects.push(NoConnect { id: uuid(item), at: y.child(item, "at").unwrap_or_default() }),
            "label" | "global_label" | "hierarchical_label" => {
                let shape = match item.get("shape") {
                    Some("input") => LabelShape::Input,
                    Some("output") => LabelShape::Output,
                    Some("bidirectional") => LabelShape::Bidirectional,
                    Some("tri_state") => LabelShape::TriState,
                    _ => LabelShape::Passive,
                };
                let kind = match item.head() {
                    "global_label" => LabelKind::Global(shape),
                    "hierarchical_label" => LabelKind::Hierarchical(shape),
                    _ => LabelKind::Local,
                };
                let fields = item.all("property").map(|p| sheet_field(p, y, None)).collect();
                sheet.labels.push(Label { id: uuid(item), kind, text: sheet_text(item, y), fields });
            }
            "text" => sheet.notes.push(Note { id: uuid(item), text: sheet_text(item, y) }),
            "polyline" | "rectangle" | "circle" | "arc" | "bezier" => {
                if let Some(shape) = read_shape(item, y) {
                    sheet.drawings.push(Drawing { id: uuid(item), shape });
                }
            }
            "version" | "generator" | "generator_version" | "uuid" | "paper" | "title_block" | "lib_symbols"
            | "sheet_instances" | "symbol_instances" | "embedded_fonts" => {}
            other => *skipped.entry(other).or_default() += 1,
        }
    }
    for (what, n) in skipped {
        warnings.push(format!("schematic: {n} × `{what}` not imported yet"));
    }
    Ok(Schematic { symbols, sheets: vec![sheet] })
}

fn read_paper(root: &Sexp) -> Paper {
    let Some(p) = root.find("paper") else { return Paper::default() };
    let name = p.str_arg(0).unwrap_or("A4").to_string();
    let (w, h) = match name.as_str() {
        "A5" => (210.0, 148.0),
        "A4" => (297.0, 210.0),
        "A3" => (420.0, 297.0),
        "A2" => (594.0, 420.0),
        "A1" => (841.0, 594.0),
        "A0" => (1189.0, 841.0),
        "A" | "USLetter" => (279.4, 215.9),
        "USLegal" => (355.6, 215.9),
        "B" | "USLedger" => (431.8, 279.4),
        "C" => (558.8, 431.8),
        "D" => (863.6, 558.8),
        "E" => (1117.6, 863.6),
        _ => (p.f64_arg(1).unwrap_or(297.0), p.f64_arg(2).unwrap_or(210.0)),
    };
    let size = if p.has_atom("portrait") { Size::mm(h, w) } else { Size::mm(w, h) };
    Paper { name, size }
}

/// Text in sheet coordinates: `(text "…" (at x y angle) (effects …))`.
fn sheet_text(item: &Sexp, y: YAxis) -> Text {
    let (style, visible) = effects(item);
    Text {
        text: item.str_arg(0).unwrap_or_default().to_string(),
        at: y.child(item, "at").unwrap_or_default(),
        angle: at_angle(item),
        style,
        visible,
    }
}

/// `(property "name" "value" (at …) (effects …))`. With a placement, the angle and
/// justification are relative to the symbol (the position is absolute).
fn sheet_field(p: &Sexp, y: YAxis, placement: Option<&Placement>) -> Field {
    let (style, visible) = effects(p);
    let mut text = Text {
        text: p.str_arg(1).unwrap_or_default().to_string(),
        at: y.child(p, "at").unwrap_or_default(),
        angle: at_angle(p),
        style,
        visible,
    };
    if let Some(pl) = placement {
        let at = text.at;
        // Only the direction and the mirror's effect: the anchor is already on the sheet.
        text = Placement { at: Pt::ZERO, ..*pl }.text(&text);
        text.at = at;
    }
    Field { name: p.str_arg(0).unwrap_or_default().to_string(), text, show_name: p.flag("show_name") == Some(true) }
}

fn read_placed(item: &Sexp, y: YAxis) -> PlacedSymbol {
    let lib_id = item.get("lib_name").or_else(|| item.get("lib_id")).unwrap_or_default().to_string();
    let mirror = match item.get("mirror") {
        Some("x") => Mirror::X,
        Some("y") => Mirror::Y,
        _ => Mirror::None,
    };
    let placement = Placement { at: y.child(item, "at").unwrap_or_default(), angle: at_angle(item), mirror };
    let mut fields: Vec<Field> = item.all("property").map(|p| sheet_field(p, y, Some(&placement))).collect();
    // The reference of the root sheet's instance wins over the property (KiCad 7+).
    let inst_ref = item
        .find("instances")
        .and_then(|i| i.find("project"))
        .and_then(|p| p.find("path"))
        .and_then(|p| p.get("reference"))
        .map(str::to_string);
    if let (Some(r), Some(f)) = (inst_ref, fields.iter_mut().find(|f| f.name == fields::REFERENCE)) {
        f.text.text = r;
    }
    PlacedSymbol {
        id: uuid(item),
        symbol: lib_id,
        placement,
        unit: item.get_f64("unit").unwrap_or(1.0) as u32,
        style: item.get_f64("convert").or_else(|| item.get_f64("body_style")).unwrap_or(1.0) as u32,
        fields,
        in_bom: item.flag("in_bom") != Some(false),
        on_board: item.flag("on_board") != Some(false),
        dnp: item.flag("dnp") == Some(true),
        exclude_from_sim: item.flag("exclude_from_sim") == Some(true),
        pin_ids: item
            .all("pin")
            .filter_map(|p| Some((p.str_arg(0)?.to_string(), uuid::Uuid::parse_str(p.get("uuid")?).ok()?)))
            .collect(),
    }
}

/// A drawn shape: `polyline`, `rectangle`, `circle`, `arc`, `bezier`.
fn read_shape(s: &Sexp, y: YAxis) -> Option<Shape> {
    let geom = match s.head() {
        "polyline" => {
            let p = pts(s, y);
            if p.len() < 2 {
                return None;
            }
            let closed = p.len() > 2 && p.first() == p.last();
            let mut p = p;
            if closed {
                p.pop();
            }
            Geom::Polyline { pts: p, closed }
        }
        "rectangle" => Geom::Rect { a: y.child(s, "start")?, b: y.child(s, "end")? },
        "circle" => Geom::Circle { center: y.child(s, "center")?, radius: mm(s.get_f64("radius")?) },
        "arc" => {
            let (start, end) = (y.child(s, "start")?, y.child(s, "end")?);
            let mid = match y.child(s, "mid") {
                Some(m) => m,
                // KiCad 6 symbol arcs: (radius (at cx cy) (length r) (angles a0 a1)).
                // Counter-clockwise from start to end, as KiCad 6 drew them.
                None => {
                    let r = s.find("radius")?;
                    let c = y.child(r, "at")?;
                    let rad = r.get_f64("length")?;
                    let ang = |p: Pt| ((p - c).y as f64).atan2((p - c).x as f64);
                    let (a0, a1) = (ang(start), ang(end));
                    let ma = a0 + (a1 - a0).rem_euclid(std::f64::consts::TAU) / 2.0;
                    c + Pt::mm(rad * ma.cos(), rad * ma.sin())
                }
            };
            Geom::Arc { start, mid, end }
        }
        "bezier" => {
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

/// A library symbol, `extends` resolved against `lib`.
pub fn read_symbol(s: &Sexp, lib: &[&Sexp]) -> Symbol {
    let id = s.str_arg(0).unwrap_or_default().to_string();
    let y = YAxis::UP;
    // A derived symbol takes its parent's drawing and pins, and overrides fields.
    let parent = s.get("extends").and_then(|p| {
        let prefix = id.rsplit_once(':').map(|(l, _)| format!("{l}:"));
        lib.iter().find(|c| {
            let n = c.str_arg(0).unwrap_or_default();
            n == p || prefix.as_ref().is_some_and(|pre| n == format!("{pre}{p}"))
        })
    });
    let mut sym = match parent {
        Some(p) => read_symbol(p, lib),
        None => Symbol {
            id: id.clone(),
            fields: vec![],
            keywords: String::new(),
            footprint_filters: vec![],
            unit_count: 1,
            units_swappable: true,
            unit_names: vec![],
            has_alternate: false,
            power: false,
            show_pin_numbers: true,
            show_pin_names: true,
            pin_name_offset: mm(0.508),
            in_bom: true,
            on_board: true,
            graphics: vec![],
            pins: vec![],
        },
    };
    sym.id = id;
    if s.find("power").is_some() || s.has_atom("power") {
        sym.power = true;
    }
    if let Some(pn) = s.find("pin_numbers") {
        sym.show_pin_numbers = pn.flag("hide") != Some(true);
    }
    if let Some(pn) = s.find("pin_names") {
        sym.show_pin_names = pn.flag("hide") != Some(true);
        if let Some(o) = pn.get_f64("offset") {
            sym.pin_name_offset = mm(o);
        }
    }
    if let Some(b) = s.flag("in_bom") {
        sym.in_bom = b;
    }
    if let Some(b) = s.flag("on_board") {
        sym.on_board = b;
    }
    for p in s.all("property") {
        let name = p.str_arg(0).unwrap_or_default();
        let value = p.str_arg(1).unwrap_or_default();
        match name {
            "ki_keywords" => sym.keywords = value.into(),
            "ki_fp_filters" => sym.footprint_filters = value.split_whitespace().map(str::to_string).collect(),
            "ki_locked" => sym.units_swappable = false,
            _ => {
                let (style, visible) = effects(p);
                let field = Field {
                    name: name.into(),
                    text: Text { text: value.into(), at: y.child(p, "at").unwrap_or_default(), angle: at_angle(p), style, visible },
                    show_name: p.flag("show_name") == Some(true),
                };
                match sym.fields.iter_mut().find(|f| f.name == name) {
                    Some(f) => *f = field,
                    None => sym.fields.push(field),
                }
            }
        }
    }
    let mut max_unit = sym.unit_count;
    for sub in s.all("symbol") {
        let name = sub.str_arg(0).unwrap_or_default();
        let mut parts = name.rsplitn(3, '_');
        let style: u32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        let unit: u32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        max_unit = max_unit.max(unit);
        if style == 2 {
            sym.has_alternate = true;
        }
        if let Some(n) = sub.get("unit_name") {
            sym.unit_names.push((unit, n.into()));
        }
        for item in sub.children() {
            match item.head() {
                "pin" => sym.pins.push(read_pin(item, unit, style)),
                "text" => {
                    let (st, visible) = effects(item);
                    let mut angle = at_angle(item);
                    // Old libraries wrote text angles in tenths of a degree.
                    if angle.abs() > 360.0 {
                        angle /= 10.0;
                    }
                    let text = Text { text: item.str_arg(0).unwrap_or_default().into(), at: y.child(item, "at").unwrap_or_default(), angle, style: st, visible };
                    sym.graphics.push(SymbolGraphic { item: SymbolItem::Text(text), unit, style });
                }
                _ => {
                    if let Some(shape) = read_shape(item, y) {
                        sym.graphics.push(SymbolGraphic { item: SymbolItem::Shape(shape), unit, style });
                    }
                }
            }
        }
    }
    sym.unit_count = max_unit.max(1);
    sym
}

fn read_pin(p: &Sexp, unit: u32, style: u32) -> Pin {
    let kind = match p.str_arg(0).unwrap_or_default() {
        "input" => PinType::Input,
        "output" => PinType::Output,
        "bidirectional" => PinType::Bidirectional,
        "tri_state" => PinType::TriState,
        "free" => PinType::Free,
        "unspecified" => PinType::Unspecified,
        "power_in" => PinType::PowerIn,
        "power_out" => PinType::PowerOut,
        "open_collector" => PinType::OpenCollector,
        "open_emitter" => PinType::OpenEmitter,
        "no_connect" => PinType::NoConnect,
        _ => PinType::Passive,
    };
    let shape = match p.str_arg(1).unwrap_or_default() {
        "inverted" => PinShape::Inverted,
        "clock" => PinShape::Clock,
        "inverted_clock" => PinShape::InvertedClock,
        "input_low" => PinShape::InputLow,
        "clock_low" => PinShape::ClockLow,
        "output_low" => PinShape::OutputLow,
        "edge_clock_high" => PinShape::EdgeClockHigh,
        "non_logic" => PinShape::NonLogic,
        _ => PinShape::Line,
    };
    let text_size = |name: &str| p.find(name).map(|n| effects(n).0.size.h).unwrap_or(mm(1.27));
    Pin {
        number: p.find("number").and_then(|n| n.str_arg(0)).unwrap_or_default().into(),
        name: p.find("name").and_then(|n| n.str_arg(0)).unwrap_or_default().into(),
        kind,
        shape,
        at: YAxis::UP.child(p, "at").unwrap_or_default(),
        angle: at_angle(p),
        length: p.get_f64("length").map(mm).unwrap_or(mm(2.54)),
        visible: p.flag("hide") != Some(true),
        name_size: text_size("name"),
        number_size: text_size("number"),
        unit,
        style,
    }
}
