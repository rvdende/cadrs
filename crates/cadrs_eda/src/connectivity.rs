//! Schematic connectivity: which pins are joined, as named nets (GS7).
//!
//! Joined: wire ends to wires they touch (end or middle), wires crossing at a junction, pins
//! and labels on wires or on each other. Then nets merge by name: local labels on one sheet,
//! global labels and power ports (by their value) everywhere. Net names: a power port's value,
//! else a global label, else a local label, else `Net-(R1-Pad2)` / `Net-(U1-ANT)` from the
//! first pin by reference.

use crate::schematic::{LabelKind, Schematic};
use crate::symbol::PinType;
use crate::units::Pt;
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

/// A pin on a net.
#[derive(Clone, Debug, PartialEq)]
pub struct NetPin {
    pub symbol: Uuid,
    pub sheet: usize,
    pub reference: String,
    pub number: String,
    pub name: String,
    pub kind: PinType,
    pub at: Pt,
    /// Of a power port or flag.
    pub power: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Net {
    pub name: String,
    pub pins: Vec<NetPin>,
    /// Label texts on it.
    pub labels: Vec<String>,
    /// Has a no-connect flag.
    pub no_connect: bool,
}

impl Net {
    /// The pins of real parts (not power ports or flags).
    pub fn part_pins(&self) -> impl Iterator<Item = &NetPin> {
        self.pins.iter().filter(|p| !p.power)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Netlist {
    pub nets: Vec<Net>,
}

impl Netlist {
    pub fn net(&self, name: &str) -> Option<&Net> {
        self.nets.iter().find(|n| n.name == name)
    }

    /// The net a pin is on: (reference, pin number) → net name.
    pub fn net_of(&self, reference: &str, pin: &str) -> Option<&str> {
        self.nets.iter().find(|n| n.pins.iter().any(|p| p.reference == reference && p.number == pin)).map(|n| n.name.as_str())
    }
}

struct Dsu(Vec<usize>);

impl Dsu {
    fn find(&mut self, i: usize) -> usize {
        let mut r = i;
        while self.0[r] != r {
            r = self.0[r];
        }
        let mut i = i;
        while self.0[i] != r {
            let n = self.0[i];
            self.0[i] = r;
            i = n;
        }
        r
    }
    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a] = b;
        }
    }
}

/// Whether `p` lies on the segment `a`–`b` exactly (schematic points are on a grid).
pub fn on_segment(p: Pt, a: Pt, b: Pt) -> bool {
    let (ab, ap) = (b - a, p - a);
    let cross = ab.x as i128 * ap.y as i128 - ab.y as i128 * ap.x as i128;
    if cross != 0 {
        return false;
    }
    let dot = ap.x as i128 * ab.x as i128 + ap.y as i128 * ab.y as i128;
    dot >= 0 && dot <= ab.x as i128 * ab.x as i128 + ab.y as i128 * ab.y as i128
}

enum Node {
    Pin(NetPin),
    Label { text: String, global: bool, sheet: usize },
    PowerName(String),
    NoConnect,
    Wire,
}

