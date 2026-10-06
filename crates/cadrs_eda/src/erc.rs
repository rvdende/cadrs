//! Electrical rules check (GS11): connection problems a schematic can show without knowing
//! what the circuit does.
//!
//! - Power input pin not driven: a net with a power input (a power port, a chip's supply pin)
//!   and no power output (a regulator, a PWR_FLAG).
//! - Pin not connected: a part's pin alone on its net with no no-connect flag.
//! - Conflicting outputs: two outputs (or power outputs) driving one net.
//! - Unannotated / duplicate references.
//! - Label not connected: a label on nothing but itself.
//! - Dangling wire end: a wire end touching nothing.

use crate::connectivity::{Netlist, netlist};
use crate::schematic::Schematic;
use crate::symbol::PinType;
use crate::units::Pt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rule {
    PowerPinNotDriven,
    PinNotConnected,
    ConflictingOutputs,
    Unannotated,
    DuplicateReference,
    LabelNotConnected,
    DanglingWire,
}

impl Rule {
    pub fn severity(self) -> Severity {
        match self {
            Rule::LabelNotConnected | Rule::DanglingWire => Severity::Warning,
            _ => Severity::Error,
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Rule::PowerPinNotDriven => "Input Power pin not driven by any Output Power pins",
            Rule::PinNotConnected => "Pin not connected",
            Rule::ConflictingOutputs => "Pins of type Output and Output are connected",
            Rule::Unannotated => "Symbol is not annotated",
            Rule::DuplicateReference => "Duplicate reference designator",
            Rule::LabelNotConnected => "Label not connected to anything",
            Rule::DanglingWire => "Wire end not connected",
        }
    }
}

/// One violation: the rule, the items it is about ("Symbol #PWR01 Pin 1 [Power input,
/// Line]") and where to point.
#[derive(Clone, Debug, PartialEq)]
pub struct Violation {
    pub rule: Rule,
    pub items: Vec<String>,
    pub at: Pt,
    pub sheet: usize,
}

impl Violation {
    pub fn severity(&self) -> Severity {
        self.rule.severity()
    }
}

fn type_name(k: PinType) -> &'static str {
    match k {
        PinType::Input => "Input",
        PinType::Output => "Output",
        PinType::Bidirectional => "Bidirectional",
        PinType::TriState => "Tri-state",
        PinType::Passive => "Passive",
        PinType::Free => "Free",
        PinType::Unspecified => "Unspecified",
        PinType::PowerIn => "Power input",
        PinType::PowerOut => "Power output",
        PinType::OpenCollector => "Open collector",
        PinType::OpenEmitter => "Open emitter",
        PinType::NoConnect => "Unconnected",
    }
}

/// Runs every rule.
pub fn check(sch: &Schematic) -> Vec<Violation> {
    let nl = netlist(sch);
    check_with(sch, &nl)
}

pub fn check_with(sch: &Schematic, nl: &Netlist) -> Vec<Violation> {
    let mut out = vec![];
    for net in &nl.nets {
        let drivers = net.pins.iter().filter(|p| p.kind == PinType::PowerOut).count();
        if drivers == 0 {
            // One violation per net, at its first power input.
            if let Some(p) = net.pins.iter().find(|p| p.kind == PinType::PowerIn) {
                out.push(Violation {
                    rule: Rule::PowerPinNotDriven,
                    items: vec![format!("Symbol {} Pin {} [{}, Line]", p.reference, p.number, type_name(p.kind))],
                    at: p.at,
                    sheet: p.sheet,
                });
            }
        }
        let outputs: Vec<_> = net.pins.iter().filter(|p| matches!(p.kind, PinType::Output | PinType::PowerOut) && !p.power).collect();
        if outputs.len() > 1 {
            out.push(Violation {
                rule: Rule::ConflictingOutputs,
                items: outputs.iter().map(|p| format!("Symbol {} Pin {} [{}]", p.reference, p.number, type_name(p.kind))).collect(),
                at: outputs[1].at,
                sheet: outputs[1].sheet,
            });
        }
        if net.pins.len() == 1 && net.labels.is_empty() && !net.no_connect {
            let p = &net.pins[0];
            if p.kind != PinType::NoConnect {
                out.push(Violation {
                    rule: Rule::PinNotConnected,
                    items: vec![format!("Symbol {} Pin {} [{}, Line]", p.reference, p.number, type_name(p.kind))],
                    at: p.at,
                    sheet: p.sheet,
                });
            }
        }
        if net.pins.is_empty() && net.labels.len() == 1 {
            out.push(Violation { rule: Rule::LabelNotConnected, items: vec![format!("Label '{}'", net.labels[0])], at: Pt::ZERO, sheet: 0 });
        }
    }
    let mut seen: Vec<(String, usize)> = vec![];
    for (si, sheet) in sch.sheets.iter().enumerate() {
        for s in &sheet.symbols {
            let r = s.reference();
            if crate::sch_edit::unannotated(r) {
                out.push(Violation { rule: Rule::Unannotated, items: vec![format!("Symbol {r}")], at: s.placement.at, sheet: si });
            } else if seen.iter().any(|(x, _)| x == r) {
                out.push(Violation { rule: Rule::DuplicateReference, items: vec![format!("Symbol {r}")], at: s.placement.at, sheet: si });
            } else {
                seen.push((r.to_string(), si));
            }
        }
        // Dangling wire ends: a wire end that touches no other wire, pin, label or junction.
        let pins: Vec<Pt> = sheet.symbols.iter().flat_map(|s| sch.placed_pins(s).map(|(_, p)| p).collect::<Vec<_>>()).collect();
        for (i, w) in sheet.wires.iter().enumerate() {
            for end in [w.a, w.b] {
                let touches = pins.contains(&end)
                    || sheet.labels.iter().any(|l| l.text.at == end)
                    || sheet.wires.iter().enumerate().any(|(j, o)| j != i && crate::connectivity::on_segment(end, o.a, o.b));
                if !touches {
                    out.push(Violation { rule: Rule::DanglingWire, items: vec!["Wire".into()], at: end, sheet: si });
                }
            }
        }
    }
    out
}
