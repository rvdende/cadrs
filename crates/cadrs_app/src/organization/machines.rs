//! The organization screen's Machines module: what each machine can make (its process, bed,
//! materials and how thick) and the location it stands at. Requests are matched against them.

use super::*;

pub(crate) const PROCESSES: [Process; 8] = [
    Process::Waterjet,
    Process::Laser,
    Process::Plasma,
    Process::Router,
    Process::Mill,
    Process::Lathe,
    Process::PressBrake,
    Process::Printer,
];

/// A process's name, from the catalogs.
pub(crate) fn process_name(process: Process) -> String {
    tr(match process {
        Process::Waterjet => "org-process-waterjet",
        Process::Laser => "org-process-laser",
        Process::Plasma => "org-process-plasma",
        Process::Router => "org-process-router",
        Process::Mill => "org-process-mill",
        Process::Lathe => "org-process-lathe",
        Process::PressBrake => "org-process-press-brake",
        Process::Printer => "org-process-printer",
    })
}

/// The machine the dialog edits; none for a new one.
#[derive(Resource)]
pub(super) struct EditingMachine(Option<cadrs_org::Id>);

/// Whether a machine of this process is described by a bed and what it cuts through (a
/// waterjet, a laser), rather than by a space it works in (a printer, a mill, a lathe).
fn cuts_sheet(process: Process) -> bool {
    process.cuts_profiles()
}

/// The catalog keys of the dialog's two labels, for a process: a cutter has a bed, a kerf and
/// how thick it cuts each material; a printer or a mill has a space of three sizes and the
/// materials it works in.
fn field_keys(process: Process) -> (&'static str, &'static str) {
    if cuts_sheet(process) {
        ("org-machine-bed", "org-machine-materials")
    } else {
        ("org-machine-envelope", "org-machine-materials-names")
    }
}

/// The dialog's labels that say what its fields mean, which depends on the process.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum MachineLabel {
    Size,
    Materials,
}

/// The dialog's material dropdown: `org-machine-material-choose`, `-search`, and a row a
/// material, `org-machine-material-item-<slug>`.
const MATERIAL_PICKER: &str = "org-machine-material";

/// The dialog's list of the materials the machine works in; the number names the next row.
#[derive(Component, Default)]
pub(super) struct MachineMaterials(u32);

/// A material of the dialog's list. Its row is `org-machine-material-<key>`, with
/// `org-machine-thickness-<key>` (the thickest it cuts) and `org-machine-material-remove-<key>`.
#[derive(Component, Debug, Clone)]
pub(super) struct MachineMaterial {
    name: String,
    key: u32,
}

/// The part of a material's row that says how thick it is cut: there for a machine that cuts
/// sheet, not for a printer or a mill.
#[derive(Component)]
pub(super) struct ThicknessCell;

fn shown(show: bool) -> Display {
    if show { Display::Flex } else { Display::None }
}

