//! The dialogs' fields: what each [`Draft`] shows, laid out with [`form`].

use super::*;
use cadrs_ui::i18n::{tr, tr_with};

/// The work plane origins offered (catalog keys).
pub(super) const ORIGINS: [&str; 4] = [
    "cam-world",
    "cam-origin-component-box-point",
    "cam-origin-stock-box-point",
    "cam-manual",
];
/// Where on a box's axis a box point is (catalog keys).
pub(super) const BOX_AXIS: [&str; 3] = ["cam-box-minimum", "cam-box-middle", "cam-box-maximum"];
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
    out.push(Field::section(&tr("cam-section-origin")));
    let o = match w.origin {
        Origin::World => 0,
        Origin::ComponentBox(_) => 1,
        Origin::StockBox(_) => 2,
        Origin::Manual(_) => 3,
    };
    out.push(Field::select(
        "p-origin",
        &tr("cam-work-plane-origin"),
        ORIGINS.iter().map(|k| tr(k)),
        o,
    ));
    match w.origin {
        Origin::ComponentBox(bp) | Origin::StockBox(bp) => {
            for (i, a) in ["X", "Y", "Z"].iter().enumerate() {
                out.push(Field::select(
                    &format!("p-box-{i}"),
                    &tr_with("cam-position-axis", &[("axis", a)]),
                    BOX_AXIS.iter().map(|k| tr(k)),
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
    out.push(Field::section(&tr("cam-section-orientation")));
    let zi = axis_index(w.z);
    let mut zs: Vec<String> = AXES
        .iter()
        .map(|(n, _)| tr_with("cam-z-along", &[("axis", n)]))
        .collect();
    zs.push(tr("cam-normal-of-picked-face"));
    out.push(Field::select(
        "p-z",
        &tr("cam-tool-axis-z"),
        zs,
        zi.unwrap_or(AXES.len()),
    ));
    if zi.is_none() || picking {
        out.push(Field::picks(
            "p-face",
            &tr("cam-planar-face-for-z"),
            if zi.is_none() {
                vec![format!("Z ({:.3}, {:.3}, {:.3})", w.z[0], w.z[1], w.z[2])]
            } else {
                Vec::new()
            },
            picking,
        ));
    }
    let xs: Vec<String> = AXES
        .iter()
        .map(|(n, _)| tr_with("cam-x-along", &[("axis", n)]))
        .collect();
    out.push(Field::select(
        "p-x",
        &tr("cam-x-direction"),
        xs,
        axis_index(w.x).unwrap_or(2),
    ));
    if with_type {
        out.push(Field::select(
            "p-type",
            &tr("common-type"),
            labels(&WorkPlaneType::ALL, WorkPlaneType::label),
            index_of(&WorkPlaneType::ALL, w.kind),
        ));
    }
    let mut datums = vec![tr("cam-automatic")];
    datums.extend((1..=6).map(cadrs_core::cam::cadrs_cam::post::datum_code));
    out.push(Field::select(
        "p-datum",
        &tr("cam-override-datum"),
        datums,
        w.datum.map_or(0, |d| d.min(6) as usize),
    ));
    out.push(Field::check(
        "p-triad",
        &tr("cam-show-work-coordinate-system"),
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
    let mut v = vec![tr("cam-world")];
    v.extend(
        AXES.iter()
            .skip(1)
            .map(|(n, _)| tr_with("cam-z-along", &[("axis", n)])),
    );
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
            Draft::Insert(d) => tr(match d.role {
                Role::Part => "cam-insert-part",
                Role::Workholding => "cam-insert-workholding",
                Role::Stock => "cam-insert-stock",
            }),
            Draft::Component(c) => c.name.clone(),
            Draft::Job { job, .. } => job.name.clone(),
            Draft::Machine { .. } => tr("cam-machine"),
            Draft::Setup { setup, .. } => setup.name.clone(),
            Draft::Tool { op, .. } | Draft::Toolpath { op, .. } | Draft::WorkPlane { op, .. } => {
                op.name.clone()
            }
        }
    }

    /// What's wrong with it (✓ is off while there is).
    pub fn problem(&self, ctx: &Ctx) -> Option<String> {
        match self {
            Draft::Insert(d) => (!d.parts.iter().any(|p| p.2)).then(|| tr("cam-tick-a-part")),
            Draft::Job { job, .. } => {
                if job.name.trim().is_empty() {
                    Some(tr("cam-job-needs-a-name"))
                } else if cadrs_core::cam::program::job_bodies(ctx.cam, job, ctx.parts).is_empty() {
                    Some(tr("cam-pick-bodies-to-machine"))
                } else {
                    None
                }
            }
            Draft::Machine {
                from_assembly: true,
                source,
                other,
                ..
            } => match source {
                None if other.document > 0 => Some(tr("cam-machine-none-in-document")),
                None => Some(tr("cam-machine-no-assemblies")),
                // The assembly's own errors keep it from being a machine.
                Some(s) => other.machine(ctx.doc, s).err().map(|issues| {
                    issues
                        .iter()
                        .find(|i| i.is_error())
                        .or(issues.first())
                        .map_or_else(|| tr("cam-machine-not-set-up"), |i| i.text.clone())
                }),
            },
            Draft::Machine { machine, .. } => machine.check().err(),
            Draft::Tool { op, .. } => match &op.kind {
                OpKind::Tool(t) => t.tool.check().err(),
                _ => None,
            },
            Draft::Toolpath { op, setup, .. } => match &op.kind {
                OpKind::Toolpath(tp) => {
                    let tool = tool_above(ctx.cam, *setup, op.id);
                    match tool {
                        None => Some(tr("cam-no-tool-above-toolpath")),
                        Some(t) => tp.params.check(&t).err().or_else(|| {
                            let p = &tp.params;
                            let needs_edges =
                                matches!(p.pattern, Pattern::Engrave) && p.drive.edges.is_empty();
                            let needs_faces =
                                p.pattern == Pattern::Fill && p.drive.faces.is_empty();
                            if needs_edges {
                                Some(tr("cam-pick-edges-to-engrave"))
                            } else if needs_faces {
                                Some(tr("cam-pick-faces-to-fill"))
                            } else {
                                rotary_problem(p)
                            }
                        }),
                    }
                }
                _ => None,
            },
            // A mount frame with no Z says nothing of how the work sits.
            Draft::Setup { setup, .. } => match setup.mating {
                Mating::Mount { z, .. } if z.iter().map(|c| c * c).sum::<f64>() < 1e-18 => {
                    Some(tr("cam-mount-needs-z"))
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
                    &tr("cam-insert-as"),
                    labels(&Role::ALL, Role::label),
                    index_of(&Role::ALL, d.role),
                ));
                if !d.others.is_empty() {
                    let mut docs = vec![tr("cam-this-document")];
                    docs.extend(d.others.iter().map(|o| o.1.clone()));
                    f.push(Field::select(
                        "document",
                        &tr("cam-document"),
                        docs,
                        d.document,
                    ));
                }
                match &d.other {
                    Some(o) if d.document > 0 => {
                        f.push(Field::select(
                            "studio",
                            &tr("common-part-studio"),
                            o.studios.iter().map(|s| s.1.clone()),
                            d.studio,
                        ));
                        f.push(Field::select(
                            "version",
                            &tr("common-version"),
                            o.versions.iter().map(|v| v.1.clone()),
                            d.version,
                        ));
                        f.push(Field::note(tr("cam-insert-other-document-note")));
                    }
                    _ => {
                        let studios = ctx.part_studios();
                        f.push(Field::select(
                            "studio",
                            &tr("common-part-studio"),
                            studios.iter().map(|s| s.1.clone()),
                            d.studio,
                        ));
                        let mut refs = vec![tr("cam-workspace-follows-edits")];
                        refs.extend(
                            ctx.versions
                                .iter()
                                .map(|v| tr_with("cam-version-named", &[("name", &v.0)])),
                        );
                        f.push(Field::select(
                            "version",
                            &tr("cam-reference"),
                            refs,
                            d.version,
                        ));
                        if d.version == 0 {
                            f.push(Field::note(tr("cam-insert-workspace-note")));
                        }
                    }
                }
                f.push(Field::section(&tr("parts")));
                if d.parts.is_empty() {
                    f.push(Field::error(tr("cam-part-studio-has-no-parts")));
                }
                for (i, (_, name, on)) in d.parts.iter().enumerate() {
                    f.push(Field::check(&format!("part-{i}"), name, *on));
                }
            }
            Draft::Component(c) => {
                f.push(Field::text("name", &tr("common-name"), &c.name));
                f.push(Field::select(
                    "role",
                    &tr("cam-used-as"),
                    labels(&Role::ALL, Role::label),
                    index_of(&Role::ALL, c.role),
                ));
                f.push(Field::note(tr_with(
                    "cam-component-reference",
                    &[("reference", &c.reference_label())],
                )));
            }
            Draft::Job { .. } => self.job_fields(&mut f, ctx, picking),
            Draft::Machine { .. } => self.machine_fields(&mut f, ctx),
            Draft::Setup { setup, job, .. } => {
                f.push(Field::text("name", &tr("cam-setup-name"), &setup.name));
                f.push(Field::count(
                    "program",
                    &tr("cam-program-number"),
                    setup.program_number as i64,
                ));
                f.push(Field::select(
                    "mating",
                    &tr("cam-machine-mating"),
                    [
                        tr("cam-automatic"),
                        tr("cam-manual"),
                        tr("cam-mating-mount"),
                    ],
                    match setup.mating {
                        Mating::Automatic => 0,
                        Mating::Manual(_) => 1,
                        Mating::Mount { .. } => 2,
                    },
                ));
                match setup.mating {
                    Mating::Automatic => {}
                    Mating::Manual(o) => {
                        for (i, a) in ["cam-offset-x", "cam-offset-y", "cam-offset-z"]
                            .iter()
                            .enumerate()
                        {
                            f.push(Field::number(&format!("mate-{i}"), &tr(a), o[i], Unit::Mm));
                        }
                    }
                    Mating::Mount { origin, z, x } => {
                        f.push(Field::picks(
                            "mount-face",
                            &tr("cam-mount-face"),
                            Vec::new(),
                            picking == Some("mount-face"),
                        ));
                        // (Each row: its keys' stem, its label's catalog key, the vector.)
                        for (k, label, v, unit) in [
                            ("mount-o", "cam-mount-origin", origin, Unit::Mm),
                            ("mount-z", "cam-mount-z", z, Unit::None),
                            ("mount-x", "cam-mount-x", x, Unit::None),
                        ] {
                            for (i, a) in ["X", "Y", "Z"].iter().enumerate() {
                                f.push(Field::number(
                                    &format!("{k}-{i}"),
                                    &tr_with(label, &[("axis", a)]),
                                    v[i],
                                    unit,
                                ));
                            }
                        }
                        f.push(Field::note(tr("cam-mount-note")));
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
                    f.push(Field::note(tr("cam-cutting-machine-lay-flat")));
                }
            }
            Draft::WorkPlane { op, .. } => {
                f.push(Field::text("name", &tr("common-name"), &op.name));
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
        f.push(Field::text("name", &tr("cam-job-name"), &job.name));
        f.push(Field::picks(
            "bodies",
            &tr("cam-bodies-all-when-empty"),
            job.bodies.iter().map(|p| ctx.part_name(*p)).collect(),
            picking == Some("bodies"),
        ));
        f.push(Field::section(&tr("cam-stock")));
        let stocks: Vec<&cadrs_core::cam::Component> = ctx
            .cam
            .components
            .iter()
            .filter(|c| c.role == Role::Stock)
            .collect();
        let kinds = [tr("cam-box"), tr("cam-cylinder"), tr("cam-user-defined")];
        let k = match job.stock.kind {
            StockKind::Box => 0,
            StockKind::Cylinder => 1,
            StockKind::Component(_) => 2,
        };
        f.push(Field::select("stock-kind", &tr("cam-stock-type"), kinds, k));
        match job.stock.kind {
            StockKind::Component(id) => {
                if stocks.is_empty() {
                    f.push(Field::error(tr("cam-insert-stock-component-first")));
                } else {
                    f.push(Field::select(
                        "stock-comp",
                        &tr("cam-stock-body"),
                        stocks.iter().map(|c| c.name.clone()),
                        stocks.iter().position(|c| c.id == id).unwrap_or(0),
                    ));
                }
            }
            _ => {
                f.push(Field::select(
                    "stock-dir",
                    &tr("cam-stock-direction"),
                    stock_direction_options(),
                    stock_direction_index(job.stock.direction),
                ));
                if job.stock.kind == StockKind::Cylinder {
                    f.push(Field::number(
                        "radial",
                        &tr("cam-radial-offset"),
                        job.stock.radial,
                        Unit::Mm,
                    ));
                    f.push(Field::number(
                        "off-4",
                        &tr("cam-offset-minus-z"),
                        job.stock.offsets[4],
                        Unit::Mm,
                    ));
                    f.push(Field::number(
                        "off-5",
                        &tr("cam-offset-plus-z"),
                        job.stock.offsets[5],
                        Unit::Mm,
                    ));
                } else {
                    for (i, n) in [
                        "cam-offset-minus-x",
                        "cam-offset-plus-x",
                        "cam-offset-minus-y",
                        "cam-offset-plus-y",
                        "cam-offset-minus-z",
                        "cam-offset-plus-z",
                    ]
                    .iter()
                    .enumerate()
                    {
                        f.push(Field::number(
                            &format!("off-{i}"),
                            &tr(n),
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
        f.push(Field::section(&tr("cam-workholding")));
        f.push(Field::picks(
            "workholdings",
            &tr("cam-workholding-bodies"),
            job.workholdings.iter().map(|p| ctx.part_name(*p)).collect(),
            picking == Some("workholdings"),
        ));
    }

    /// The Machine draft's fields.
    fn machine_fields(&self, f: &mut Vec<Field>, ctx: &Ctx) {
        let Draft::Machine {
            job,
            machine,
            by_kind,
            from_assembly,
            source,
            other,
            ..
        } = self
        else {
            return;
        };
        let _ = job;
        f.push(Field::select(
            "machine-from",
            &tr("cam-machine-from"),
            [
                tr("cam-machine-from-catalogue"),
                tr("cam-machine-from-assembly"),
            ],
            usize::from(*from_assembly),
        ));
        if *from_assembly {
            source_fields(f, ctx.doc, source.as_ref(), other);
        } else {
            controller_fields(f, machine, *by_kind, true, machine.rotary_axes() > 0);
        }
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
        f.push(Field::check(
            "library",
            &tr("cam-from-tool-library"),
            *library,
        ));
        if *library {
            let mut types = vec![tr("cam-all-types")];
            types.extend(labels(&MachiningType::ALL, MachiningType::label));
            f.push(Field::select(
                "lib-type",
                &tr("cam-machining-type"),
                types,
                filter.map_or(0, |m| index_of(&MachiningType::ALL, m) + 1),
            ));
            if lib.is_empty() {
                f.push(Field::note(tr("cam-no-tool-of-this-type")));
            } else {
                let cur = lib.iter().position(|(_, x)| **x == t.tool);
                let mut opts: Vec<String> = lib.iter().map(|(_, x)| x.name.clone()).collect();
                if cur.is_none() {
                    opts.insert(0, tr("cam-choose-a-tool"));
                }
                f.push(Field::select(
                    "lib-tool",
                    &tr("cam-tool"),
                    opts,
                    cur.unwrap_or(0),
                ));
            }
            f.push(Field::button(
                "lib-import",
                &tr("cam-import-tool-library"),
                "file-import",
            ));
            f.push(Field::button(
                "lib-export",
                &tr("cam-export-tool-library"),
                "file-export",
            ));
        }
        let tool = &t.tool;
        f.push(Field::section(&tr("cam-cutter")));
        f.push(Field::text("name", &tr("common-name"), &op.name));
        let kinds = ToolKind::for_machine(machine);
        f.push(Field::select(
            "kind",
            &tr("cam-cutter-type"),
            labels(&kinds, ToolKind::label),
            index_of(&kinds, tool.kind),
        ));
        if tool.is_beam() {
            f.push(Field::number(
                "diameter",
                &tr("cam-kerf-width"),
                tool.diameter,
                Unit::Mm,
            ));
        } else {
            f.push(Field::select(
                "unit",
                &tr("cam-unit"),
                [tr("cam-millimetres"), tr("cam-inches")],
                usize::from(tool.unit == LenUnit::Inch),
            ));
            f.push(Field::select(
                "hand",
                &tr("cam-tool-hand"),
                [tr("cam-hand-right"), tr("cam-hand-left")],
                usize::from(tool.hand == Hand::Left),
            ));
            // A form tool's diameter is its profile's widest; a taper ball's is its shank's.
            if tool.kind != ToolKind::Form {
                f.push(Field::number(
                    "diameter",
                    &tr(if tool.kind == ToolKind::TaperBall {
                        "cam-shank-diameter"
                    } else {
                        "cam-diameter"
                    }),
                    tool.diameter,
                    Unit::Mm,
                ));
            }
            match tool.kind {
                ToolKind::TaperBall => {
                    f.push(Field::number(
                        "tip",
                        &tr("cam-ball-diameter"),
                        tool.tip_diameter,
                        Unit::Mm,
                    ));
                    f.push(Field::number(
                        "angle",
                        &tr("cam-included-angle"),
                        tool.angle,
                        Unit::Deg,
                    ));
                }
                ToolKind::Lollipop => {
                    f.push(Field::number(
                        "neck",
                        &tr("cam-neck-diameter"),
                        tool.neck_diameter,
                        Unit::Mm,
                    ));
                }
                ToolKind::Barrel => {
                    f.push(Field::number(
                        "profile-radius",
                        &tr("cam-side-radius"),
                        tool.profile_radius,
                        Unit::Mm,
                    ));
                }
                ToolKind::Form => form_fields(f, tool),
                ToolKind::BullMill => {
                    f.push(Field::number(
                        "corner",
                        &tr("cam-corner-radius"),
                        tool.corner_radius,
                        Unit::Mm,
                    ));
                }
                ToolKind::ChamferMill | ToolKind::TaperMill => {
                    f.push(Field::number(
                        "angle",
                        &tr("cam-included-angle"),
                        tool.angle,
                        Unit::Deg,
                    ));
                    f.push(Field::number(
                        "tip",
                        &tr("cam-tip-diameter"),
                        tool.tip_diameter,
                        Unit::Mm,
                    ));
                }
                ToolKind::Drill => {
                    f.push(Field::number(
                        "angle",
                        &tr("cam-point-angle"),
                        tool.angle,
                        Unit::Deg,
                    ));
                }
                ToolKind::Tap | ToolKind::ThreadMill => {
                    f.push(Field::number(
                        "pitch",
                        &tr("cam-thread-pitch"),
                        tool.pitch,
                        Unit::Mm,
                    ));
                }
                _ => {}
            }
            // (A form tool cuts over its profile's height.)
            if tool.kind != ToolKind::Form {
                f.push(Field::number(
                    "cut-len",
                    &tr("cam-cutting-length"),
                    tool.cutting_length,
                    Unit::Mm,
                ));
            }
            f.push(Field::number(
                "overall",
                &tr("cam-overall-length"),
                tool.overall_length,
                Unit::Mm,
            ));
            f.push(Field::count(
                "flutes",
                &tr("cam-flutes"),
                tool.flutes as i64,
            ));
            f.push(Field::check(
                "center",
                &tr("cam-center-cutting"),
                tool.center_cutting,
            ));
        }
        if tool.is_beam() {
            f.push(Field::section(&tr("cam-beam")));
            if tool.kind != ToolKind::Plasma {
                f.push(Field::number(
                    "power",
                    &tr("cam-power"),
                    tool.beam.power,
                    Unit::Percent,
                ));
            }
            f.push(Field::number(
                "pierce",
                &tr("cam-pierce-delay"),
                tool.beam.pierce_delay,
                Unit::Seconds,
            ));
            if tool.kind == ToolKind::Plasma {
                f.push(Field::number(
                    "pierce-h",
                    &tr("cam-pierce-height"),
                    tool.beam.pierce_height,
                    Unit::Mm,
                ));
                f.push(Field::check(
                    "thc",
                    &tr("cam-torch-height-control"),
                    tool.beam.thc,
                ));
                f.push(Field::count(
                    "material",
                    &tr("cam-material-number"),
                    tool.beam.material as i64,
                ));
            }
            if tool.kind != ToolKind::Laser {
                f.push(Field::number(
                    "cut-h",
                    &tr("cam-cut-height"),
                    tool.beam.cut_height,
                    Unit::Mm,
                ));
            } else {
                f.push(Field::check(
                    "air",
                    &tr("cam-air-assist"),
                    tool.beam.air_assist,
                ));
                f.push(Field::count(
                    "beam-passes",
                    &tr("cam-passes"),
                    tool.beam.passes as i64,
                ));
            }
        }
        f.push(Field::section(&tr("cam-cutting-data")));
        if !tool.is_beam() {
            let mut opts = vec![tr("cam-set-by-hand")];
            opts.extend(WorkMaterial::ALL.iter().map(|m| m.label().to_string()));
            let at = tool
                .cutting
                .material
                .and_then(|m| WorkMaterial::ALL.iter().position(|x| *x == m))
                .map_or(0, |i| i + 1);
            f.push(Field::select(
                "work-material",
                &tr("cam-worked-out-for"),
                opts,
                at,
            ));
            f.push(Field::number(
                "rpm",
                &tr("cam-spindle-speed"),
                tool.cutting.spindle_rpm,
                Unit::Rpm,
            ));
        }
        f.push(Field::number(
            "feed",
            &tr("cam-feed-rate"),
            tool.cutting.feed,
            Unit::Feed,
        ));
        if !tool.is_beam() {
            f.push(Field::number(
                "plunge",
                &tr("cam-plunge-rate"),
                tool.cutting.plunge_feed,
                Unit::Feed,
            ));
            f.push(Field::number(
                "stepdown",
                &tr("cam-step-down"),
                tool.cutting.step_down,
                Unit::Mm,
            ));
            f.push(Field::number(
                "stepover",
                &tr("cam-step-over"),
                tool.cutting.step_over,
                Unit::Mm,
            ));
            f.push(Field::section(&tr("cam-holder")));
            f.push(Field::check(
                "holder",
                &tr("cam-holder"),
                tool.holder.is_some(),
            ));
            if let Some(h) = &tool.holder {
                let holders: Vec<String> = ctx
                    .cam
                    .library
                    .holders
                    .iter()
                    .map(|x| x.name.clone())
                    .collect();
                if !holders.is_empty() {
                    let mut opts = vec![tr("cam-cylinder")];
                    opts.extend(holders);
                    let cur = ctx
                        .cam
                        .library
                        .holders
                        .iter()
                        .position(|x| x == h)
                        .map_or(0, |i| i + 1);
                    f.push(Field::select("holder-lib", &tr("cam-holder"), opts, cur));
                }
                f.push(Field::number(
                    "holder-d",
                    &tr("cam-holder-diameter"),
                    h.max_radius() * 2.0,
                    Unit::Mm,
                ));
                f.push(Field::number(
                    "holder-l",
                    &tr("cam-holder-length"),
                    h.length(),
                    Unit::Mm,
                ));
            }
        }
        f.push(Field::section(&tr("cam-tool-assembly")));
        f.push(Field::count(
            "number",
            &tr("cam-assembly-number"),
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
        let rotaries = rotaries_of(ctx.cam, *setup);
        let tool = tool_above(ctx.cam, *setup, op.id);
        let tk = tool.as_ref().map_or(ToolKind::EndMill, |t| t.kind);
        let beam = tk.is_beam();
        // (Each tab: its id, its label's catalog key.)
        let tabs: &[(&str, &str)] = if beam {
            &[
                ("Tool path", "cam-tab-tool-path"),
                ("Link", "cam-tab-link"),
                ("Misc", "cam-tab-misc"),
                ("Beam", "cam-beam"),
            ]
        } else {
            &[
                ("Tool path", "cam-tab-tool-path"),
                ("Link", "cam-tab-link"),
                ("Axis", "cam-tab-axis"),
                ("Collision", "cam-tab-collision"),
                ("Misc", "cam-tab-misc"),
            ]
        };
        let selected = tab.min(tabs.len() - 1);
        f.push(Field::Tabs {
            key: "tab".into(),
            labels: tabs.iter().map(|(_, k)| tr(k)).collect(),
            selected,
        });
        match tabs[selected].0 {
            "Link" => link_fields(f, p, beam),
            "Axis" => axis_fields(f, ctx, *setup, p, picking),
            "Collision" => collision_fields(f, p, tool.as_ref()),
            "Misc" => {
                f.push(Field::section(&tr("cam-quality")));
                f.push(Field::number(
                    "tolerance",
                    &tr("cam-cut-tolerance"),
                    p.tolerance,
                    Unit::Mm,
                ));
                if !beam {
                    f.push(Field::section(&tr("cam-smoothing")));
                    f.push(Field::number(
                        "smooth",
                        &tr("cam-smooth-corners"),
                        p.smooth_corners,
                        Unit::Mm,
                    ));
                }
                repeat_fields(f, &p.repeat);
                f.push(Field::section(&tr("cam-template")));
                f.push(Field::button(
                    "save-template",
                    &tr("cam-save-as-template"),
                    "books",
                ));
            }
            "Beam" => {
                f.push(Field::section(&tr("cam-beam")));
                f.push(Field::number(
                    "beam-power",
                    &tr("cam-power"),
                    p.beam.power,
                    Unit::Percent,
                ));
                f.push(Field::count(
                    "beam-passes",
                    &tr("cam-passes"),
                    p.beam.passes as i64,
                ));
                if p.beam.passes > 1 {
                    f.push(Field::number(
                        "pass-depth",
                        &tr("cam-depth-per-pass"),
                        p.beam.pass_depth,
                        Unit::Mm,
                    ));
                }
                if p.pattern == Pattern::Cut {
                    f.push(Field::count(
                        "micro",
                        &tr("cam-microjoints-per-part"),
                        p.beam.microjoints as i64,
                    ));
                    if p.beam.microjoints > 0 {
                        f.push(Field::number(
                            "micro-w",
                            &tr("cam-microjoint-width"),
                            p.beam.microjoint_width,
                            Unit::Mm,
                        ));
                    }
                    f.push(Field::number(
                        "small-hole",
                        &tr("cam-small-holes-under"),
                        p.beam.small_hole,
                        Unit::Mm,
                    ));
                    if p.beam.small_hole > 0.0 {
                        f.push(Field::number(
                            "small-feed",
                            &tr("cam-small-hole-feed"),
                            p.beam.small_hole_feed * 100.0,
                            Unit::Percent,
                        ));
                    }
                }
            }
            _ => toolpath_fields(f, ctx, op, p, *kind, tk, machine, rotaries, picking),
        }
        if tool.is_none() {
            f.push(Field::error(tr("cam-no-tool-above-toolpath")));
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
    face_plane(ctx, pick).map(|(_, n, _)| n)
}

/// A picked planar face (model coordinates): a point of it (its centre where the kernel gave
/// one), its outward normal and a direction along it, both unit.
pub(super) fn face_plane(ctx: &Ctx, pick: Pick) -> Option<([f64; 3], [f64; 3], [f64; 3])> {
    use cadrs_core::cam::cadrs_cam::geom::{cross, len, normalize};
    let Pick::Face(part, name) = pick else {
        return None;
    };
    let face = ctx
        .parts
        .iter()
        .find(|p| p.id == part)
        .and_then(|p| p.solid.faces.iter().find(|f| f.name == name))?;
    let pl = face.plane?;
    let n = cross(pl.u, pl.v);
    (len(n) > 1e-9 && len(pl.u) > 1e-9).then(|| {
        (
            face.center.unwrap_or(pl.origin),
            normalize(n),
            normalize(pl.u),
        )
    })
}

/// The line a pick gives (model coordinates): the axis of a cylindrical face (or any face of
/// revolution), a straight edge, or the axis of a circular edge. A point of it and its unit
/// direction.
pub(super) fn picked_line(ctx: &Ctx, pick: Pick) -> Option<([f64; 3], [f64; 3])> {
    use cadrs_core::cam::cadrs_cam::geom::{cross, len, normalize, sub};
    match pick {
        Pick::Face(part, name) => {
            let (point, dir) = ctx
                .parts
                .iter()
                .find(|p| p.id == part)
                .and_then(|p| p.solid.faces.iter().find(|f| f.name == name))
                .and_then(|f| f.axis)?;
            (len(dir) > 1e-9).then(|| (point, normalize(dir)))
        }
        Pick::Edge(part, name) => {
            let edge = ctx
                .parts
                .iter()
                .find(|p| p.id == part)
                .and_then(|p| p.solid.edges.iter().find(|e| e.name == name))?;
            if let Some(c) = edge.circle {
                return (len(c.normal) > 1e-9).then(|| (c.center, normalize(c.normal)));
            }
            if edge.ellipse.is_some() {
                return None;
            }
            let (a, b) = (*edge.points.first()?, *edge.points.last()?);
            let along = sub(b, a);
            let l = len(along);
            // Straight: every point of it on the line between its ends.
            let straight = l > 1e-9
                && edge
                    .points
                    .iter()
                    .all(|q| len(cross(sub(*q, a), along)) <= 1e-6 * l.max(1.0) * l);
            straight.then(|| (a, normalize(along)))
        }
        _ => None,
    }
}

/// A picked vertex's point (model coordinates).
pub(super) fn picked_point(ctx: &Ctx, pick: Pick) -> Option<[f64; 3]> {
    let Pick::Vertex(part, name) = pick else {
        return None;
    };
    ctx.parts
        .iter()
        .find(|p| p.id == part)
        .and_then(|p| p.solid.vertices.iter().find(|v| v.name == name))
        .map(|v| v.point)
}

/// "(1.5, 0, -20)".
pub(super) fn fmt_point(p: [f64; 3]) -> String {
    format!("({}, {}, {})", fmt(p[0]), fmt(p[1]), fmt(p[2]))
}

/// The library's toolpath templates a `tk` tool on a `machine` with `rotaries` rotary axes can
/// run.
pub(super) fn usable_templates(
    cam: &CamStudio,
    tk: ToolKind,
    machine: MachineKind,
    rotaries: usize,
) -> Vec<&Template> {
    cam.library
        .templates
        .iter()
        .filter(|t| {
            offered_patterns(t.params.pattern.type_of(), tk, machine, rotaries, None)
                .contains(&t.params.pattern)
        })
        .collect()
}

pub(super) fn machine_of(cam: &CamStudio, setup: CamId) -> MachineKind {
    cam.setup(setup)
        .and_then(|(j, _)| j.machine.as_ref())
        .map_or(MachineKind::Mill, |m| m.kind)
}

/// How many rotary axes the machine of a setup's job has.
pub(super) fn rotaries_of(cam: &CamStudio, setup: CamId) -> usize {
    cam.setup(setup)
        .and_then(|(j, _)| j.machine.as_ref())
        .map_or(0, Machine::rotary_axes)
}

/// The patterns of `kind` a toolpath is offered: those the tool and the machine can run, the
/// machine having the rotary axes each needs. `current`, the operation's own pattern, stays in
/// the list when only the axes are missing (the job's machine was changed under it), so the
/// dialog shows what the operation is.
pub(super) fn offered_patterns(
    kind: Type,
    tk: ToolKind,
    machine: MachineKind,
    rotaries: usize,
    current: Option<Pattern>,
) -> Vec<Pattern> {
    Pattern::available(kind, tk, machine)
        .into_iter()
        .filter(|p| p.rotary_axes() <= rotaries || current == Some(*p))
        .collect()
}

/// The types with a pattern to offer ([`offered_patterns`]).
pub(super) fn offered_types(
    tk: ToolKind,
    machine: MachineKind,
    rotaries: usize,
    current: Option<Pattern>,
) -> Vec<Type> {
    Type::ALL
        .into_iter()
        .filter(|t| !offered_patterns(*t, tk, machine, rotaries, current).is_empty())
        .collect()
}

/// What a machine with `rotaries` rotary axes lacks for `pattern` (`None`: it can run it).
pub(super) fn missing_axes(pattern: Pattern, rotaries: usize) -> Option<String> {
    let key = match (pattern.rotary_axes(), rotaries) {
        (need, have) if need <= have => return None,
        (1, _) => "cam-pattern-needs-a-rotary-axis",
        (_, 0) => "cam-pattern-needs-two-rotary-axes-has-none",
        _ => "cam-pattern-needs-two-rotary-axes-has-one",
    };
    Some(tr_with(key, &[("pattern", pattern.label())]))
}

/// What's wrong with a rotary pattern's own settings.
fn rotary_problem(p: &Params) -> Option<String> {
    use cadrs_core::cam::cadrs_cam::strategy::RotaryStyle;
    if p.pattern.rotary_axes() != 1 {
        return None;
    }
    if p.rotary.range.is_some_and(|r| r[1] <= r[0]) {
        Some(tr("cam-rotary-range-backwards"))
    } else if p.pattern == Pattern::Rotary
        && p.rotary.style == RotaryStyle::Thread
        && p.rotary.lead <= 0.0
    {
        Some(tr("cam-rotary-thread-needs-lead"))
    } else {
        None
    }
}

/// A form tool's profile in the Tool dialog: a row of radius, height and bulge a point, from
/// the tip upward, each with its remove button, and Add point; what is wrong with the profile
/// under them. Keys: `form-r-<i>`, `form-h-<i>`, `form-b-<i>`, `form-del-<i>` (from 0) and
/// `form-add`.
fn form_fields(f: &mut Vec<Field>, tool: &Tool) {
    f.push(Field::section(&tr("cam-form-profile")));
    f.push(Field::note(tr("cam-form-profile-note")));
    let empty = FormProfile::default();
    let profile = tool.form.as_ref().unwrap_or(&empty);
    for (i, p) in profile.points.iter().enumerate() {
        let n = (i + 1).to_string();
        f.push(Field::number(
            &format!("form-r-{i}"),
            &tr_with("cam-form-point-radius", &[("n", &n)]),
            p.r,
            Unit::Mm,
        ));
        f.push(Field::number(
            &format!("form-h-{i}"),
            &tr_with("cam-form-point-height", &[("n", &n)]),
            p.h,
            Unit::Mm,
        ));
        // (The first point has no segment arriving at it.)
        if i > 0 {
            f.push(Field::number(
                &format!("form-b-{i}"),
                &tr_with("cam-form-point-bulge", &[("n", &n)]),
                p.bulge,
                Unit::None,
            ));
        }
        if profile.points.len() > 2 {
            f.push(Field::button(
                &format!("form-del-{i}"),
                &tr_with("cam-form-remove-point", &[("n", &n)]),
                "remove-circle",
            ));
        }
    }
    if profile.points.len() < FormProfile::MAX_POINTS {
        f.push(Field::button("form-add", &tr("cam-form-add-point"), "plus"));
    }
    if let Err(e) = profile.check() {
        f.push(Field::error(e));
    }
    f.push(Field::note(tr_with(
        "cam-form-size",
        &[
            ("diameter", &fmt(tool.diameter)),
            ("length", &fmt(tool.cutting_length)),
        ],
    )));
}

/// A machine's axes in a line, the tool's side then the work's: "Z B · Y X C" (a machine
/// known by its travel alone: "X Y Z").
pub(crate) fn axes_summary(machine: &Machine) -> String {
    let chain = machine.kinematics();
    let names = |joints: &[cadrs_core::cam::cadrs_cam::kinematics::Joint]| {
        joints
            .iter()
            .map(|j| j.name.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    };
    match (chain.tool.is_empty(), chain.table.is_empty()) {
        (false, false) => format!("{} · {}", names(&chain.tool), names(&chain.table)),
        (false, true) => names(&chain.tool),
        _ => names(&chain.table),
    }
}

/// The Machine dialog's fields for a machine that comes from an assembly: the document (when
/// the library has others with versions), the assembly and, for another document's, the
/// version; what machine it makes (its axes and controller, read-only: they are the Machine
/// Builder's to change), what stands in its way, and the way to the builder (an assembly of
/// this document) or where the frozen copy is from.
fn source_fields(
    f: &mut Vec<Field>,
    doc: &cadrs_core::Document,
    source: Option<&MachineSource>,
    from: &MachineFrom,
) {
    if !from.others.is_empty() {
        let mut docs = vec![tr("cam-this-document")];
        docs.extend(from.others.iter().map(|o| o.1.clone()));
        f.push(Field::select(
            "machine-document",
            &tr("cam-document"),
            docs,
            from.document,
        ));
    }
    let current = |list: &[(ElementId, String)]| {
        source
            .and_then(|s| list.iter().position(|(e, _)| *e == s.reference.element))
            .unwrap_or(0)
    };
    let assemblies = |f: &mut Vec<Field>, list: &[(ElementId, String)]| {
        f.push(Field::select(
            "machine-assembly",
            &tr("cam-machine-assembly"),
            list.iter().map(|(_, n)| n.clone()).collect::<Vec<_>>(),
            current(list),
        ));
    };
    if from.document == 0 {
        let list = machine_assemblies(doc);
        if list.is_empty() {
            f.push(Field::note(tr("cam-machine-no-assemblies")));
            return;
        }
        assemblies(f, &list);
    } else if let Some(o) = &from.other {
        if o.assemblies.is_empty() {
            f.push(Field::note(tr("cam-machine-none-in-document")));
        } else {
            assemblies(f, &o.assemblies);
        }
        f.push(Field::select(
            "machine-version",
            &tr("common-version"),
            o.versions.iter().map(|v| v.1.clone()).collect::<Vec<_>>(),
            o.version,
        ));
    }
    let Some(source) = source else { return };
    // "V2 of Machines › 5-axis mill".
    if source.copy.is_some() {
        f.push(Field::note(source.label()));
    }
    let issues = match from.machine(doc, source) {
        Ok((machine, issues)) => {
            f.push(Field::note(tr_with(
                "cam-machine-axes-summary",
                &[("axes", &axes_summary(&machine))],
            )));
            f.push(Field::note(tr_with(
                "cam-machine-kind-controller",
                &[
                    ("kind", machine.kind.label()),
                    ("controller", machine.dialect.label()),
                ],
            )));
            if machine.rotary_axes() > 0 {
                f.push(Field::note(if machine.post.rotary.tcp {
                    tr("cam-machine-tcp-on")
                } else {
                    tr("cam-machine-tcp-off")
                }));
            }
            issues
        }
        Err(issues) => issues,
    };
    if !issues.is_empty() {
        f.push(Field::section(&tr("asm-machine-issues")));
        for i in &issues {
            if i.is_error() {
                f.push(Field::error(i.text.clone()));
            } else {
                f.push(Field::note(format!("⚠ {}", i.text)));
            }
        }
    }
    if source.copy.is_some() {
        f.push(Field::note(tr("cam-machine-frozen-copy")));
        return;
    }
    f.push(Field::note(tr("cam-machine-edit-in-builder")));
    f.push(Field::button(
        "open-builder",
        &tr("cam-open-machine-builder"),
        "machine-builder",
    ));
}

/// A machine's controller as a dialog edits it: the catalogue machine it starts from, its
/// spindle and tool changer, and how its programs are written (the post settings, and the
/// rotary ones for a machine with rotary axes). `kinematics`: its travel, rates and indexing
/// axes too, for a catalogue machine; an assembly's machine has them from its axes (the
/// Assembly tab's Machine Builder shows the rest). [`set_controller`] takes the changes.
pub(crate) fn controller_fields(
    f: &mut Vec<Field>,
    machine: &Machine,
    by_kind: bool,
    kinematics: bool,
    rotary: bool,
) {
    use cadrs_core::cam::cadrs_cam::Dialect;
    let by_kind = &by_kind;
    let cat = catalog();
    f.push(Field::check(
        "by-kind",
        &tr("cam-group-by-machine-type"),
        *by_kind,
    ));
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
    f.push(Field::select("machine", &tr("cam-machine"), opts, current));
    f.push(Field::note(tr_with(
        "cam-machine-kind-controller",
        &[
            ("kind", machine.kind.label()),
            ("controller", machine.dialect.label()),
        ],
    )));
    if kinematics {
        f.push(Field::section(&tr("cam-machine")));
        for (i, a) in ["X", "Y", "Z"].iter().enumerate() {
            f.push(Field::number(
                &format!("travel-{i}"),
                &tr_with("cam-axis-travel", &[("axis", a)]),
                machine.travel[i],
                Unit::Mm,
            ));
        }
        f.push(Field::number(
            "rapid",
            &tr("cam-rapid-rate"),
            machine.rapid_rate,
            Unit::Feed,
        ));
        f.push(Field::number(
            "max-feed",
            &tr("cam-fastest-feed"),
            machine.max_feed,
            Unit::Feed,
        ));
    } else {
        // An assembly's machine: its travel and rates are its axes'; the controller family is
        // chosen here rather than by the catalogue machine alone.
        f.push(Field::select(
            "dialect",
            &tr("cam-controller"),
            labels(&Dialect::ALL, Dialect::label),
            index_of(&Dialect::ALL, machine.dialect),
        ));
    }
    if machine.kind.is_milling() {
        f.push(Field::number(
            "max-spindle",
            &tr("cam-fastest-spindle"),
            machine.max_spindle,
            Unit::Rpm,
        ));
        f.push(Field::check(
            "changer",
            &tr("cam-automatic-tool-changer"),
            machine.tool_changer,
        ));
        match &machine.chain {
            _ if !kinematics => {}
            // A machine described by an assembly has the axes that gives it.
            Some(chain) => {
                let axes: Vec<&str> = chain.joints().map(|j| j.name.as_str()).collect();
                f.push(Field::note(tr_with(
                    "cam-machine-own-axes",
                    &[("axes", &axes.join(", "))],
                )));
            }
            None => {
                f.push(Field::check(
                    "indexing",
                    &tr("cam-indexing-rotary-axes"),
                    machine.indexing.is_some(),
                ));
                if let Some(ix) = machine.indexing {
                    f.push(Field::number(
                        "max-tilt",
                        &tr("cam-largest-tilt"),
                        ix.max_tilt,
                        Unit::Deg,
                    ));
                    f.push(Field::select(
                        "rotary",
                        &tr("cam-rotary-axes"),
                        vec![tr("cam-rotary-a-and-c"), tr("cam-rotary-b-and-c")],
                        usize::from(ix.rotary[0] == 'B'),
                    ));
                }
            }
        }
    } else if machine.kind == MachineKind::Laser {
        f.push(Field::number(
            "max-spindle",
            &tr("cam-s-at-full-power"),
            machine.max_spindle,
            Unit::None,
        ));
    }
    let s = &machine.post;
    f.push(Field::section(&tr("cam-post-settings")));
    f.push(Field::select(
        "unit",
        &tr("cam-output-unit"),
        [tr("cam-millimetres"), tr("cam-inches")],
        usize::from(s.output_unit == LenUnit::Inch),
    ));
    f.push(Field::check(
        "adv-wp",
        &tr("cam-advanced-work-planes"),
        s.advanced_work_planes,
    ));
    f.push(Field::check(
        "seq",
        &tr("cam-sequence-numbers"),
        s.sequence_numbers,
    ));
    if s.sequence_numbers {
        f.push(Field::count(
            "block-start",
            &tr("cam-block-number-start"),
            s.block_start as i64,
        ));
        f.push(Field::count(
            "block-inc",
            &tr("cam-block-increment"),
            s.block_increment as i64,
        ));
        f.push(Field::check(
            "tools-only",
            &tr("cam-numbers-at-tool-changes-only"),
            s.numbers_at_tools_only,
        ));
    }
    f.push(Field::check(
        "rapid-pos",
        &tr("cam-rapid-positioning"),
        s.rapid_positioning,
    ));
    if !s.rapid_positioning {
        f.push(Field::number(
            "post-rapid",
            &tr("cam-rapid-feed"),
            s.rapid_rate,
            Unit::Feed,
        ));
    }
    f.push(Field::check("arcs", &tr("cam-output-arcs"), s.output_arcs));
    if s.output_arcs {
        f.push(Field::check(
            "helices",
            &tr("cam-output-helices"),
            s.output_helices,
        ));
    }
    f.push(Field::count(
        "decimals",
        &tr("cam-decimal-places"),
        s.decimals as i64,
    ));
    f.push(Field::check(
        "dec-point",
        &tr("cam-always-write-decimal-point"),
        s.decimal_point,
    ));
    if machine.kind.is_milling() {
        f.push(Field::check(
            "ret-tool",
            &tr("cam-retract-on-tool-changes"),
            s.force_retract_on_tool_change,
        ));
        f.push(Field::check(
            "ret-plane",
            &tr("cam-retract-on-plane-changes"),
            s.force_retract_on_plane_change,
        ));
        f.push(Field::check(
            "ret-end",
            &tr("cam-retract-at-program-end"),
            s.force_retract_on_end,
        ));
        if machine.dialect.has_cycles() {
            f.push(Field::section(&tr("cam-fixed-cycles")));
            f.push(Field::check(
                "cy-drill",
                &tr("cam-cycles-drilling"),
                s.canned_drill,
            ));
            f.push(Field::check(
                "cy-peck",
                &tr("cam-cycles-peck-drilling"),
                s.canned_peck,
            ));
            f.push(Field::check(
                "cy-tap",
                &tr("cam-cycles-tapping"),
                s.canned_tap,
            ));
            f.push(Field::check(
                "cy-chip",
                &tr("cam-cycles-chip-break"),
                s.canned_chip_break,
            ));
        }
    }
    if machine.kind == MachineKind::Laser {
        f.push(Field::select(
            "laser-mode",
            &tr("cam-laser-power"),
            [tr("cam-laser-dynamic"), tr("cam-laser-constant")],
            usize::from(s.laser_mode == LaserMode::Constant),
        ));
    }
    f.push(Field::check(
        "comments",
        &tr("cam-comments-in-program"),
        s.comments,
    ));
    if rotary {
        rotary_post_fields(f, machine);
    }
}

/// The Machine dialog's group for a machine with rotary axes: how its program is written
/// where they turn.
fn rotary_post_fields(f: &mut Vec<Field>, machine: &Machine) {
    use cadrs_core::cam::cadrs_cam::Dialect;
    use cadrs_core::cam::cadrs_cam::post::{RotaryFeed, TiltedPlane};
    let r = &machine.post.rotary;
    f.push(Field::section(&tr("cam-rotary-post")));
    f.push(Field::check("tcp", &tr("cam-tcp"), r.tcp));
    if r.tcp {
        f.push(Field::text("tcp-on", &tr("cam-tcp-on"), &r.tcp_on));
        f.push(Field::text("tcp-off", &tr("cam-tcp-off"), &r.tcp_off));
        // A controller family with no code of its own posts without it until one is given.
        if r.tcp_codes(machine.dialect, 1).is_none() {
            f.push(Field::error(tr("cam-tcp-needs-codes")));
        } else {
            f.push(Field::note(tr("cam-tcp-codes-note")));
        }
    }
    f.push(Field::select(
        "tilted-plane",
        &tr("cam-tilted-planes"),
        [
            tr("cam-tilted-plane-control"),
            tr("cam-tilted-plane-positions"),
        ],
        usize::from(r.tilted_plane == TiltedPlane::Positions),
    ));
    if r.tilted_plane == TiltedPlane::Control && machine.dialect != Dialect::Fanuc {
        f.push(Field::note(tr("cam-tilted-plane-no-code")));
    }
    f.push(Field::select(
        "rot-feed",
        &tr("cam-rotary-feed"),
        labels(&RotaryFeed::ALL, RotaryFeed::label),
        index_of(&RotaryFeed::ALL, r.feed),
    ));
    f.push(Field::number(
        "rot-tol",
        &tr("cam-rotary-tolerance"),
        r.tolerance,
        Unit::Mm,
    ));
    f.push(Field::number(
        "rot-axis-tol",
        &tr("cam-rotary-axis-tolerance"),
        r.axis_tolerance,
        Unit::Deg,
    ));
    f.push(Field::number(
        "rot-retract",
        &tr("cam-rotary-retract"),
        r.retract,
        Unit::Mm,
    ));
    f.push(Field::count(
        "angle-decimals",
        &tr("cam-angle-decimal-places"),
        r.angle_decimals as i64,
    ));
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

/// A drive entity's label: the face or edge and its part (`what` and `what_of` are catalog
/// keys: the entity alone, and "… of { $name }").
pub(super) fn entity_label(ctx: &Ctx, key: &str, what: &str, what_of: &str) -> String {
    match pick_of_key(key).and_then(|p| p.part()) {
        Some(part) => tr_with(what_of, &[("name", &ctx.part_name(part))]),
        None => tr(what),
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
