//! The CAM Studios, one per machine: [`cam3`], [`cam4`] and [`cam5`]. Each takes the Test
//! piece (and the 4-axis one the Worm) with its workholding as workspace components, and its
//! jobs run on the machine their assembly is set up as ([`super::machines`]), the work set on
//! that machine's mount:
//!
//! | Studio | Job | Machine | Held | Operations |
//! |---|---|---|---|---|
//! | CAM 3-axis | Job 1: Test piece | 3-axis mill | in the Vise, on the table | T1 D6 end mill: 2-axis rough of the pocket; T2 D6 drill: Drill of the two holes |
//! | CAM 4-axis | Job 1: Test piece | 4-axis mill | by its block in the chuck, on the `A` axis | T1 D6 end mill: Face of the flat that faces +Z, then of the flat 120° round (indexed); T2 D6 ball mill: Rotary spiral round the shaft |
//! | CAM 4-axis | Worm: the Worm, bar stock about X | 4-axis mill | by its spigot in the chuck | T1 D1 end mill: Rotary rough along the thread, in levels; T2 D1 ball mill: Rotary along the thread, at the lead read off the worm |
//! | CAM 5-axis | Job 1: Test piece | 5-axis mill | standing on the Fixture plate, on the `C` axis | T3 D10 end mill: Rough round the dome, upright; T1 D6 ball mill: Parallel over the dome with the tool square to it (5-axis); T2 D4 drill: Drill of the four dome holes, each down its own axis; Dynamic work plane "Dome hole" with a Drill in it (3+2) |

use cadrs_cam::strategy::{AxisMode, Height, HeightRef, Pattern, RotaryStyle, ToolAxis};
use cadrs_cam::{Params, Tool, ToolKind};

use crate::cam::{
    CamId, Edit, EditCam, Job, MachineSource, Mating, OpKind, Operation, Origin, Role, Setup,
    StockDef, StockDirection, StockKind, ToolOp, ToolpathOp, WorkPlane, WorkPlaneType,
    component_feature, geometry, program, workspace_component,
};
use crate::command::CommandError;
use crate::ids::{ElementId, PartId};
use crate::parts::Part;
use crate::solid::SolidFace;
use crate::studio::Studio;

use super::work;

/// The id numbered `n` in the CAM Studio of the mill with `axes` axes.
const fn id(axes: u64, n: u64) -> CamId {
    CamId(0xc4c0_0278_0000_0000 | (axes << 12) | n)
}

fn invalid(what: impl Into<String>) -> CommandError {
    CommandError::Invalid(what.into())
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1e-3)
}

/// One CAM Studio being filled.
struct Cam<'a> {
    s: &'a mut dyn Studio,
    element: ElementId,
}

