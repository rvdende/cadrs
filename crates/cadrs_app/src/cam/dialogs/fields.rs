//! The dialogs' fields: what each [`Draft`] shows, laid out with [`form`].

use super::*;

pub(super) const ORIGINS: [&str; 4] = ["World", "Component box point", "Stock box point", "Manual"];
pub(super) const BOX_AXIS: [&str; 3] = ["Minimum", "Middle", "Maximum"];
/// The axis directions offered for a work plane's Z and X, and a stock's direction.
pub(super) const AXES: [(&str, [f64; 3]); 6] = [
    ("+Z", [0.0, 0.0, 1.0]),
    ("−Z", [0.0, 0.0, -1.0]),
    ("+X", [1.0, 0.0, 0.0]),
    ("−X", [-1.0, 0.0, 0.0]),
    ("+Y", [0.0, 1.0, 0.0]),
    ("−Y", [0.0, -1.0, 0.0]),
];

pub(super) fn axis_index(v: [f64; 3]) -> Option<usize> {
    AXES.iter()
        .position(|(_, a)| (a[0] - v[0]).abs() + (a[1] - v[1]).abs() + (a[2] - v[2]).abs() < 1e-6)
}

pub(super) fn index_of<T: PartialEq + Copy>(all: &[T], v: T) -> usize {
    all.iter().position(|x| *x == v).unwrap_or(0)
}

pub(super) fn labels<T: Copy>(all: &[T], f: impl Fn(T) -> &'static str) -> Vec<String> {
    all.iter().map(|x| f(*x).to_string()).collect()
}

/// A work plane's fields (`p` prefixes the keys).
pub(super) fn plane_fields(out: &mut Vec<Field>, w: &WorkPlane, picking: bool, with_type: bool) {
    out.push(Field::section("Origin"));
    let o = match w.origin {
        Origin::World => 0,
        Origin::ComponentBox(_) => 1,
        Origin::StockBox(_) => 2,
        Origin::Manual(_) => 3,
    };
    out.push(Field::select("p-origin", "Work plane origin", ORIGINS, o));
    match w.origin {
        Origin::ComponentBox(bp) | Origin::StockBox(bp) => {
            for (i, a) in ["X", "Y", "Z"].iter().enumerate() {
                out.push(Field::select(
                    &format!("p-box-{i}"),
                    &format!("Position {a}"),
                    BOX_AXIS,
                    (bp[i] + 1).clamp(0, 2) as usize,
                ));
            }
        }
        Origin::Manual(p) => {
            for (i, a) in ["X", "Y", "Z"].iter().enumerate() {
                out.push(Field::number(&format!("p-man-{i}"), a, p[i], Unit::Mm));
            }
        }
        Origin::World => {}
    }
    out.push(Field::section("Orientation"));
    let zi = axis_index(w.z);
    let mut zs: Vec<String> = AXES.iter().map(|(n, _)| format!("Z along {n}")).collect();
    zs.push("Normal of a picked face".into());
    out.push(Field::select(
        "p-z",
        "Tool axis (Z)",
        zs,
        zi.unwrap_or(AXES.len()),
    ));
    if zi.is_none() || picking {
        out.push(Field::picks(
            "p-face",
            "Planar face for Z",
            if zi.is_none() {
                vec![format!("Z ({:.3}, {:.3}, {:.3})", w.z[0], w.z[1], w.z[2])]
            } else {
                Vec::new()
            },
            picking,
        ));
    }
    let xs: Vec<String> = AXES.iter().map(|(n, _)| format!("X along {n}")).collect();
    out.push(Field::select(
        "p-x",
        "X direction",
        xs,
        axis_index(w.x).unwrap_or(2),
    ));
    if with_type {
        out.push(Field::select(
            "p-type",
            "Type",
            labels(&WorkPlaneType::ALL, WorkPlaneType::label),
            index_of(&WorkPlaneType::ALL, w.kind),
        ));
    }
    let mut datums = vec!["Automatic".to_string()];
    datums.extend((1..=6).map(cadrs_core::cam::cadrs_cam::post::datum_code));
    out.push(Field::select(
        "p-datum",
        "Override datum",
        datums,
        w.datum.map_or(0, |d| d.min(6) as usize),
    ));
    out.push(Field::check(
        "p-triad",
        "Show work coordinate system",
        w.show_triad,
    ));
}

