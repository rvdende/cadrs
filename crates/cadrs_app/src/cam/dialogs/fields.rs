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
            Draft::Machine { .. } => self.machine_fields(&mut f),
            Draft::Setup { setup, job, .. } => {
                f.push(Field::text("name", &tr("cam-setup-name"), &setup.name));
                f.push(Field::count(
                    "program",
                    &tr("cam-program-number"),
                    setup.program_number as i64,
                ));
                let man = matches!(setup.mating, Mating::Manual(_));
                f.push(Field::select(
                    "mating",
                    &tr("cam-machine-mating"),
                    [tr("cam-automatic"), tr("cam-manual")],
                    usize::from(man),
                ));
                if let Mating::Manual(o) = setup.mating {
                    for (i, a) in ["cam-offset-x", "cam-offset-y", "cam-offset-z"]
                        .iter()
                        .enumerate()
                    {
                        f.push(Field::number(&format!("mate-{i}"), &tr(a), o[i], Unit::Mm));
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
    fn machine_fields(&self, f: &mut Vec<Field>) {
        let Draft::Machine {
            machine, by_kind, ..
        } = self
        else {
            return;
        };
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
                if machine.dialect != cadrs_core::cam::cadrs_cam::Dialect::Fanuc {
                    f.push(Field::note(tr("cam-tilted-planes-fanuc-only")));
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
            f.push(Field::number(
                "diameter",
                &tr("cam-diameter"),
                tool.diameter,
                Unit::Mm,
            ));
            match tool.kind {
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
            f.push(Field::number(
                "cut-len",
                &tr("cam-cutting-length"),
                tool.cutting_length,
                Unit::Mm,
            ));
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
            _ => toolpath_fields(f, ctx, op, p, *kind, tk, machine, picking),
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