impl Cam<'_> {
    fn edit(&mut self, edit: Edit) -> Result<(), CommandError> {
        self.s.run(&EditCam::new(self.element, edit))
    }

    fn studio(&self) -> Result<&crate::cam::CamStudio, CommandError> {
        self.s
            .document()
            .element(self.element)
            .and_then(|e| e.cam())
            .ok_or_else(|| invalid("not a CAM Studio"))
    }

    /// The studio's parts as they build.
    fn parts(&self) -> Result<Vec<Part>, CommandError> {
        Ok(crate::rebuild::build(self.studio()?.part_features())
            .parts
            .clone())
    }

    /// Inserts `part` of the Part Studio `source` as the component `id`; returns the part it
    /// is in this studio.
    fn component(
        &mut self,
        id: CamId,
        source: ElementId,
        part: PartId,
        role: Role,
    ) -> Result<PartId, CommandError> {
        let mut c = workspace_component(self.s.document(), source, part, role).map_err(invalid)?;
        c.id = id;
        self.edit(Edit::AddComponents {
            components: vec![c],
            links: vec![],
        })?;
        self.parts()?
            .iter()
            .map(|p| p.id)
            .find(|p| p.feature == component_feature(id))
            .ok_or_else(|| invalid("a component of the CNC machines sample didn't build"))
    }

    /// Creates a job of `bodies` held by `workholdings` on the machine the assembly `machine`
    /// is set up as.
    fn job(
        &mut self,
        id: CamId,
        name: &str,
        bodies: Vec<PartId>,
        workholdings: Vec<PartId>,
        stock: StockDef,
        machine: ElementId,
    ) -> Result<(), CommandError> {
        let source = MachineSource::workspace(self.s.document(), machine)
            .ok_or_else(|| invalid("a machine of the CNC machines sample is gone"))?;
        self.edit(Edit::AddJob(Box::new(Job {
            id,
            name: name.into(),
            bodies,
            stock,
            workholdings,
            machine: None,
            machine_source: None,
            setups: vec![],
            suppressed: false,
        })))?;
        self.edit(Edit::SetMachineSource {
            job: id,
            source: Some(Box::new(source)),
            links: vec![],
        })
    }

    /// A stretch of the model's X as the setup `setup` counts it (its work plane's X).
    fn along(&self, job: CamId, setup: CamId, x: [f64; 2]) -> Result<[f64; 2], CommandError> {
        let parts = self.parts()?;
        let cam = self.studio()?;
        let j = cam.job(job).ok_or_else(|| invalid("a job is gone"))?;
        let st = j
            .setups
            .iter()
            .find(|s| s.id == setup)
            .ok_or_else(|| invalid("a setup is gone"))?;
        let frame = program::setup_frame(cam, j, st, &parts);
        Ok(x.map(|v| frame.to_local([v, 0.0, 0.0])[0]))
    }

    /// Sets a setup's work on its machine's mount: the frame at `origin` with these `z` and
    /// `x` (all in the model's coordinates) is where the mount is.
    fn mount(
        &mut self,
        job: CamId,
        setup: CamId,
        origin: [f64; 3],
        z: [f64; 3],
        x: [f64; 3],
    ) -> Result<(), CommandError> {
        let parts = self.parts()?;
        let cam = self.studio()?;
        let j = cam.job(job).ok_or_else(|| invalid("a job is gone"))?;
        let mut st = j
            .setups
            .iter()
            .find(|s| s.id == setup)
            .ok_or_else(|| invalid("a setup is gone"))?
            .clone();
        // (The mating is given in the setup's own coordinates.)
        let frame = program::setup_frame(cam, j, &st, &parts);
        st.mating = Mating::Mount {
            origin: frame.to_local(origin),
            z: frame.dir_to_local(z),
            x: frame.dir_to_local(x),
        };
        self.edit(Edit::SetSetup {
            job,
            setup: Box::new(st),
        })
    }

    fn setup(
        &mut self,
        job: CamId,
        id: CamId,
        name: &str,
        program_number: u32,
        wcs: WorkPlane,
    ) -> Result<(), CommandError> {
        self.edit(Edit::AddSetup {
            job,
            setup: Box::new(Setup {
                id,
                name: name.into(),
                program_number,
                mating: Mating::Automatic,
                wcs,
                operations: vec![],
                suppressed: false,
            }),
        })
    }

    /// Adds an operation at the end of `setup`.
    fn operation(
        &mut self,
        setup: CamId,
        id: CamId,
        name: String,
        kind: OpKind,
    ) -> Result<(), CommandError> {
        self.edit(Edit::AddOperation {
            setup,
            after: None,
            op: Box::new(Operation {
                id,
                name,
                kind,
                suppressed: false,
                hidden: false,
            }),
        })
    }

    /// Adds `tool` as T`number`.
    fn tool(
        &mut self,
        setup: CamId,
        id: CamId,
        tool: &Tool,
        number: u32,
    ) -> Result<(), CommandError> {
        self.operation(
            setup,
            id,
            tool.name.clone(),
            OpKind::Tool(Box::new(ToolOp {
                tool: tool.clone(),
                number,
            })),
        )
    }

    /// Adds a toolpath, named after its pattern ("Drill 1").
    fn toolpath(&mut self, setup: CamId, id: CamId, params: Params) -> Result<(), CommandError> {
        let name = self.studio()?.next_name(params.pattern.label());
        self.operation(
            setup,
            id,
            name,
            OpKind::Toolpath(Box::new(ToolpathOp {
                params,
                name_edited: false,
            })),
        )
    }

    /// The drive names ([`geometry::face_key`]) of the faces of `part` that `pick` takes.
    fn faces(
        &self,
        part: PartId,
        pick: impl Fn(&SolidFace) -> bool,
    ) -> Result<Vec<String>, CommandError> {
        let parts = self.parts()?;
        let p = parts
            .iter()
            .find(|p| p.id == part)
            .ok_or_else(|| invalid("a component of the CNC machines sample is gone"))?;
        let keys: Vec<String> = p
            .solid
            .faces
            .iter()
            .filter(|f| pick(f))
            .map(|f| geometry::face_key(part, &f.name))
            .collect();
        if keys.is_empty() {
            return Err(invalid("a face of the Test piece wasn't found"));
        }
        Ok(keys)
    }

    /// The flat face of `part` whose middle is `at`.
    fn flat_face(&self, part: PartId, at: [f64; 3]) -> Result<Vec<String>, CommandError> {
        self.faces(part, |f| {
            f.plane.is_some() && f.center.is_some_and(|c| close(c, at))
        })
    }
}