pub(super) fn set_plane(w: &mut WorkPlane, key: &str, v: &Value) -> bool {
    match (key, v) {
        ("p-origin", Value::Index(i)) => {
            w.origin = match i {
                0 => Origin::World,
                1 => Origin::ComponentBox([0, 0, 1]),
                2 => Origin::StockBox([0, 0, 1]),
                _ => Origin::Manual([0.0; 3]),
            }
        }
        (k, Value::Index(i)) if k.starts_with("p-box-") => {
            let a: usize = k[6..].parse().unwrap_or(0);
            let val = *i as i8 - 1;
            match &mut w.origin {
                Origin::ComponentBox(bp) | Origin::StockBox(bp) => bp[a] = val,
                _ => {}
            }
        }
        (k, Value::Number(n)) if k.starts_with("p-man-") => {
            let a: usize = k[6..].parse().unwrap_or(0);
            if let Origin::Manual(p) = &mut w.origin {
                p[a] = *n;
            }
        }
        ("p-z", Value::Index(i)) => {
            if let Some((_, a)) = AXES.get(*i) {
                w.z = *a;
                // Keep X across Z.
                if (w.z[0] * w.x[0] + w.z[1] * w.x[1] + w.z[2] * w.x[2]).abs() > 0.5 {
                    w.x = if w.z[0].abs() > 0.5 {
                        [0.0, 1.0, 0.0]
                    } else {
                        [1.0, 0.0, 0.0]
                    };
                }
            } else {
                return false;
            }
        }
        ("p-x", Value::Index(i)) => {
            if let Some((_, a)) = AXES.get(*i) {
                w.x = *a;
            }
        }
        ("p-type", Value::Index(i)) => w.kind = WorkPlaneType::ALL[(*i).min(2)],
        ("p-datum", Value::Index(i)) => w.datum = (*i > 0).then_some(*i as u32),
        ("p-triad", Value::Bool(b)) => w.show_triad = *b,
        _ => return false,
    }
    true
}

pub(super) fn stock_direction_options() -> Vec<String> {
    let mut v = vec!["World".to_string()];
    v.extend(AXES.iter().skip(1).map(|(n, _)| format!("Z along {n}")));
    v
}

pub(super) fn stock_direction_index(d: StockDirection) -> usize {
    match d {
        StockDirection::World => 0,
        StockDirection::Axes { z, .. } => axis_index(z).filter(|i| *i > 0).unwrap_or(0),
    }
}

impl Draft {
    /// The dialog's title.
    pub fn title(&self) -> String {
        match self {
            Draft::Insert(d) => format!("Insert {}", d.role.label().to_lowercase()),
            Draft::Component(c) => c.name.clone(),
            Draft::Job { job, .. } => job.name.clone(),
            Draft::Machine { .. } => "Machine".into(),
            Draft::Setup { setup, .. } => setup.name.clone(),
            Draft::Tool { op, .. } | Draft::Toolpath { op, .. } | Draft::WorkPlane { op, .. } => {
                op.name.clone()
            }
        }
    }