/// The schematic's nets.
pub fn netlist(sch: &Schematic) -> Netlist {
    let mut nodes: Vec<Node> = vec![];
    let mut dsu = Dsu(vec![]);
    let mut add = |n: Node, dsu: &mut Dsu| {
        nodes.push(n);
        dsu.0.push(dsu.0.len());
        dsu.0.len() - 1
    };
    for (si, sheet) in sch.sheets.iter().enumerate() {
        // Wire segments.
        let wires: Vec<(usize, Pt, Pt)> = sheet.wires.iter().map(|w| (add(Node::Wire, &mut dsu), w.a, w.b)).collect();
        // Points that join: each connection point with the node it belongs to.
        let mut points: Vec<(Pt, usize, bool)> = vec![]; // (where, node, may join a wire's middle)
        for &(n, a, b) in &wires {
            points.push((a, n, true));
            points.push((b, n, true));
        }
        for s in &sheet.symbols {
            let def = sch.symbol(&s.symbol);
            let power = def.is_some_and(|d| d.power);
            for (pin, at) in sch.placed_pins(s) {
                let n = add(
                    Node::Pin(NetPin {
                        symbol: s.id,
                        sheet: si,
                        reference: s.reference().into(),
                        number: pin.number.clone(),
                        name: pin.name.clone(),
                        kind: pin.kind,
                        at,
                        power,
                    }),
                    &mut dsu,
                );
                points.push((at, n, true));
                if power && pin.kind == PinType::PowerIn {
                    let pn = add(Node::PowerName(s.value().to_string()), &mut dsu);
                    dsu.join(n, pn);
                }
            }
        }
        for l in &sheet.labels {
            let global = !matches!(l.kind, LabelKind::Local);
            let n = add(Node::Label { text: l.text.text.clone(), global, sheet: si }, &mut dsu);
            points.push((l.text.at, n, true));
        }
        for nc in &sheet.no_connects {
            let n = add(Node::NoConnect, &mut dsu);
            points.push((nc.at, n, false));
        }
        // Coincident points.
        let mut by_point: HashMap<Pt, usize> = HashMap::new();
        for &(p, n, _) in &points {
            match by_point.get(&p) {
                Some(&m) => dsu.join(n, m),
                None => {
                    by_point.insert(p, n);
                }
            }
        }
        // Points on a wire's middle; junctions join every wire through them.
        for &(n, a, b) in &wires {
            for &(p, m, mid_ok) in &points {
                if mid_ok && m != n && p != a && p != b && on_segment(p, a, b) {
                    dsu.join(n, m);
                }
            }
            for j in &sheet.junctions {
                if on_segment(j.at, a, b) {
                    if let Some(&m) = by_point.get(&j.at) {
                        dsu.join(n, m);
                    } else {
                        by_point.insert(j.at, n);
                    }
                }
            }
        }
    }
    // Merge by name.
    let mut names: HashMap<(String, Option<usize>), usize> = HashMap::new();
    for (i, node) in nodes.iter().enumerate() {
        let key = match node {
            Node::Label { text, global: false, sheet } => (text.clone(), Some(*sheet)),
            Node::Label { text, global: true, .. } | Node::PowerName(text) => (text.clone(), None),
            _ => continue,
        };
        match names.get(&key) {
            Some(&j) => dsu.join(i, j),
            None => {
                names.insert(key, i);
            }
        }
    }
    // Collect.
    let mut groups: BTreeMap<usize, Net> = BTreeMap::new();
    let mut power_name: HashMap<usize, String> = HashMap::new();
    let mut global_name: HashMap<usize, String> = HashMap::new();
    let mut local_name: HashMap<usize, String> = HashMap::new();
    for (i, node) in nodes.iter().enumerate() {
        let root = dsu.find(i);
        let net = groups.entry(root).or_insert_with(|| Net { name: String::new(), pins: vec![], labels: vec![], no_connect: false });
        match node {
            Node::Pin(p) => net.pins.push(p.clone()),
            Node::Label { text, global, .. } => {
                net.labels.push(text.clone());
                let m = if *global { &mut global_name } else { &mut local_name };
                m.entry(root).or_insert_with(|| text.clone());
            }
            Node::PowerName(n) => {
                power_name.entry(root).or_insert_with(|| n.clone());
            }
            Node::NoConnect => net.no_connect = true,
            Node::Wire => {}
        }
    }
    let mut nets: Vec<Net> = vec![];
    for (root, mut net) in groups {
        if net.pins.is_empty() && net.labels.is_empty() {
            continue;
        }
        net.pins.sort_by(|a, b| natural(&a.reference).cmp(&natural(&b.reference)).then(natural(&a.number).cmp(&natural(&b.number))));
        net.name = power_name
            .get(&root)
            .or_else(|| global_name.get(&root))
            .or_else(|| local_name.get(&root))
            .cloned()
            .unwrap_or_else(|| {
                // A lone pin's net is "unconnected-(…)".
                let kind = if net.pins.len() == 1 { "unconnected" } else { "Net" };
                match net.part_pins().next().or(net.pins.first()) {
                    Some(p) if !p.name.is_empty() && p.name != "~" => format!("{kind}-({}-{})", p.reference, p.name),
                    Some(p) => format!("{kind}-({}-Pad{})", p.reference, p.number),
                    None => "unconnected".into(),
                }
            });
        nets.push(net);
    }
    nets.sort_by(|a, b| a.name.cmp(&b.name));
    Netlist { nets }
}

/// A key that sorts "R2" before "R10".
pub fn natural(s: &str) -> (String, u64, String) {
    let head: String = s.chars().take_while(|c| !c.is_ascii_digit()).collect();
    let digits: String = s[head.len()..].chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = s[head.len() + digits.len()..].to_string();
    (head, digits.parse().unwrap_or(0), rest)
}