/// "CAM 3-axis": the pocket and the two holes, the Test piece in the vise.
pub mod cam3 {
    use super::*;

    pub const STUDIO: ElementId = crate::samples::cnc_machines::CAM_3;
    /// The components.
    pub const PIECE: CamId = id(3, 0x01);
    pub const VISE: CamId = id(3, 0x02);
    pub const JOB: CamId = id(3, 0x10);
    pub const SETUP: CamId = id(3, 0x20);
    /// The operations, in order.
    pub const END_MILL: CamId = id(3, 0x31);
    pub const ROUGH: CamId = id(3, 0x32);
    pub const DRILL_BIT: CamId = id(3, 0x33);
    pub const DRILL: CamId = id(3, 0x34);
    /// The toolpaths.
    pub const TOOLPATHS: [CamId; 2] = [ROUGH, DRILL];

    /// Fills the empty CAM Studio [`STUDIO`].
    pub fn build_in(s: &mut dyn Studio) -> Result<(), CommandError> {
        use crate::samples::cnc_machines::{TEST_PIECE, WORKHOLDING};
        let mut c = Cam { s, element: STUDIO };
        let piece = c.component(PIECE, TEST_PIECE, work::PIECE, Role::Part)?;
        let vise = c.component(VISE, WORKHOLDING, work::VISE, Role::Workholding)?;
        c.job(
            JOB,
            "Job 1",
            vec![piece],
            vec![vise],
            StockDef::default(),
            crate::samples::cnc_machines::MILL_3,
        )?;
        c.setup(JOB, SETUP, "Setup 1", 1001, WorkPlane::default())?;
        // The vise's underside on the middle of the table, the piece's axes the machine's.
        c.mount(
            JOB,
            SETUP,
            work::VISE_FOOT,
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
        )?;
        let machine = cadrs_cam::MachineKind::Mill;
        let end_mill = Tool::new(ToolKind::EndMill, 6.0);
        c.tool(SETUP, END_MILL, &end_mill, 1)?;
        // Only the pocket: the stock above its floor.
        let mut rough = Params::new(Pattern::Rough2D, &end_mill, machine);
        rough.drive.faces = c.flat_face(piece, work::POCKET_FLOOR)?;
        c.toolpath(SETUP, ROUGH, rough)?;
        let drill = Tool::new(ToolKind::Drill, 6.0);
        c.tool(SETUP, DRILL_BIT, &drill, 2)?;
        c.toolpath(SETUP, DRILL, Params::new(Pattern::Drill, &drill, machine))
    }
}