    /// What's wrong with it (✓ is off while there is).
    pub fn problem(&self, ctx: &Ctx) -> Option<String> {
        match self {
            Draft::Insert(d) => (!d.parts.iter().any(|p| p.2)).then(|| "Tick a part".into()),
            Draft::Job { job, .. } => {
                if job.name.trim().is_empty() {
                    Some("The job needs a name".into())
                } else if cadrs_core::cam::program::job_bodies(ctx.cam, job, ctx.parts).is_empty() {
                    Some("Pick the bodies to machine (or insert a part)".into())
                } else {
                    None
                }
            }
            Draft::Machine { machine, .. } => machine.check().err(),
            Draft::Tool { op, .. } => match &op.kind {
                OpKind::Tool(t) => t.tool.check().err(),
                _ => None,
            },
            Draft::Toolpath { op, setup, .. } => match &op.kind {
                OpKind::Toolpath(tp) => {
                    let tool = tool_above(ctx.cam, *setup, op.id);
                    match tool {
                        None => Some("No tool above the toolpath".into()),
                        Some(t) => tp.params.check(&t).err().or_else(|| {
                            let p = &tp.params;
                            let needs_edges =
                                matches!(p.pattern, Pattern::Engrave) && p.drive.edges.is_empty();
                            let needs_faces =
                                p.pattern == Pattern::Fill && p.drive.faces.is_empty();
                            if needs_edges {
                                Some("Pick the edges to engrave".into())
                            } else if needs_faces {
                                Some("Pick the faces to fill".into())
                            } else {
                                None
                            }
                        }),
                    }
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// The dialog's fields.
    pub fn fields(&self, ctx: &Ctx, picking: Option<&str>, tab: usize) -> Vec<Field> {
        let mut f = Vec::new();
        match self {
            Draft::Insert(d) => {
                f.push(Field::select(
                    "role",
                    "Insert as",
                    labels(&Role::ALL, Role::label),
                    index_of(&Role::ALL, d.role),
                ));
                if !d.others.is_empty() {
                    let mut docs = vec!["This document".to_string()];
                    docs.extend(d.others.iter().map(|o| o.1.clone()));
                    f.push(Field::select("document", "Document", docs, d.document));
                }
                match &d.other {
                    Some(o) if d.document > 0 => {
                        f.push(Field::select(
                            "studio",
                            "Part Studio",
                            o.studios.iter().map(|s| s.1.clone()),
                            d.studio,
                        ));
                        f.push(Field::select(
                            "version",
                            "Version",
                            o.versions.iter().map(|v| v.1.clone()),
                            d.version,
                        ));
                        f.push(Field::note(
                            "Parts from another document come in at a version: Update brings a \
                             newer one.",
                        ));
                    }
                    _ => {
                        let studios = ctx.part_studios();
                        f.push(Field::select(
                            "studio",
                            "Part Studio",
                            studios.iter().map(|s| s.1.clone()),
                            d.studio,
                        ));
                        let mut refs = vec!["Workspace (follows edits)".to_string()];
                        refs.extend(ctx.versions.iter().map(|v| format!("Version {}", v.0)));
                        f.push(Field::select("version", "Reference", refs, d.version));
                        if d.version == 0 {
                            f.push(Field::note(
                                "A version keeps the part as it is; the workspace follows every \
                                 edit.",
                            ));
                        }
                    }
                }
                f.push(Field::section("Parts"));
                if d.parts.is_empty() {
                    f.push(Field::error("The Part Studio has no parts"));
                }
                for (i, (_, name, on)) in d.parts.iter().enumerate() {
                    f.push(Field::check(&format!("part-{i}"), name, *on));
                }
            }
            Draft::Component(c) => {
                f.push(Field::text("name", "Name", &c.name));
                f.push(Field::select(
                    "role",
                    "Used as",
                    labels(&Role::ALL, Role::label),
                    index_of(&Role::ALL, c.role),
                ));
                f.push(Field::note(format!("Reference: {}", c.reference_label())));
            }
            Draft::Job { .. } => self.job_fields(&mut f, ctx, picking),
            Draft::Machine { .. } => self.machine_fields(&mut f),
            Draft::Setup { setup, job, .. } => {
                f.push(Field::text("name", "Setup name", &setup.name));
                f.push(Field::count(
                    "program",
                    "Program number",
                    setup.program_number as i64,
                ));
                let man = matches!(setup.mating, Mating::Manual(_));
                f.push(Field::select(
                    "mating",
                    "Machine mating",
                    ["Automatic", "Manual"],
                    usize::from(man),
                ));
                if let Mating::Manual(o) = setup.mating {
                    for (i, a) in ["Offset X", "Offset Y", "Offset Z"].iter().enumerate() {
                        f.push(Field::number(&format!("mate-{i}"), a, o[i], Unit::Mm));
                    }
                }
                plane_fields(&mut f, &setup.wcs, picking == Some("p-face"), false);
                if ctx
                    .cam
                    .job(*job)
                    .and_then(|j| j.machine.as_ref())
                    .is_some_and(|m| m.kind.is_cutting())
                    && setup.wcs.z != [0.0, 0.0, 1.0]
                {
                    f.push(Field::note(
                        "A cutting machine cuts down its Z: lay the sheet flat.",
                    ));
                }
            }
            Draft::WorkPlane { op, .. } => {
                f.push(Field::text("name", "Name", &op.name));
                if let OpKind::WorkPlane(w) = &op.kind {
                    plane_fields(&mut f, w, picking == Some("p-face"), true);
                }
            }
            Draft::Tool { .. } => self.tool_fields(&mut f, ctx),
            Draft::Toolpath { .. } => self.toolpath_fields(&mut f, ctx, picking, tab),
        }
        if let Some(e) = self.problem(ctx) {
            f.push(Field::error(e));
        }
        f
    }

    /// The Job draft's fields.
    fn job_fields(&self, f: &mut Vec<Field>, ctx: &Ctx, picking: Option<&str>) {
        let Draft::Job { job, .. } = self else {
            return;
        };
        f.push(Field::text("name", "Job name", &job.name));
        f.push(Field::picks(
            "bodies",
            "Bodies (all parts when empty)",
            job.bodies.iter().map(|p| ctx.part_name(*p)).collect(),
            picking == Some("bodies"),
        ));
        f.push(Field::section("Stock"));
        let stocks: Vec<&cadrs_core::cam::Component> = ctx
            .cam
            .components
            .iter()
            .filter(|c| c.role == Role::Stock)
            .collect();
        let kinds = ["Box", "Cylinder", "User defined"];
        let k = match job.stock.kind {
            StockKind::Box => 0,
            StockKind::Cylinder => 1,
            StockKind::Component(_) => 2,
        };
        f.push(Field::select("stock-kind", "Stock type", kinds, k));
        match job.stock.kind {
            StockKind::Component(id) => {
                if stocks.is_empty() {
                    f.push(Field::error("Insert a stock component first"));
                } else {
                    f.push(Field::select(
                        "stock-comp",
                        "Stock body",
                        stocks.iter().map(|c| c.name.clone()),
                        stocks.iter().position(|c| c.id == id).unwrap_or(0),
                    ));
                }
            }
            _ => {
                f.push(Field::select(
                    "stock-dir",
                    "Stock direction",
                    stock_direction_options(),
                    stock_direction_index(job.stock.direction),
                ));
                if job.stock.kind == StockKind::Cylinder {
                    f.push(Field::number(
                        "radial",
                        "Radial offset",
                        job.stock.radial,
                        Unit::Mm,
                    ));
                    f.push(Field::number(
                        "off-4",
                        "Offset −Z",
                        job.stock.offsets[4],
                        Unit::Mm,
                    ));
                    f.push(Field::number(
                        "off-5",
                        "Offset +Z",
                        job.stock.offsets[5],
                        Unit::Mm,
                    ));
                } else {
                    for (i, n) in [
                        "Offset −X",
                        "Offset +X",
                        "Offset −Y",
                        "Offset +Y",
                        "Offset −Z",
                        "Offset +Z",
                    ]
                    .iter()
                    .enumerate()
                    {
                        f.push(Field::number(
                            &format!("off-{i}"),
                            n,
                            job.stock.offsets[i],
                            Unit::Mm,
                        ));
                    }
                }
                let bodies = cadrs_core::cam::program::job_bodies(ctx.cam, job, ctx.parts);
                if !bodies.is_empty()
                    && let Some(s) = cadrs_core::cam::program::stock_shape(ctx.cam, job, ctx.parts)
                {
                    let b = s.bounds_in(&job.stock.frame());
                    let z = b.size();
                    f.push(Field::note(format!(
                        "{} × {} × {} mm",
                        fmt(z[0]),
                        fmt(z[1]),
                        fmt(z[2])
                    )));
                }
            }
        }
        f.push(Field::section("Workholding"));
        f.push(Field::picks(
            "workholdings",
            "Workholding bodies",
            job.workholdings.iter().map(|p| ctx.part_name(*p)).collect(),
            picking == Some("workholdings"),
        ));
    }

    /// The Machine draft's fields.
    fn machine_fields(&self, f: &mut Vec<Field>) {
        let Draft::Machine {
            machine, by_kind, ..
        } = self
        else {
            return;
        };
        let cat = catalog();
        f.push(Field::check("by-kind", "Group by machine type", *by_kind));
        let mut order: Vec<usize> = (0..cat.len()).collect();
        if *by_kind {
            order.sort_by_key(|i| index_of(&MachineKind::ALL, cat[*i].kind));
        } else {
            order.sort_by_key(|i| cat[*i].title());
        }
        let opts: Vec<String> = order
            .iter()
            .map(|i| {
                if *by_kind {
                    format!("{}: {}", cat[*i].kind.label(), cat[*i].model)
                } else {
                    cat[*i].title()
                }
            })
            .collect();
        let current = order
            .iter()
            .position(|i| cat[*i].title() == machine.title())
            .unwrap_or(0);
        f.push(Field::select("machine", "Machine", opts, current));
        f.push(Field::note(format!(
            "{} · {} controller",
            machine.kind.label(),
            machine.dialect.label()
        )));
        f.push(Field::section("Machine"));
        for (i, a) in ["X", "Y", "Z"].iter().enumerate() {
            f.push(Field::number(
                &format!("travel-{i}"),
                &format!("{a} travel"),
                machine.travel[i],
                Unit::Mm,
            ));
        }
        f.push(Field::number(
            "rapid",
            "Rapid rate",
            machine.rapid_rate,
            Unit::Feed,
        ));
        f.push(Field::number(
            "max-feed",
            "Fastest feed",
            machine.max_feed,
            Unit::Feed,
        ));
        if machine.kind.is_milling() {
            f.push(Field::number(
                "max-spindle",
                "Fastest spindle",
                machine.max_spindle,
                Unit::Rpm,
            ));
            f.push(Field::check(
                "changer",
                "Automatic tool changer",
                machine.tool_changer,
            ));
            f.push(Field::check(
                "indexing",
                "Indexing rotary axes (3+2)",
                machine.indexing.is_some(),
            ));
            if let Some(ix) = machine.indexing {
                f.push(Field::number(
                    "max-tilt",
                    "Largest tilt",
                    ix.max_tilt,
                    Unit::Deg,
                ));
                f.push(Field::select(
                    "rotary",
                    "Rotary axes",
                    vec!["A and C (trunnion)", "B and C (trunnion)"],
                    usize::from(ix.rotary[0] == 'B'),
                ));
                if machine.dialect != cadrs_core::cam::cadrs_cam::Dialect::Fanuc {
                    f.push(Field::note(
                        "Tilted work planes are posted as Fanuc's G68.2 only",
                    ));
                }
            }
        } else if machine.kind == MachineKind::Laser {
            f.push(Field::number(
                "max-spindle",
                "S at full power",
                machine.max_spindle,
                Unit::None,
            ));
        }
        let s = &machine.post;
        f.push(Field::section("Post settings"));
        f.push(Field::select(
            "unit",
            "Output unit",
            ["Millimetres", "Inches"],
            usize::from(s.output_unit == LenUnit::Inch),
        ));
        f.push(Field::check(
            "adv-wp",
            "Advanced work planes (own datums)",
            s.advanced_work_planes,
        ));
        f.push(Field::check("seq", "Sequence numbers", s.sequence_numbers));
        if s.sequence_numbers {
            f.push(Field::count(
                "block-start",
                "Block number start",
                s.block_start as i64,
            ));
            f.push(Field::count(
                "block-inc",
                "Block increment",
                s.block_increment as i64,
            ));
            f.push(Field::check(
                "tools-only",
                "Numbers at tool changes only",
                s.numbers_at_tools_only,
            ));
        }
        f.push(Field::check(
            "rapid-pos",
            "Rapid positioning (G0)",
            s.rapid_positioning,
        ));
        if !s.rapid_positioning {
            f.push(Field::number(
                "post-rapid",
                "Rapid feed (G1)",
                s.rapid_rate,
                Unit::Feed,
            ));
        }
        f.push(Field::check("arcs", "Output arcs (G2/G3)", s.output_arcs));
        if s.output_arcs {
            f.push(Field::check("helices", "Output helices", s.output_helices));
        }
        f.push(Field::count(
            "decimals",
            "Decimal places",
            s.decimals as i64,
        ));
        f.push(Field::check(
            "dec-point",
            "Always write a decimal point",
            s.decimal_point,
        ));
        if machine.kind.is_milling() {
            f.push(Field::check(
                "ret-tool",
                "Retract on tool changes",
                s.force_retract_on_tool_change,
            ));
            f.push(Field::check(
                "ret-plane",
                "Retract on work plane changes",
                s.force_retract_on_plane_change,
            ));
            f.push(Field::check(
                "ret-end",
                "Retract at the program end",
                s.force_retract_on_end,
            ));
            if machine.dialect.has_cycles() {
                f.push(Field::section("Fixed cycles"));
                f.push(Field::check(
                    "cy-drill",
                    "Drilling canned cycles",
                    s.canned_drill,
                ));
                f.push(Field::check(
                    "cy-peck",
                    "Peck drilling canned cycles",
                    s.canned_peck,
                ));
                f.push(Field::check(
                    "cy-tap",
                    "Thread tapping canned cycles",
                    s.canned_tap,
                ));
                f.push(Field::check(
                    "cy-chip",
                    "Chip break canned cycles",
                    s.canned_chip_break,
                ));
            }
        }
        if machine.kind == MachineKind::Laser {
            f.push(Field::select(
                "laser-mode",
                "Laser power",
                ["Dynamic (M4)", "Constant (M3)"],
                usize::from(s.laser_mode == LaserMode::Constant),
            ));
        }
        f.push(Field::check(
            "comments",
            "Comments in the program",
            s.comments,
        ));
    }

    /// The Tool draft's fields.
    fn tool_fields(&self, f: &mut Vec<Field>, ctx: &Ctx) {
        let Draft::Tool {
            op,
            library,
            filter,
            setup,
            ..
        } = self
        else {
            return;
        };
        let OpKind::Tool(t) = &op.kind else {
            return;
        };
        let machine = machine_of(ctx.cam, *setup);
        let lib: Vec<(usize, &Tool)> = ctx
            .cam
            .library
            .filtered(*filter, None)
            .into_iter()
            .filter(|(_, x)| x.kind.fits(machine))
            .collect();
        f.push(Field::check("library", "From the tool library", *library));
        if *library {
            let mut types = vec!["All".to_string()];
            types.extend(labels(&MachiningType::ALL, MachiningType::label));
            f.push(Field::select(
                "lib-type",
                "Machining type",
                types,
                filter.map_or(0, |m| index_of(&MachiningType::ALL, m) + 1),
            ));
            if lib.is_empty() {
                f.push(Field::note("No tool of this type for this machine yet"));
            } else {
                let cur = lib.iter().position(|(_, x)| **x == t.tool);
                let mut opts: Vec<String> = lib.iter().map(|(_, x)| x.name.clone()).collect();
                if cur.is_none() {
                    opts.insert(0, "Choose a tool…".into());
                }
                f.push(Field::select("lib-tool", "Tool", opts, cur.unwrap_or(0)));
            }
            f.push(Field::button(
                "lib-import",
                "Import tool library…",
                "file-import",
            ));
            f.push(Field::button(
                "lib-export",
                "Export tool library…",
                "file-export",
            ));
        }
        let tool = &t.tool;
        f.push(Field::section("Cutter"));
        f.push(Field::text("name", "Name", &op.name));
        let kinds = ToolKind::for_machine(machine);
        f.push(Field::select(
            "kind",
            "Cutter type",
            labels(&kinds, ToolKind::label),
            index_of(&kinds, tool.kind),
        ));
        if tool.is_beam() {
            f.push(Field::number(
                "diameter",
                "Kerf width",
                tool.diameter,
                Unit::Mm,
            ));
        } else {
            f.push(Field::select(
                "unit",
                "Unit",
                ["Millimetres", "Inches"],
                usize::from(tool.unit == LenUnit::Inch),
            ));
            f.push(Field::select(
                "hand",
                "Tool hand",
                ["Right", "Left"],
                usize::from(tool.hand == Hand::Left),
            ));
            f.push(Field::number(
                "diameter",
                "Diameter",
                tool.diameter,
                Unit::Mm,
            ));
            match tool.kind {
                ToolKind::BullMill => {
                    f.push(Field::number(
                        "corner",
                        "Corner radius",
                        tool.corner_radius,
                        Unit::Mm,
                    ));
                }
                ToolKind::ChamferMill | ToolKind::TaperMill => {
                    f.push(Field::number(
                        "angle",
                        "Included angle",
                        tool.angle,
                        Unit::Deg,
                    ));
                    f.push(Field::number(
                        "tip",
                        "Tip diameter",
                        tool.tip_diameter,
                        Unit::Mm,
                    ));
                }
                ToolKind::Drill => {
                    f.push(Field::number("angle", "Point angle", tool.angle, Unit::Deg));
                }
                ToolKind::Tap | ToolKind::ThreadMill => {
                    f.push(Field::number("pitch", "Thread pitch", tool.pitch, Unit::Mm));
                }
                _ => {}
            }
            f.push(Field::number(
                "cut-len",
                "Cutting length",
                tool.cutting_length,
                Unit::Mm,
            ));
            f.push(Field::number(
                "overall",
                "Overall length",
                tool.overall_length,
                Unit::Mm,
            ));
            f.push(Field::count("flutes", "Flutes", tool.flutes as i64));
            f.push(Field::check(
                "center",
                "Center cutting",
                tool.center_cutting,
            ));
        }
        if tool.is_beam() {
            f.push(Field::section("Beam"));
            if tool.kind != ToolKind::Plasma {
                f.push(Field::number(
                    "power",
                    "Power",
                    tool.beam.power,
                    Unit::Percent,
                ));
            }
            f.push(Field::number(
                "pierce",
                "Pierce delay",
                tool.beam.pierce_delay,
                Unit::Seconds,
            ));
            if tool.kind == ToolKind::Plasma {
                f.push(Field::number(
                    "pierce-h",
                    "Pierce height",
                    tool.beam.pierce_height,
                    Unit::Mm,
                ));
                f.push(Field::check("thc", "Torch height control", tool.beam.thc));
                f.push(Field::count(
                    "material",
                    "Material number (0: as set)",
                    tool.beam.material as i64,
                ));
            }
            if tool.kind != ToolKind::Laser {
                f.push(Field::number(
                    "cut-h",
                    "Cut height",
                    tool.beam.cut_height,
                    Unit::Mm,
                ));
            } else {
                f.push(Field::check("air", "Air assist", tool.beam.air_assist));
                f.push(Field::count(
                    "beam-passes",
                    "Passes",
                    tool.beam.passes as i64,
                ));
            }
        }
        f.push(Field::section("Cutting data"));
        if !tool.is_beam() {
            let mut opts = vec!["Set by hand".to_string()];
            opts.extend(WorkMaterial::ALL.iter().map(|m| m.label().to_string()));
            let at = tool
                .cutting
                .material
                .and_then(|m| WorkMaterial::ALL.iter().position(|x| *x == m))
                .map_or(0, |i| i + 1);
            f.push(Field::select("work-material", "Worked out for", opts, at));
            f.push(Field::number(
                "rpm",
                "Spindle speed",
                tool.cutting.spindle_rpm,
                Unit::Rpm,
            ));
        }
        f.push(Field::number(
            "feed",
            "Feed rate",
            tool.cutting.feed,
            Unit::Feed,
        ));
        if !tool.is_beam() {
            f.push(Field::number(
                "plunge",
                "Plunge rate",
                tool.cutting.plunge_feed,
                Unit::Feed,
            ));
            f.push(Field::number(
                "stepdown",
                "Step down",
                tool.cutting.step_down,
                Unit::Mm,
            ));
            f.push(Field::number(
                "stepover",
                "Step over",
                tool.cutting.step_over,
                Unit::Mm,
            ));
            f.push(Field::section("Holder"));
            f.push(Field::check("holder", "Holder", tool.holder.is_some()));
            if let Some(h) = &tool.holder {
                let holders: Vec<String> = ctx
                    .cam
                    .library
                    .holders
                    .iter()
                    .map(|x| x.name.clone())
                    .collect();
                if !holders.is_empty() {
                    let mut opts = vec!["Cylinder".to_string()];
                    opts.extend(holders);
                    let cur = ctx
                        .cam
                        .library
                        .holders
                        .iter()
                        .position(|x| x == h)
                        .map_or(0, |i| i + 1);
                    f.push(Field::select("holder-lib", "Holder", opts, cur));
                }
                f.push(Field::number(
                    "holder-d",
                    "Holder diameter",
                    h.max_radius() * 2.0,
                    Unit::Mm,
                ));
                f.push(Field::number(
                    "holder-l",
                    "Holder length",
                    h.length(),
                    Unit::Mm,
                ));
            }
        }
        f.push(Field::section("Assembly"));
        f.push(Field::count(
            "number",
            "Assembly number (T)",
            t.number as i64,
        ));
    }

    /// The Toolpath draft's fields.
    fn toolpath_fields(&self, f: &mut Vec<Field>, ctx: &Ctx, picking: Option<&str>, tab: usize) {
        let Draft::Toolpath {
            op, kind, setup, ..
        } = self
        else {
            return;
        };
        let OpKind::Toolpath(tp) = &op.kind else {
            return;
        };
        let p = &tp.params;
        let machine = machine_of(ctx.cam, *setup);
        let tool = tool_above(ctx.cam, *setup, op.id);
        let tk = tool.as_ref().map_or(ToolKind::EndMill, |t| t.kind);
        let beam = tk.is_beam();
        let tabs: &[&str] = if beam {
            &["Tool path", "Link", "Misc", "Beam"]
        } else {
            &["Tool path", "Link", "Axis", "Collision", "Misc"]
        };
        let selected = tab.min(tabs.len() - 1);
        f.push(Field::Tabs {
            key: "tab".into(),
            labels: tabs.iter().map(|s| s.to_string()).collect(),
            selected,
        });
        match tabs[selected] {
            "Link" => link_fields(f, p, beam),
            "Axis" => axis_fields(f, ctx, *setup, p, picking),
            "Collision" => collision_fields(f, p, tool.as_ref()),
            "Misc" => {
                f.push(Field::section("Quality"));
                f.push(Field::number(
                    "tolerance",
                    "Cut tolerance",
                    p.tolerance,
                    Unit::Mm,
                ));
                if !beam {
                    f.push(Field::section("Smoothing"));
                    f.push(Field::number(
                        "smooth",
                        "Smooth corners",
                        p.smooth_corners,
                        Unit::Mm,
                    ));
                }
                repeat_fields(f, &p.repeat);
                f.push(Field::section("Template"));
                f.push(Field::button("save-template", "Save as template", "books"));
            }
            "Beam" => {
                f.push(Field::section("Beam"));
                f.push(Field::number(
                    "beam-power",
                    "Power",
                    p.beam.power,
                    Unit::Percent,
                ));
                f.push(Field::count("beam-passes", "Passes", p.beam.passes as i64));
                if p.beam.passes > 1 {
                    f.push(Field::number(
                        "pass-depth",
                        "Depth per pass",
                        p.beam.pass_depth,
                        Unit::Mm,
                    ));
                }
                if p.pattern == Pattern::Cut {
                    f.push(Field::count(
                        "micro",
                        "Microjoints per part",
                        p.beam.microjoints as i64,
                    ));
                    if p.beam.microjoints > 0 {
                        f.push(Field::number(
                            "micro-w",
                            "Microjoint width",
                            p.beam.microjoint_width,
                            Unit::Mm,
                        ));
                    }
                    f.push(Field::number(
                        "small-hole",
                        "Small holes under",
                        p.beam.small_hole,
                        Unit::Mm,
                    ));
                    if p.beam.small_hole > 0.0 {
                        f.push(Field::number(
                            "small-feed",
                            "Small hole feed",
                            p.beam.small_hole_feed * 100.0,
                            Unit::Percent,
                        ));
                    }
                }
            }
            _ => toolpath_fields(f, ctx, op, p, *kind, tk, machine, picking),
        }
        if tool.is_none() {
            f.push(Field::error("No tool above the toolpath"));
        }
    }
}

pub(super) fn fmt(v: f64) -> String {
    form::fmt_num(v, Unit::None)
}

/// The Z a toolpath's work plane gives it (model coordinates): the work plane above it, or
/// the setup's.
pub(super) fn op_axis(cam: &CamStudio, setup: CamId, op: CamId) -> [f64; 3] {
    cam.work_plane_for(op)
        .map(|(_, w)| w.z)
        .or_else(|| cam.setup(setup).map(|(_, s)| s.wcs.z))
        .unwrap_or([0.0, 0.0, 1.0])
}

/// A picked planar face's normal (model coordinates, unit).
pub(super) fn face_normal(ctx: &Ctx, pick: Pick) -> Option<[f64; 3]> {
    let Pick::Face(part, name) = pick else {
        return None;
    };
    let n = ctx
        .parts
        .iter()
        .find(|p| p.id == part)
        .and_then(|p| p.solid.faces.iter().find(|f| f.name == name))
        .and_then(|f| f.plane)
        .map(|pl| {
            let (u, v) = (pl.u, pl.v);
            [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ]
        })?;
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    (l > 1e-9).then(|| [n[0] / l, n[1] / l, n[2] / l])
}

/// The library's toolpath templates a `tk` tool on a `machine` can run.
pub(super) fn usable_templates(
    cam: &CamStudio,
    tk: ToolKind,
    machine: MachineKind,
) -> Vec<&Template> {
    cam.library
        .templates
        .iter()
        .filter(|t| {
            Pattern::available(t.params.pattern.type_of(), tk, machine).contains(&t.params.pattern)
        })
        .collect()
}

pub(super) fn machine_of(cam: &CamStudio, setup: CamId) -> MachineKind {
    cam.setup(setup)
        .and_then(|(j, _)| j.machine.as_ref())
        .map_or(MachineKind::Mill, |m| m.kind)
}

/// The tool a toolpath draft would use (the one above it, or above where it goes).
pub(super) fn tool_above(cam: &CamStudio, setup: CamId, op: CamId) -> Option<Tool> {
    if let Some(t) = cam.tool_for(op) {
        return Some(t.tool.clone());
    }
    cam.setup(setup).and_then(|(_, s)| {
        s.operations
            .iter()
            .rev()
            .filter(|o| !o.suppressed)
            .find_map(|o| o.tool().map(|t| t.tool.clone()))
    })
}

/// A drive entity's label: the face or edge and its part.
pub(super) fn entity_label(ctx: &Ctx, key: &str, what: &str) -> String {
    match pick_of_key(key).and_then(|p| p.part()) {
        Some(part) => format!("{what} of {}", ctx.part_name(part)),
        None => what.to_string(),
    }
}

/// The view pick a drive key names (for highlighting it).
pub fn pick_of_key(key: &str) -> Option<Pick> {
    let mut it = key.splitn(3, '/');
    let feature = it.next()?.parse::<uuid::Uuid>().ok()?;
    let index = it.next()?.parse::<u32>().ok()?;
    let rest = it.next()?;
    let part = PartId::new(cadrs_core::FeatureId(feature), index);
    if let Ok(f) = ron::from_str::<cadrs_core::solid::FaceName>(rest) {
        return Some(Pick::Face(part, f));
    }
    ron::from_str::<cadrs_core::solid::EdgeName>(rest)
        .ok()
        .map(|e| Pick::Edge(part, e))
}
