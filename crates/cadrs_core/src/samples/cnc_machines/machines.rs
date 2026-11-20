//! The three assemblies set up as machines (the Machine Builder's work): where the tool goes
//! on, where the work goes on, the axes between, and the controller.
//!
//! | Machine | Tool mount | Table mount | Axes | Controller |
//! |---|---|---|---|---|
//! | 3-axis mill | the spindle's nose | the middle of the table's top | Z · Y X | Fanuc |
//! | 4-axis mill | the spindle's nose | the chuck's jaw face, looking along the part | Z · Y X A | LinuxCNC (no tool centre point control) |
//! | 5-axis mill | the spindle's nose | the middle of the platter's top | Z B · Y X C | Fanuc, with tool centre point control |
//!
//! The axes are the ones the definition's mounts give ([`resolve_model`]): every joint between
//! the Base and a mount, named after its direction. The 4-axis mill's `Magazine` is on neither
//! path, so it is an actuator: its eight pockets are its positions and the tool change's.

use cadrs_cam::Dialect;
use cadrs_cam::machine::catalog;

use crate::assembly::InstanceId;
use crate::assembly::connector::{ConnectorFrame, MateConnector};
use crate::assembly::machine::commands::SetMachineDef;
use crate::assembly::machine::{
    Actuator, ActuatorId, MachineDef, Pocket, ToolChange, flat_model, resolve_model, stored_frames,
};
use crate::assembly::mate::Dof;
use crate::command::CommandError;
use crate::ids::ElementId;
use crate::studio::Studio;

use super::assemblies::{mill3, mill4, mill5};
use super::{PREFIX, parts};

/// The 4-axis mill's tool magazine as an actuator.
pub const MAGAZINE: ActuatorId = ActuatorId(uuid::Uuid::from_u128(PREFIX | (7 << 92) | 0x41));

/// The spindle's nose: the tool goes on here, pointing down.
fn nose() -> ConnectorFrame {
    ConnectorFrame::new(
        [0.0, 0.0, parts::SPINDLE_NOSE],
        [0.0, 0.0, -1.0],
        [1.0, 0.0, 0.0],
    )
}

/// The chuck's jaw face on the 4-axis mill: the work is held against it, on the `A` axis. The
/// mount looks along the part (+X), its own X up.
pub fn chuck_mount() -> ConnectorFrame {
    ConnectorFrame::new(
        [parts::CHUCK_JAW_FACE, 0.0, parts::CHUCK_AXIS[2]],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
    )
}

/// The middle of a flat table's top at height `z`: the mount looks up, its X the machine's.
fn top(z: f64) -> ConnectorFrame {
    ConnectorFrame::new([0.0, 0.0, z], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0])
}

/// A catalogue mill made over as a sample machine's controller.
fn controller(model: &str, dialect: Dialect, tool_changer: bool) -> cadrs_cam::Machine {
    let mut m = catalog()
        .into_iter()
        .find(|m| m.kind == cadrs_cam::MachineKind::Mill)
        .expect("the catalogue has a mill");
    m.builder = "cadrs sample".into();
    m.model = model.into();
    m.dialect = dialect;
    m.post = cadrs_cam::PostSettings::for_dialect(dialect, m.kind);
    m.tool_changer = tool_changer;
    m.indexing = None;
    m
}

/// Sets the assembly `element` up as a machine: the mounts given, the axes they give.
fn set_up(
    s: &mut dyn Studio,
    element: ElementId,
    spindle: InstanceId,
    table: (InstanceId, ConnectorFrame),
    controller: cadrs_cam::Machine,
    finish: impl FnOnce(&mut MachineDef),
) -> Result<(), CommandError> {
    let mut def = MachineDef::new(controller);
    def.tool_mount = Some(MateConnector::at(spindle, nose()));
    def.table_mount = Some(MateConnector::at(table.0, table.1));
    let doc = s.document();
    let asm = doc
        .element(element)
        .and_then(|e| e.assembly_model())
        .ok_or_else(|| CommandError::Invalid("not an assembly".into()))?;
    let resolved =
        resolve_model(&flat_model(doc, asm), &def, &stored_frames).map_err(|issues| {
            CommandError::Invalid(format!(
                "a machine of the CNC machines sample doesn't resolve: {}",
                issues
                    .iter()
                    .map(|i| i.text.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            ))
        })?;
    def.axes = resolved.axes;
    finish(&mut def);
    s.run(&SetMachineDef {
        element,
        def: Some(Box::new(def)),
        label: "Set up machine".into(),
    })
}

/// Sets up the three mills.
pub fn build_in(s: &mut dyn Studio) -> Result<(), CommandError> {
    set_up(
        s,
        mill3::ASSEMBLY,
        mill3::SPINDLE,
        (mill3::TABLE, top(0.0)),
        controller("3-axis mill", Dialect::Fanuc, false),
        |_| {},
    )?;
    set_up(
        s,
        mill4::ASSEMBLY,
        mill4::SPINDLE,
        (mill4::CHUCK, chuck_mount()),
        controller("4-axis mill", Dialect::LinuxCnc, true),
        |def| {
            // The magazine: a pocket every 45°, changed at with the head right up.
            let pockets: Vec<(String, f64)> = (1..=8)
                .map(|k| (format!("Pocket {k}"), mill4::pocket_angle(k).to_degrees()))
                .collect();
            def.tool_change = ToolChange {
                position: vec![("Z".into(), 0.0)],
                magazine: Some(MAGAZINE),
                pockets: pockets
                    .iter()
                    .enumerate()
                    .map(|(k, p)| Pocket {
                        number: k as u32 + 1,
                        value: p.1,
                    })
                    .collect(),
            };
            def.actuators = vec![Actuator {
                id: MAGAZINE,
                name: "Magazine".into(),
                mate: mill4::MAGAZINE,
                dof: Dof::Angle,
                positions: pockets,
                rest: 0,
            }];
        },
    )?;
    set_up(
        s,
        mill5::ASSEMBLY,
        mill5::SPINDLE,
        (mill5::PLATTER, top(parts::PLATTER_TOP)),
        controller("5-axis mill", Dialect::Fanuc, false),
        |_| {},
    )
}