/// "CAM 4-axis": the Test piece in the chuck (two of its flats, indexed, and its shaft round
/// the rotary axis), and the Worm roughed and finished along its thread.
pub mod cam4 {
    use super::*;

    pub const STUDIO: ElementId = crate::samples::cnc_machines::CAM_4;
    /// The components.
    pub const PIECE: CamId = id(4, 0x01);
    pub const CHUCK: CamId = id(4, 0x02);
    pub const WORM: CamId = id(4, 0x03);
    /// The Test piece's job, its setup and operations.
    pub const JOB: CamId = id(4, 0x10);
    pub const SETUP: CamId = id(4, 0x20);
    pub const END_MILL: CamId = id(4, 0x31);
    pub const FACE: CamId = id(4, 0x32);
    pub const FACE_ROUND: CamId = id(4, 0x33);
    pub const SHAFT_BALL: CamId = id(4, 0x34);
    pub const SHAFT: CamId = id(4, 0x35);
    /// The Worm's job, its setup and operations.
    pub const WORM_JOB: CamId = id(4, 0x11);
    pub const WORM_SETUP: CamId = id(4, 0x21);
    pub const ROUGHER: CamId = id(4, 0x41);
    pub const ROUGH: CamId = id(4, 0x42);
    pub const BALL_MILL: CamId = id(4, 0x43);
    pub const THREAD: CamId = id(4, 0x44);
    /// The toolpaths.
    pub const TOOLPATHS: [CamId; 5] = [FACE, FACE_ROUND, SHAFT, ROUGH, THREAD];

