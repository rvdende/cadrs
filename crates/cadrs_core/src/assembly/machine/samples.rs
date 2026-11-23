//! Sample machines as assemblies, for tests here and in CAM: each kind of
//! [`cadrs_cam::kinematics::samples`] built the way a user would, from instances and mates,
//! with the mounts picked ([`router`], [`trunnion_ac`], [`trunnion_bc`], [`gantry`], [`arm`]).
//!
//! They have no parts: every connector is a fixed frame of its instance
//! ([`MateConnector::at`]), so [`super::stored_frames`] resolves them. To be a fair test of
//! the signs, they are not tidy: instances are placed away from the identity, some mates have
//! their first connector on the body they carry and some on the one that carries it, one sits
//! away from its zero, one has an offset and one a flipped connector.
//!
//! [`Builder`] puts such an assembly together; the tests build their odd ones with it too.

use cadrs_sketch::Vec3;

use super::super::connector::{ConnectorFrame, MateConnector};
use super::super::mate::{
    Mate, MateFeature, MateId, MateKind, MateLimits, MateOffset, MateType, next_name,
};
use super::super::{Assembly, Instance, InstanceId, InstanceSource, Pose};
use super::{Issue, MachineDef, Resolved, derive, detect, resolve_model, stored_frames};
use crate::document::{Document, Element};
use crate::ids::{ElementId, FeatureId, PartId};

const X: Vec3 = [1.0, 0.0, 0.0];
const Y: Vec3 = [0.0, 1.0, 0.0];
const Z: Vec3 = [0.0, 0.0, 1.0];
const DOWN: Vec3 = [0.0, 0.0, -1.0];

/// What every id of a sample starts with.
const PREFIX: u128 = 0x3ac4_1e00_0000_0000_0000_0000_0000_0000;

/// The frame at `origin` with its Z along `z` (its X the model's X, or Y for a Z along X).
pub fn frame(origin: Vec3, z: Vec3) -> ConnectorFrame {
    ConnectorFrame::new(origin, z, super::super::connector::x_for(z))
}

/// A mate's position: its X, Y and Z travel (mm) and its angle (radians).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub angle: f64,
}

impl Position {
    /// `D` of the mate module: the turn about Z, then the move.
    fn pose(&self) -> Pose {
        Pose::rotation_about([0.0; 3], Z, self.angle)
            .then(&Pose::translation([self.x, self.y, self.z]))
    }
}

/// A sample machine: an assembly with its machine definition.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub name: &'static str,
    /// The assembly; its [`Assembly::machine`] has the mounts and the axes [`detect`]
    /// proposes.
    pub assembly: Assembly,
    /// The instances by the names they were made with.
    pub parts: Vec<(&'static str, InstanceId)>,
}

impl Sample {
    pub fn def(&self) -> &MachineDef {
        self.assembly
            .machine
            .as_deref()
            .expect("a sample has a machine definition")
    }

    /// The instance made as `name`.
    pub fn part(&self, name: &str) -> InstanceId {
        self.parts
            .iter()
            .find(|p| p.0 == name)
            .map(|p| p.1)
            .unwrap_or_else(|| panic!("no part {name}"))
    }

    /// The mate named `name`.
    pub fn mate(&self, name: &str) -> MateId {
        self.assembly
            .mates
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.id)
            .unwrap_or_else(|| panic!("no mate {name}"))
    }

    /// The machine it resolves to.
    pub fn resolve(&self) -> Result<Resolved, Vec<Issue>> {
        resolve_model(&self.assembly, self.def(), &stored_frames)
    }

    /// A document with the assembly as its one tab, and the tab's id.
    pub fn document(&self) -> (Document, ElementId) {
        let mut doc = Document::empty(self.name);
        let mut el = Element::assembly(self.name);
        el.id = ElementId::from_u128(PREFIX | 0xe1);
        el.assembly = self.assembly.clone();
        let id = el.id;
        doc.elements.push(el);
        (doc, id)
    }
}