/// A material's row in the dialog: its name, the thickest of it the machine cuts (`sheet`),
/// and Remove.
fn material_row(t: &Theme, name: &str, key: u32, thickness: f64, sheet: bool) -> impl Bundle {
    let thickness = if thickness > 0.0 {
        thickness.to_string()
    } else {
        String::new()
    };
    (
        Name::new(format!("org-machine-material-{key}")),
        MachineMaterial {
            name: name.to_owned(),
            key,
        },
        Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            min_height: Val::Px(28.0),
            ..default()
        },
        children![
            (
                Node {
                    flex_grow: 1.0,
                    flex_basis: Val::Px(0.0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                children![t.text(
                    name.to_owned(),
                    t.font_base,
                    FontWeight::NORMAL,
                    t.foreground
                )],
            ),
            (
                ThicknessCell,
                Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(6.0),
                    display: shown(sheet),
                    ..default()
                },
                children![
                    t.text(
                        tr("org-machine-thickest"),
                        t.font_sm,
                        FontWeight::NORMAL,
                        t.muted_foreground
                    ),
                    TextInput::new(format!("org-machine-thickness-{key}"))
                        .value(thickness)
                        .width(Val::Px(70.0))
                        .build(t),
                    t.text("mm", t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                ],
            ),
            (
                Button::new(format!("org-machine-material-remove-{key}"))
                    .label(tr("common-remove"))
                    .small()
                    .ghost()
                    .build(t),
                observe(
                    |ev: On<Activate>, rows: Query<&ChildOf>, mut commands: Commands| {
                        if let Ok(row) = rows.get(ev.entity) {
                            commands.entity(row.parent()).try_despawn();
                        }
                    },
                ),
            ),
        ],
    )
}

/// The dialog's dropdown chose a material: it joins the machine's list, once.
pub(super) fn on_material_picked(
    ev: On<cadrs_ui::SearchSelectPick>,
    names: Query<&Name>,
    mut commands: Commands,
) {
    if names
        .get(ev.entity)
        .is_ok_and(|n| n.as_str() == format!("{MATERIAL_PICKER}-select"))
    {
        let name = ev.value.clone();
        commands.queue(move |world: &mut World| add_machine_material(world, &name));
    }
}

fn add_machine_material(world: &mut World, name: &str) {
    let name: String = name.trim().chars().take(cadrs_org::MAX_LABEL).collect();
    let mut rows = world.query::<&MachineMaterial>();
    if name.is_empty()
        || rows
            .iter(world)
            .any(|m| m.name.to_lowercase() == name.to_lowercase())
    {
        return;
    }
    let sheet = cuts_sheet(dialog_process(world));
    let theme = world.resource::<Theme>().clone();
    let mut lists = world.query::<(Entity, &mut MachineMaterials)>();
    let Some((list, mut next)) = lists.iter_mut(world).next() else {
        return;
    };
    let key = next.0;
    next.0 += 1;
    world
        .entity_mut(list)
        .with_child(material_row(&theme, &name, key, 0.0, sheet));
}

/// The process the dialog has picked.
fn dialog_process(world: &mut World) -> Process {
    PROCESSES[strip_selected(world, "org-machine-process").min(PROCESSES.len() - 1)]
}

/// The dialog's materials, in the order they are listed: for a machine that cuts sheet
/// (`thicknesses`), each with the thickest it cuts if that is said.
fn dialog_materials(world: &mut World, thicknesses: bool) -> Vec<MaterialLimit> {
    let mut lists = world.query_filtered::<&Children, With<MachineMaterials>>();
    let rows: Vec<Entity> = lists
        .iter(world)
        .next()
        .map(|children| children.to_vec())
        .unwrap_or_default();
    let listed: Vec<MachineMaterial> = rows
        .into_iter()
        .filter_map(|row| world.get::<MachineMaterial>(row).cloned())
        .collect();
    listed
        .into_iter()
        .map(|m| MaterialLimit {
            max_thickness: number(world, &format!("org-machine-thickness-{}", m.key))
                .filter(|mm| thicknesses && mm.is_finite() && *mm > 0.0)
                .unwrap_or(0.0),
            material: m.name,
            speeds: Vec::new(),
        })
        .collect()
}

/// Another process was picked in the dialog: its fields are named for it, and its materials
/// say how thick only for a machine that cuts sheet.
pub(super) fn on_process(
    ev: On<cadrs_ui::tab_strip::TabStripSelect>,
    names: Query<&Name>,
    mut labels: Query<(&MachineLabel, &mut Text)>,
    mut thicknesses: Query<&mut Node, With<ThicknessCell>>,
) {
    if names.get(ev.entity).map(Name::as_str) != Ok("org-machine-process") {
        return;
    }
    let process = PROCESSES[ev.index.min(PROCESSES.len() - 1)];
    let (size, materials) = field_keys(process);
    for (label, mut text) in &mut labels {
        text.0 = tr(match label {
            MachineLabel::Size => size,
            MachineLabel::Materials => materials,
        });
    }
    for mut cell in &mut thicknesses {
        cell.display = shown(cuts_sheet(process));
    }
}

pub(super) fn machines(b: &mut ChildSpawnerCommands, t: &Theme, v: &ModuleView) {
    let org = &v.org;
    page_head(b, t, "org-tab-machines", "org-machines-about");
    let columns = vec![
        Column::new("name", tr("org-machine-name")).width(190.0),
        Column::new("process", tr("org-machine-process")).width(100.0),
        Column::new("bed", tr("org-machine-bed-short")).width(170.0),
        Column::new("materials", tr("org-machine-materials-short")).width(260.0),
        Column::new("location", tr("org-machine-location")).width(150.0),
        Column::new("actions", "").width(150.0),
    ];
    let rows = org
        .machines
        .iter()
        .enumerate()
        .map(|(i, (id, m))| {
            let id = *id;
            let materials: Vec<String> = m.materials.iter().map(MaterialLimit::label).collect();
            let size = if cuts_sheet(m.process) {
                format!("{} × {} mm", m.envelope[0], m.envelope[1])
            } else {
                format!(
                    "{} × {} × {} mm",
                    m.envelope[0], m.envelope[1], m.envelope[2]
                )
            };
            let location = org
                .location(m.location)
                .map_or_else(|| "—".to_owned(), |l| l.name.clone());
            text_row(
                t,
                format!("org-machine-{i}"),
                &columns,
                [
                    m.name.clone(),
                    process_name(m.process),
                    size,
                    materials.join(", "),
                    location,
                ],
            )
            .cell(move |c| {
                cell_button(
                    c,
                    format!("org-machine-edit-{i}"),
                    tr("common-edit"),
                    false,
                    move |w| open_machine(w, Some(id)),
                );
                cell_button(
                    c,
                    format!("org-machine-remove-{i}"),
                    tr("common-remove"),
                    false,
                    move |w| ask_remove(w, Op::SetMachine { id, machine: None }),
                );
            })
        })
        .collect();
    table(b, t, "org-machines", columns, rows, "org-no-machines");
    b.spawn((
        Button::new("org-add-machine")
            .label(tr("org-add-machine"))
            .icon("plus")
            .build(t),
        observe(|_: On<Activate>, mut commands: Commands| {
            commands.queue(|w: &mut World| open_machine(w, None));
        }),
    ));
}

/// The Add machine dialog, or the same for the machine `id` with what it has.
pub(super) fn open_machine(world: &mut World, id: Option<cadrs_org::Id>) {
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let org = open.session.org();
    let machine = id.and_then(|id| org.machines.get(&id)).cloned();
    let material_names = material_names(org);
    let locations: Vec<(cadrs_org::Id, String)> = org
        .locations
        .iter()
        .map(|(id, l)| (*id, l.name.clone()))
        .collect();
    // A new machine stands where things are made, when there is one such place.
    let mut made_at = org.locations.iter().filter(|(_, l)| l.production);
    let only = match (made_at.next(), made_at.next()) {
        (Some((id, _)), None) if machine.is_none() => Some(*id),
        _ => None,
    };
    let location_at = machine
        .as_ref()
        .and_then(|m| m.location)
        .or(only)
        .and_then(|l| locations.iter().position(|(id, _)| *id == l))
        .map_or(0, |i| i + 1);
    world.insert_resource(EditingMachine(id));
    let theme = world.resource::<Theme>().clone();
    let tb = theme.clone();
    let title = tr(if id.is_some() {
        "org-edit-machine"
    } else {
        "org-add-machine"
    });
    world.spawn((
        Dialog::new("org-machine-dialog")
            .title(title)
            .width(640.0)
            .body(move |b| {
                let t = &tb;
                let m = machine.as_ref();
                b.spawn(t_text(t, "org-machine-name"));
                b.spawn(
                    TextInput::new("org-machine-name")
                        .value(m.map_or(String::new(), |m| m.name.clone()))
                        .autofocus()
                        .build(t),
                );
                b.spawn(t_text(t, "org-machine-process"));
                let mut strip = TabStrip::new("org-machine-process").compact();
                for p in PROCESSES {
                    strip = strip.tab(process_name(p));
                }
                let at = m
                    .and_then(|m| PROCESSES.iter().position(|p| *p == m.process))
                    .unwrap_or(0);
                b.spawn(strip.selected(at).build(t));
                // What the size and the materials mean depends on the process (`on_process`
                // renames them when another is picked): a cutter's third number is its kerf,
                // a printer's or a mill's is how high it works.
                let process = m.map_or(PROCESSES[0], |m| m.process);
                let (size_key, materials_key) = field_keys(process);
                b.spawn((
                    Name::new("org-machine-size-label"),
                    MachineLabel::Size,
                    t_text(t, size_key),
                ));
                let (x, y, kerf) = m.map_or((3000.0, 1500.0, 1.0), |m| {
                    let third = if cuts_sheet(m.process) {
                        m.kerf
                    } else {
                        m.envelope[2]
                    };
                    (m.envelope[0], m.envelope[1], third)
                });
                b.spawn(Node {
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|r| {
                    for (name, value) in [
                        ("org-machine-x", x),
                        ("org-machine-y", y),
                        ("org-machine-kerf", kerf),
                    ] {
                        r.spawn(
                            TextInput::new(name)
                                .value(value.to_string())
                                .width(Val::Px(120.0))
                                .build(t),
                        );
                    }
                });
                b.spawn((
                    Name::new("org-machine-materials-label"),
                    MachineLabel::Materials,
                    t_text(t, materials_key),
                ));
                // The materials it works in, each picked from the Part Studio's materials (so
                // that a part's material and a machine's are the same name), with the
                // thickest it cuts where it cuts sheet.
                let listed = m.map_or(&[][..], |m| &m.materials[..]);
                b.spawn((
                    Name::new("org-machine-materials"),
                    MachineMaterials(listed.len() as u32),
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        ..default()
                    },
                ))
                .with_children(|l| {
                    for (key, limit) in listed.iter().enumerate() {
                        l.spawn(material_row(
                            t,
                            &limit.material,
                            key as u32,
                            limit.max_thickness,
                            cuts_sheet(process),
                        ));
                    }
                });
                b.spawn(
                    material_picker(MATERIAL_PICKER, material_names)
                        .placeholder(tr("org-machine-add-material"))
                        .adds()
                        .build(t),
                );
                b.spawn(t_text(t, "org-machine-location"));
                let places = std::iter::once(tr("org-machine-no-location"))
                    .chain(locations.iter().map(|(_, name)| name.clone()));
                b.spawn(choice(t, "org-machine-location", places, location_at));
            })
            .footer(|f| dialog_buttons(f, "org-machine", save_machine))
            .build(&theme),
        OrgDialog,
        DespawnOnExit(AppState::Organization),
    ));
    world.flush();
}

pub(super) fn save_machine(world: &mut World) {
    let Some(EditingMachine(editing)) = world.remove_resource::<EditingMachine>() else {
        return;
    };
    let retry = |world: &mut World, why: &str| {
        crate::linked::toast(world, tr(why));
        world.insert_resource(EditingMachine(editing));
    };
    let name = field(world, "org-machine-name");
    if name.trim().is_empty() {
        return retry(world, "org-machine-needs-name");
    }
    let process = dialog_process(world);
    let (Some(x), Some(y)) = (
        number(world, "org-machine-x").filter(|v| *v > 0.0),
        number(world, "org-machine-y").filter(|v| *v > 0.0),
    ) else {
        return retry(world, "org-machine-needs-bed");
    };
    // The third number: a cutter's kerf, or how high a printer, a mill or a lathe works.
    let third = number(world, "org-machine-kerf").unwrap_or(0.0).max(0.0);
    if !cuts_sheet(process) && third <= 0.0 {
        return retry(world, "org-machine-needs-height");
    }
    let mut limits = dialog_materials(world, cuts_sheet(process));
    let location_at = strip_selected(world, "org-machine-location");
    let Some(open) = world.get_resource::<OpenOrg>() else {
        return;
    };
    let org = open.session.org();
    let location = location_at
        .checked_sub(1)
        .and_then(|i| org.locations.keys().nth(i).copied());
    let before = editing.and_then(|id| org.machines.get(&id));
    // What the dialog doesn't ask for stays as it was (a machine's own speeds, its tolerance).
    if let Some(before) = before {
        for limit in &mut limits {
            if let Some(kept) = before
                .materials
                .iter()
                .find(|l| l.material.eq_ignore_ascii_case(&limit.material))
            {
                limit.speeds.clone_from(&kept.speeds);
            }
        }
    }
    let machine = MachineCaps {
        name: name.trim().to_owned(),
        process,
        envelope: if cuts_sheet(process) {
            [x, y, before.map_or(0.0, |m| m.envelope[2])]
        } else {
            [x, y, third]
        },
        materials: limits,
        kerf: if cuts_sheet(process) {
            third
        } else {
            before.map_or(0.0, |m| m.kerf)
        },
        min_hole_ratio: before.map_or(if process == Process::Plasma { 1.5 } else { 0.5 }, |m| {
            m.min_hole_ratio
        }),
        tolerance: before.map_or(0.1, |m| m.tolerance),
        pierce_seconds: before.map_or(2.0, |m| m.pierce_seconds),
        location,
    };
    let op = Op::SetMachine {
        id: editing.unwrap_or_else(new_id),
        machine: Some(machine),
    };
    if write(world, op) {
        close_dialogs(world);
    } else {
        world.insert_resource(EditingMachine(editing));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog_world() -> World {
        let mut world = World::new();
        world.insert_resource(Theme::default());
        world.spawn((MachineMaterials::default(), Node::default()));
        world
    }

    /// What the dropdown picks joins the machine's materials once, in the order picked, under
    /// the name it was picked by: "petg" typed finds PETG, and PETG is what is kept.
    #[test]
    fn picked_materials_are_listed_once_in_order() {
        let mut world = dialog_world();
        for name in ["PETG", "petg", "  PLA ", "", "Nylon 6/6", "pla"] {
            add_machine_material(&mut world, name);
        }
        // A printer's are names: no thickness is asked for or kept.
        let listed = dialog_materials(&mut world, false);
        let names: Vec<&str> = listed.iter().map(|l| l.material.as_str()).collect();
        assert_eq!(names, ["PETG", "PLA", "Nylon 6/6"]);
        assert!(listed.iter().all(|l| l.max_thickness == 0.0));
        assert_eq!(listed[0].label(), "PETG");
        // Each row has a number of its own, so a removed row's is not given again.
        let mut rows = world.query::<(Entity, &MachineMaterial)>();
        let keys: Vec<u32> = rows.iter(&world).map(|(_, m)| m.key).collect();
        assert_eq!(keys, [0, 1, 2]);
        let pla = rows
            .iter(&world)
            .find(|(_, m)| m.name == "PLA")
            .map(|(row, _)| row)
            .unwrap();
        world.entity_mut(pla).despawn();
        add_machine_material(&mut world, "ABS");
        let listed = dialog_materials(&mut world, true);
        let names: Vec<&str> = listed.iter().map(|l| l.material.as_str()).collect();
        assert_eq!(names, ["PETG", "Nylon 6/6", "ABS"]);
        let mut rows = world.query::<&MachineMaterial>();
        assert_eq!(rows.iter(&world).map(|m| m.key).max(), Some(3));
        // A sheet cutter with no thickness typed says none: the material is kept all the same.
        assert!(listed.iter().all(|l| l.max_thickness == 0.0));
    }

    /// A name of any length typed in is cut to a label's.
    #[test]
    fn a_typed_material_is_a_label() {
        let mut world = dialog_world();
        add_machine_material(&mut world, &"x".repeat(5000));
        let listed = dialog_materials(&mut world, false);
        assert_eq!(listed[0].material.chars().count(), cadrs_org::MAX_LABEL);
    }

    /// The materials to pick from are the Part Studio's, then what the organization names
    /// already and they don't have, once each whatever the case.
    #[test]
    fn the_materials_to_pick_from_are_the_part_studios_first() {
        let mut org = cadrs_org::Org::default();
        let library = cadrs_core::material::LIBRARY;
        assert_eq!(material_names(&org).len(), library.len());
        for (id, name) in [(1, "glass"), (2, "petg"), (3, "Glass"), (4, "Steel - A36")] {
            org.materials.insert(
                id,
                cadrs_org::Material {
                    name: name.into(),
                    thickness: 3.0,
                    sheet: [2500.0, 1250.0],
                },
            );
        }
        let names = material_names(&org);
        assert_eq!(names[0], library[0].name);
        assert!(
            names.iter().any(|n| n == "PETG"),
            "PETG is a library material"
        );
        assert_eq!(names.len(), library.len() + 1, "{names:?}");
        assert_eq!(names.last().map(String::as_str), Some("glass"));
    }
}