    /// Fills the empty CAM Studio [`STUDIO`].
    pub fn build_in(s: &mut dyn Studio) -> Result<(), CommandError> {
        use crate::samples::cnc_machines as doc;
        let mut c = Cam { s, element: STUDIO };
        let piece = c.component(PIECE, doc::TEST_PIECE, work::PIECE, Role::Part)?;
        let chuck = c.component(CHUCK, doc::WORKHOLDING, work::CHUCK, Role::Workholding)?;
        let worm = c.component(WORM, doc::WORM, work::WORM_PART, Role::Part)?;
        let machine = cadrs_cam::MachineKind::Mill;
        c.job(
            JOB,
            "Job 1",
            vec![piece],
            vec![chuck],
            StockDef::default(),
            doc::MILL_4,
        )?;
        c.setup(JOB, SETUP, "Setup 1", 1001, WorkPlane::default())?;
        // The block's end against the chuck's jaws, on the rotary axis: the mount looks along
        // the piece, its X up.
        let (along, up) = ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        c.mount(JOB, SETUP, [0.0; 3], along, up)?;
        let end_mill = Tool::new(ToolKind::EndMill, 6.0);
        c.tool(SETUP, END_MILL, &end_mill, 1)?;
        // The flat that faces up, down to it.
        let mut face = Params::new(Pattern::Face, &end_mill, machine);
        face.drive.faces = c.flat_face(piece, work::TOP_FLAT)?;
        face.heights.bottom = Height::new(HeightRef::Selection, 0.0);
        c.toolpath(SETUP, FACE, face.clone())?;
        // The next flat round, with the tool along its normal: the chuck indexes 120°.
        let round = work::flat_at(120.0);
        face.drive.faces = c.flat_face(piece, round.0)?;
        face.axis = ToolAxis::Direction(round.1);
        c.toolpath(SETUP, FACE_ROUND, face)?;
        // Round the shaft between the flats and the dome: the part turning under the ball.
        let ball = Tool::new(ToolKind::BallMill, 6.0);
        c.tool(SETUP, SHAFT_BALL, &ball, 2)?;
        let mut shaft = Params::new(Pattern::Rotary, &ball, machine);
        shaft.rotary.range = Some(c.along(JOB, SETUP, [88.0, 128.0])?);
        shaft.step_over = 2.0;
        shaft.tolerance = 0.02;
        c.toolpath(SETUP, SHAFT, shaft)?;
        // The worm from a bar: a cylinder about X, 1 mm over its tips.
        let bar = StockDef {
            kind: StockKind::Cylinder,
            direction: StockDirection::Axes {
                z: [1.0, 0.0, 0.0],
                x: [0.0, 1.0, 0.0],
            },
            offsets: [0.0; 6],
            radial: 1.0,
        };
        c.job(WORM_JOB, "Worm", vec![worm], vec![], bar, doc::MILL_4)?;
        c.setup(WORM_JOB, WORM_SETUP, "Setup 2", 1002, WorkPlane::default())?;
        // Its spigot in the chuck, the jaws well back from the thread so the spindle clears
        // them.
        c.mount(
            WORM_JOB,
            WORM_SETUP,
            [work::WORM_START - work::WORM_GRIP, 0.0, 0.0],
            along,
            up,
        )?;
        // The thread alone, clear of the chuck.
        let thread = c.along(
            WORM_JOB,
            WORM_SETUP,
            [
                work::WORM_START + 4.0,
                work::WORM_START + work::WORM_LENGTH - 1.0,
            ],
        )?;
        // A mill that fits the thread's space opens it along the thread, level by level.
        let mut rougher = Tool::new(ToolKind::EndMill, 1.0);
        rougher.cutting_length = 6.0;
        rougher.overall_length = 12.0;
        c.tool(WORM_SETUP, ROUGHER, &rougher, 1)?;
        let mut rough = Params::new(Pattern::RotaryRough, &rougher, machine);
        rough.rotary.style = RotaryStyle::Thread;
        rough.rotary.range = Some(thread);
        rough.step_over = 0.8;
        rough.step_down = 1.0;
        rough.wall_stock = 0.1;
        rough.floor_stock = 0.1;
        rough.tolerance = 0.02;
        c.toolpath(WORM_SETUP, ROUGH, rough)?;
        // Down the thread, at its own lead (read off the worm's helical edges).
        // (Its flutes reach from the thread's root out past its tips: no shank in the cut.)
        let mut ball_mill = Tool::new(ToolKind::BallMill, 1.0);
        ball_mill.cutting_length = 8.0;
        ball_mill.overall_length = 14.0;
        c.tool(WORM_SETUP, BALL_MILL, &ball_mill, 2)?;
        let mut along_thread = Params::new(Pattern::Rotary, &ball_mill, machine);
        along_thread.rotary.style = RotaryStyle::Thread;
        along_thread.rotary.range = Some(thread);
        along_thread.step_over = 0.5;
        along_thread.tolerance = 0.02;
        c.toolpath(WORM_SETUP, THREAD, along_thread)?;
        c.edit(Edit::SetActiveJob(JOB))
    }
}

/// "CAM 5-axis": the dome and one of its holes, the Test piece standing on the fixture plate.
pub mod cam5 {
    use super::*;

    pub const STUDIO: ElementId = crate::samples::cnc_machines::CAM_5;
    /// The components.
    pub const PIECE: CamId = id(5, 0x01);
    pub const FIXTURE_PLATE: CamId = id(5, 0x02);
    pub const JOB: CamId = id(5, 0x10);
    pub const SETUP: CamId = id(5, 0x20);
    /// The operations, in order.
    pub const BALL_MILL: CamId = id(5, 0x31);
    pub const PARALLEL: CamId = id(5, 0x32);
    pub const HOLE_PLANE: CamId = id(5, 0x33);
    pub const DRILL_BIT: CamId = id(5, 0x34);
    pub const DRILL: CamId = id(5, 0x35);
    /// The toolpaths.
    pub const DRILL_ALL: CamId = id(5, 0x36);
    pub const ROUGHER: CamId = id(5, 0x37);
    pub const ROUGH: CamId = id(5, 0x38);
    pub const TOOLPATHS: [CamId; 4] = [ROUGH, PARALLEL, DRILL_ALL, DRILL];
    /// The dome hole the work plane looks along (of [`work::DOME_HOLE_AXES`]).
    pub const HOLE: usize = 0;