/// Puts an assembly together from world frames: each mate's connectors are worked out from
/// where its instances are, so the assembly is solved as built.
#[derive(Debug, Clone, Default)]
pub struct Builder {
    pub assembly: Assembly,
    pub parts: Vec<(&'static str, InstanceId)>,
    tool_mount: Option<MateConnector>,
    table_mount: Option<MateConnector>,
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    /// An instance `name` placed at `pose` (where its own coordinates are: its parts being
    /// none, any placement does).
    pub fn part(&mut self, name: &'static str, pose: Pose) -> InstanceId {
        let n = self.assembly.instances.len() as u128 + 1;
        let id = InstanceId::from_u128(PREFIX | 0x1000 | n);
        let mut i = Instance::new(
            id,
            InstanceSource::Part {
                element: ElementId::from_u128(PREFIX | 0xe0),
                part: PartId::new(FeatureId::from_u128(PREFIX | 0x2000 | n), 0),
            },
            pose,
        );
        i.index = 1;
        self.assembly.instances.push(i);
        self.parts.push((name, id));
        id
    }

    /// A fixed instance at the identity: the base.
    pub fn base(&mut self, name: &'static str) -> InstanceId {
        let id = self.part(name, Pose::IDENTITY);
        if let Some(i) = self.assembly.instance_mut(id) {
            i.fixed = true;
        }
        id
    }

    /// `frame` (the assembly's coordinates) in the coordinates of `instance`.
    fn local(&self, instance: InstanceId, frame: &ConnectorFrame) -> ConnectorFrame {
        let pose = self
            .assembly
            .instance(instance)
            .map(|i| i.pose)
            .unwrap_or_default();
        frame.moved(&pose.inverse())
    }

    fn add(&mut self, name: &str, kind: MateKind) -> MateId {
        let id = MateId::from_u128(PREFIX | 0x3000 | (self.assembly.mates.len() as u128 + 1));
        self.assembly.mates.push(MateFeature::new(id, name, kind));
        id
    }

    /// A mate `name` of type `t` whose first connector is on `first` at `at` (the assembly's
    /// coordinates) and whose second is on `second`, where the mate's position is `now`.
    #[allow(clippy::too_many_arguments)]
    pub fn mate(
        &mut self,
        name: &str,
        t: MateType,
        first: InstanceId,
        second: InstanceId,
        at: ConnectorFrame,
        now: Position,
        limits: Option<MateLimits>,
    ) -> MateId {
        let target = at.then_local(&now.pose());
        let mut m = Mate::new(
            t,
            MateConnector::at(first, self.local(first, &at)),
            MateConnector::at(second, self.local(second, &target)),
        );
        m.limits = limits;
        self.add(name, MateKind::Mate(m))
    }

    /// A Slider `name` along `at`'s Z between `lo` and `hi` mm (see [`Builder::mate`]).
    pub fn slider(
        &mut self,
        name: &str,
        first: InstanceId,
        second: InstanceId,
        at: ConnectorFrame,
        (lo, hi): (f64, f64),
    ) -> MateId {
        let limits = MateLimits {
            z: Some((lo, hi)),
            ..MateLimits::default()
        };
        self.mate(
            name,
            MateType::Slider,
            first,
            second,
            at,
            Position::default(),
            Some(limits),
        )
    }

    /// A Revolute `name` about `at`'s Z, between `limits` degrees if any.
    pub fn revolute(
        &mut self,
        name: &str,
        first: InstanceId,
        second: InstanceId,
        at: ConnectorFrame,
        limits: Option<(f64, f64)>,
    ) -> MateId {
        let limits = limits.map(|(lo, hi)| MateLimits {
            angle: Some((lo.to_radians(), hi.to_radians())),
            ..MateLimits::default()
        });
        self.mate(
            name,
            MateType::Revolute,
            first,
            second,
            at,
            Position::default(),
            limits,
        )
    }

    /// A Fastened mate of `a` and `b` where they are.
    pub fn fasten(&mut self, a: InstanceId, b: InstanceId) -> MateId {
        let name = next_name(&self.assembly.mates, "Fastened");
        let at = frame([0.0; 3], Z);
        self.mate(
            &name,
            MateType::Fastened,
            a,
            b,
            at,
            Position::default(),
            None,
        )
    }

    /// A group of `instances`.
    pub fn group(&mut self, instances: &[InstanceId]) -> MateId {
        let name = next_name(&self.assembly.mates, "Group");
        self.add(
            &name,
            MateKind::Group {
                instances: instances.to_vec(),
            },
        )
    }

    /// The mate feature `id`, to change after it is made.
    pub fn mate_mut(&mut self, id: MateId) -> &mut Mate {
        self.assembly
            .mates
            .iter_mut()
            .find(|f| f.id == id)
            .and_then(|f| f.mate_mut())
            .expect("a mate")
    }

    /// The tool mount on `instance` at `at` (the assembly's coordinates; its Z out of the
    /// spindle).
    pub fn tool_mount(&mut self, instance: InstanceId, at: ConnectorFrame) {
        self.tool_mount = Some(MateConnector::at(instance, self.local(instance, &at)));
    }

    /// The table mount on `instance` at `at` (its Z out of the table).
    pub fn table_mount(&mut self, instance: InstanceId, at: ConnectorFrame) {
        self.table_mount = Some(MateConnector::at(instance, self.local(instance, &at)));
    }

    /// The definition with the mounts picked and no axes yet.
    pub fn def(&self) -> MachineDef {
        MachineDef {
            tool_mount: self.tool_mount,
            table_mount: self.table_mount,
            ..MachineDef::default()
        }
    }

    /// The sample: the assembly with the mounts and the axes [`detect`] proposes.
    pub fn finish(mut self, name: &'static str) -> Sample {
        let mut def = self.def();
        let tree = derive(&self.assembly, &def, &stored_frames);
        def.axes = detect(&tree, &def).axes;
        self.assembly.machine = Some(Box::new(def));
        Sample {
            name,
            assembly: self.assembly,
            parts: self.parts,
        }
    }
}

/// A gantry router with a rotary axis along X on its bed
/// ([`cadrs_cam::kinematics::samples::fourth_axis`]): X, Y and Z carry the tool, A the work.
///
/// | Mate | From → to | Along / about | Mate limits | First connector on |
/// |---|---|---|---|---|
/// | X | Base → Gantry | +X | −300…300 | the Base |
/// | Y | Gantry → Carriage | +Y | −150…250 | the Carriage (so it moves −Y as the mate rises) |
/// | Z | Carriage → Head | −Z (plunge) | 0…120 | the Carriage |
/// | A | Base → Chuck | +X through (0, 0, 50) | none | the Base |
pub fn router() -> Sample {
    let mut b = Builder::new();
    let base = b.base("Base");
    let gantry = b.part("Gantry", Pose::translation([0.0, 0.0, 150.0]));
    let carriage = b.part("Carriage", Pose::rotation_about([5.0, 0.0, 0.0], Z, 0.3));
    let head = b.part("Head", Pose::translation([0.0, 20.0, 260.0]));
    let chuck = b.part("Chuck", Pose::rotation_about([0.0, 0.0, 50.0], X, 0.5));
    b.slider(
        "X",
        base,
        gantry,
        frame([0.0, -200.0, 100.0], X),
        (-300.0, 300.0),
    );
    b.slider(
        "Y",
        carriage,
        gantry,
        frame([0.0, 0.0, 250.0], Y),
        (-150.0, 250.0),
    );
    b.slider(
        "Z",
        carriage,
        head,
        frame([0.0, 40.0, 250.0], DOWN),
        (0.0, 120.0),
    );
    b.revolute("A", base, chuck, frame([0.0, 0.0, 50.0], X), None);
    b.tool_mount(head, frame([0.0, 0.0, 200.0], DOWN));
    b.table_mount(chuck, frame([0.0, 0.0, 50.0], Z));
    b.finish("Router")
}

/// A vertical mill with a trunnion on its table
/// ([`cadrs_cam::kinematics::samples::trunnion_ac`]): the table moves in Y and X under a head
/// that moves in Z; `tilt` is the trunnion's letter, A (about X) or B (about Y), and the
/// platter C misses its line by 10 mm.
///
/// | Mate | From → to | Along / about | Mate limits | Note |
/// |---|---|---|---|---|
/// | Y | Base → Saddle | +Y | −200…200 | |
/// | X | Saddle → Table | +X | −300…300 | first connector on the Table |
/// | Z | Column → Head | +Z | −400…0 | 25 mm below its zero as built |
/// | A or B | Table → Trunnion | +X or +Y through (0, 0, −60) | −120°…120° | |
/// | C | Trunnion → Platter | +Z through (0, 10, 0) | none | |
///
/// The Column is fastened to the Base, and the Spindle is grouped with the Head.
fn trunnion(tilt: &'static str) -> Sample {
    let mut b = Builder::new();
    let base = b.base("Base");
    let column = b.part("Column", Pose::translation([0.0, 300.0, 0.0]));
    let saddle = b.part("Saddle", Pose::translation([0.0, 0.0, -150.0]));
    let table = b.part("Table", Pose::rotation_about([0.0; 3], Z, -0.7));
    let head = b.part("Head", Pose::translation([0.0, 150.0, 500.0]));
    let spindle = b.part("Spindle", Pose::rotation_about([0.0, 0.0, 300.0], Y, 1.1));
    let cradle = b.part("Trunnion", Pose::translation([0.0, 0.0, -60.0]));
    let platter = b.part("Platter", Pose::translation([0.0, 10.0, 0.0]));
    b.fasten(base, column);
    b.group(&[head, spindle]);
    b.slider(
        "Y",
        base,
        saddle,
        frame([0.0, 0.0, -160.0], Y),
        (-200.0, 200.0),
    );
    b.slider(
        "X",
        table,
        saddle,
        frame([0.0, 0.0, -100.0], X),
        (-300.0, 300.0),
    );
    b.mate(
        "Z",
        MateType::Slider,
        column,
        head,
        frame([0.0, 250.0, 500.0], Z),
        Position {
            z: -25.0,
            ..Position::default()
        },
        Some(MateLimits {
            z: Some((-400.0, 0.0)),
            ..MateLimits::default()
        }),
    );
    let line = if tilt == "B" { Y } else { X };
    b.revolute(
        tilt,
        table,
        cradle,
        frame([0.0, 0.0, -60.0], line),
        Some((-120.0, 120.0)),
    );
    b.revolute("C", cradle, platter, frame([0.0, 10.0, 0.0], Z), None);
    b.tool_mount(spindle, frame([0.0, 0.0, 300.0], DOWN));
    b.table_mount(platter, frame([0.0, 10.0, 0.0], Z));
    b.finish(if tilt == "B" {
        "Trunnion B/C"
    } else {
        "Trunnion A/C"
    })
}

/// The trunnion mill with A and C under the work.
pub fn trunnion_ac() -> Sample {
    trunnion("A")
}

/// The trunnion mill with B and C under the work.
pub fn trunnion_bc() -> Sample {
    trunnion("B")
}

/// A gantry with a fork head ([`cadrs_cam::kinematics::samples::head_head`]): X, Y, Z, then C
/// about the vertical carrying B, all on the tool's side; the work sits on the Base.
///
/// | Mate | From → to | Along / about | Mate limits | Note |
/// |---|---|---|---|---|
/// | X | Base → Bridge | +X | −500…500 | |
/// | Y | Bridge → Saddle | +Y | −400…400 | |
/// | Z | Saddle → Ram | +Z | −300…0 | |
/// | C | Ram → Fork | −Z through the origin | −360°…360° | the Fork's connector is stored flipped, with Flip on |
/// | B | Fork → Head | +Y through (0, 0, 420) | −100°…100° | an offset of 15 mm along the pivot |
pub fn gantry() -> Sample {
    let mut b = Builder::new();
    let base = b.base("Base");
    let bridge = b.part("Bridge", Pose::translation([0.0, 0.0, 600.0]));
    let saddle = b.part("Saddle", Pose::translation([30.0, 0.0, 600.0]));
    let ram = b.part("Ram", Pose::rotation_about([0.0; 3], X, 0.2));
    let fork = b.part("Fork", Pose::translation([0.0, 0.0, 450.0]));
    let head = b.part("Head", Pose::rotation_about([0.0, 0.0, 420.0], Y, -0.4));
    b.slider(
        "X",
        base,
        bridge,
        frame([0.0, -450.0, 600.0], X),
        (-500.0, 500.0),
    );
    b.slider(
        "Y",
        bridge,
        saddle,
        frame([0.0, 0.0, 650.0], Y),
        (-400.0, 400.0),
    );
    b.slider(
        "Z",
        saddle,
        ram,
        frame([0.0, 60.0, 600.0], Z),
        (-300.0, 0.0),
    );
    let c = b.revolute(
        "C",
        ram,
        fork,
        frame([0.0, 0.0, 480.0], DOWN),
        Some((-360.0, 360.0)),
    );
    // The same connector, stored the other way up and flipped back by the mate.
    let on_fork = &mut b.mate_mut(c).connectors[1];
    on_fork.frame = on_fork.frame.flipped();
    on_fork.flip = true;
    let pivot = b.revolute(
        "B",
        fork,
        head,
        frame([0.0, 0.0, 420.0], Y),
        Some((-100.0, 100.0)),
    );
    // The Head's connector sits 15 mm back along the pivot, and the mate's offset makes it
    // up: the target F₂ · O is where it was.
    let offset = MateOffset {
        translation: [0.0, 0.0, 15.0],
        axis: 2,
        angle: 0.0,
    };
    let m = b.mate_mut(pivot);
    m.connectors[1].frame = m.connectors[1].frame.then_local(&offset.pose().inverse());
    m.offset = Some(offset);
    b.tool_mount(head, frame([0.0, 0.0, 300.0], DOWN));
    b.table_mount(base, frame([0.0, 0.0, 0.0], Z));
    b.finish("Gantry C/B")
}

/// A six-axis arm with the AR4's link lengths (mm: shoulder 64.2 out and 169.77 up, upper arm
/// 305, forearm 222.63, flange 36.25), stretched forward at home, a spindle on its flange
/// pointing down, and the work on a stand fixed with the Base.
///
/// | Mate | From → to | About, through | Limits | First connector on |
/// |---|---|---|---|---|
/// | J1 | Base → Link 1 | +Z, the origin | ±170° | the Base |
/// | J2 | Link 1 → Link 2 | +Y, (64.2, 0, 169.77) | −42°…90° | Link 2 |
/// | J3 | Link 2 → Link 3 | +Y, (64.2, 0, 474.77) | −89°…52° | Link 2 |
/// | J4 | Link 3 → Link 4 | +X, (64.2, 0, 474.77) | ±165° | Link 4 |
/// | J5 | Link 4 → Link 5 | +Y, (286.83, 0, 474.77) | ±105° | Link 4 |
/// | J6 | Link 5 → Link 6 | +X, (286.83, 0, 474.77) | ±155° | Link 5 |
pub fn arm() -> Sample {
    let mut b = Builder::new();
    let base = b.base("Base");
    let stand = b.part("Stand", Pose::translation([350.0, 0.0, 0.0]));
    b.fasten(base, stand);
    let shoulder = [64.2, 0.0, 169.77];
    let elbow = [64.2, 0.0, 474.77];
    let wrist = [286.83, 0.0, 474.77];
    let l1 = b.part("Link 1", Pose::translation([0.0, 0.0, 90.0]));
    let l2 = b.part("Link 2", Pose::rotation_about(shoulder, Y, 0.6));
    let l3 = b.part("Link 3", Pose::translation(elbow));
    let l4 = b.part("Link 4", Pose::rotation_about(elbow, X, -1.2));
    let l5 = b.part("Link 5", Pose::translation(wrist));
    let l6 = b.part("Link 6", Pose::rotation_about(wrist, Z, 0.9));
    b.revolute("J1", base, l1, frame([0.0; 3], Z), Some((-170.0, 170.0)));
    b.revolute("J2", l2, l1, frame(shoulder, Y), Some((-42.0, 90.0)));
    b.revolute("J3", l2, l3, frame(elbow, Y), Some((-89.0, 52.0)));
    b.revolute("J4", l4, l3, frame(elbow, X), Some((-165.0, 165.0)));
    b.revolute("J5", l4, l5, frame(wrist, Y), Some((-105.0, 105.0)));
    b.revolute("J6", l5, l6, frame(wrist, X), Some((-155.0, 155.0)));
    b.tool_mount(l6, frame([383.08, 0.0, 434.77], DOWN));
    b.table_mount(stand, frame([350.0, 0.0, 100.0], Z));
    b.finish("Arm")
}

/// Every sample machine.
pub fn all() -> Vec<Sample> {
    vec![router(), trunnion_ac(), trunnion_bc(), gantry(), arm()]
}