    /// Fills the empty CAM Studio [`STUDIO`].
    pub fn build_in(s: &mut dyn Studio) -> Result<(), CommandError> {
        use crate::samples::cnc_machines::{TEST_PIECE, WORKHOLDING};
        let mut c = Cam { s, element: STUDIO };
        let piece = c.component(PIECE, TEST_PIECE, work::PIECE, Role::Part)?;
        let plate = c.component(
            FIXTURE_PLATE,
            WORKHOLDING,
            work::FIXTURE_PLATE,
            Role::Workholding,
        )?;
        c.job(
            JOB,
            "Job 1",
            vec![piece],
            vec![plate],
            StockDef::default(),
            crate::samples::cnc_machines::MILL_5,
        )?;
        // Looking at the dome: the tool along the piece's axis.
        let end_on = WorkPlane {
            z: [1.0, 0.0, 0.0],
            x: [0.0, 1.0, 0.0],
            ..WorkPlane::default()
        };
        c.setup(JOB, SETUP, "Setup 1", 1001, end_on)?;
        // The fixture plate's underside (the spigot's end) on the middle of the platter, the
        // piece standing up its axis.
        c.mount(
            JOB,
            SETUP,
            [-25.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
        )?;
        let machine = cadrs_cam::MachineKind::Mill;
        // The stock cleared round the dome first, upright, so the finishing tool leans in
        // free air: a D10 end mill in levels, a little left on the dome.
        let rougher = Tool::new(ToolKind::EndMill, 10.0);
        c.tool(SETUP, ROUGHER, &rougher, 3)?;
        let mut rough = Params::new(Pattern::Rough3D, &rougher, machine);
        rough.heights.bottom = Height::new(HeightRef::ModelTop, -work::DOME_RADIUS);
        rough.step_down = 4.0;
        rough.wall_stock = 0.5;
        rough.floor_stock = 0.5;
        c.toolpath(SETUP, ROUGH, rough)?;
        let ball_mill = Tool::new(ToolKind::BallMill, 6.0);
        c.tool(SETUP, BALL_MILL, &ball_mill, 1)?;
        // The dome only, down to its rim.
        let mut parallel = Params::new(Pattern::Parallel, &ball_mill, machine);
        parallel.drive.faces =
            c.faces(piece, |f| f.kind == Some(cadrs_kernel::SurfaceKind::Sphere))?;
        parallel.heights.bottom = Height::new(HeightRef::ModelTop, -work::DOME_RADIUS);
        // The tool square to the dome as it cuts (5-axis), no further over than 60°.
        parallel.tilt.mode = AxisMode::Normal;
        parallel.tilt.max_tilt = 60.0;
        parallel.step_over = 1.5;
        parallel.tolerance = 0.02;
        c.toolpath(SETUP, PARALLEL, parallel)?;
        // The four holes in the dome, each down its own axis.
        let (d, _) = work::DOME_HOLE;
        let drill = Tool::new(ToolKind::Drill, d);
        c.tool(SETUP, DRILL_BIT, &drill, 2)?;
        let mut all = Params::new(Pattern::Drill, &drill, machine);
        all.cycle.any_axis = true;
        c.toolpath(SETUP, DRILL_ALL, all)?;
        // A tilted plane (3+2) along one dome hole, its origin where the hole breaks out.
        let axis = work::DOME_HOLE_AXES[HOLE];
        c.operation(
            SETUP,
            HOLE_PLANE,
            "Dome hole".into(),
            OpKind::WorkPlane(Box::new(WorkPlane {
                origin: Origin::Manual(work::dome_hole_mouth(axis)),
                z: axis,
                x: [0.0, 1.0, 0.0],
                kind: WorkPlaneType::Dynamic,
                ..WorkPlane::default()
            })),
        )?;
        c.toolpath(SETUP, DRILL, Params::new(Pattern::Drill, &drill, machine))
    }
}
