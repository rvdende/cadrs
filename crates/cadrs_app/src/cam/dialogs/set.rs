//! The dialogs' changes: a changed field ([`Draft::set`]), a pick in the view ([`Draft::pick`]),
//! what the view may pick, the draft as the studio would have it (for the live preview) and the
//! edits ✓ makes ([`Draft::commit`]).

use super::*;

impl Draft {
    /// Takes a changed field.
    pub(super) fn set(&mut self, world: &mut World, key: &str, v: &Value, ctx_cam: &CamStudio) {
        match self {
            Draft::Insert(d) => match (key, v) {
                ("role", Value::Index(i)) => d.role = Role::ALL[(*i).min(2)],
                ("document", Value::Index(i)) => {
                    d.document = (*i).min(d.others.len());
                    d.studio = 0;
                    d.other = match d.document {
                        0 => None,
                        k => super::load_other(world, d.others[k - 1].0, None),
                    };
                    // Another document's newest version; this one's workspace.
                    d.version = d
                        .other
                        .as_ref()
                        .map_or(0, |o| o.versions.len().saturating_sub(1));
                    fill_parts(world, d);
                }
                ("studio", Value::Index(i)) => {
                    d.studio = *i;
                    fill_parts(world, d);
                }
                ("version", Value::Index(i)) => {
                    d.version = *i;
                    // Another document's Part Studios as that version has them.
                    if d.document > 0
                        && let Some(vid) = d.other.as_ref().and_then(|o| o.versions.get(*i))
                    {
                        let vid = vid.0;
                        d.other = super::load_other(world, d.others[d.document - 1].0, Some(vid));
                        d.studio = 0;
                    }
                    fill_parts(world, d);
                }
                (k, Value::Bool(b)) if k.starts_with("part-") => {
                    if let Some(p) = k[5..]
                        .parse::<usize>()
                        .ok()
                        .and_then(|i| d.parts.get_mut(i))
                    {
                        p.2 = *b;
                    }
                }
                _ => {}
            },
            Draft::Component(c) => match (key, v) {
                ("name", Value::Text(t)) => c.name = t.clone(),
                ("role", Value::Index(i)) => c.role = Role::ALL[(*i).min(2)],
                _ => {}
            },
            Draft::Job { job, .. } => match (key, v) {
                ("name", Value::Text(t)) => job.name = t.clone(),
                ("bodies", Value::Remove(i)) => {
                    if *i < job.bodies.len() {
                        job.bodies.remove(*i);
                    }
                }
                ("workholdings", Value::Remove(i)) => {
                    if *i < job.workholdings.len() {
                        job.workholdings.remove(*i);
                    }
                }
                ("stock-kind", Value::Index(i)) => {
                    job.stock.kind = match i {
                        0 => StockKind::Box,
                        1 => StockKind::Cylinder,
                        _ => ctx_cam
                            .components
                            .iter()
                            .find(|c| c.role == Role::Stock)
                            .map_or(StockKind::Component(CamId(0)), |c| {
                                StockKind::Component(c.id)
                            }),
                    }
                }
                ("stock-comp", Value::Index(i)) => {
                    if let Some(c) = ctx_cam
                        .components
                        .iter()
                        .filter(|c| c.role == Role::Stock)
                        .nth(*i)
                    {
                        job.stock.kind = StockKind::Component(c.id);
                    }
                }
                ("stock-dir", Value::Index(i)) => {
                    job.stock.direction = if *i == 0 {
                        StockDirection::World
                    } else {
                        let z = AXES[*i].1;
                        let x = if z[0].abs() > 0.5 {
                            [0.0, 1.0, 0.0]
                        } else {
                            [1.0, 0.0, 0.0]
                        };
                        StockDirection::Axes { z, x }
                    }
                }
                ("radial", Value::Number(n)) => job.stock.radial = n.max(0.0),
                (k, Value::Number(n)) if k.starts_with("off-") => {
                    if let Ok(i) = k[4..].parse::<usize>() {
                        job.stock.offsets[i.min(5)] = *n;
                    }
                }
                _ => {}
            },
            Draft::Machine { .. } => self.set_machine(key, v),
            Draft::Setup { setup, .. } => match (key, v) {
                ("name", Value::Text(t)) => setup.name = t.clone(),
                ("program", Value::Number(n)) => setup.program_number = n.clamp(1.0, 9999.0) as u32,
                ("mating", Value::Index(i)) => {
                    setup.mating = if *i == 1 {
                        Mating::Manual([0.0; 3])
                    } else {
                        Mating::Automatic
                    }
                }
                (k, Value::Number(n)) if k.starts_with("mate-") => {
                    if let (Mating::Manual(o), Ok(i)) = (&mut setup.mating, k[5..].parse::<usize>())
                    {
                        o[i.min(2)] = *n;
                    }
                }
                _ => {
                    set_plane(&mut setup.wcs, key, v);
                }
            },
            Draft::WorkPlane { op, .. } => {
                if let ("name", Value::Text(t)) = (key, v) {
                    op.name = t.clone();
                } else if let OpKind::WorkPlane(w) = &mut op.kind {
                    set_plane(w, key, v);
                }
            }
            Draft::Tool { .. } => self.set_tool(world, key, v, ctx_cam),
            Draft::Toolpath { .. } => self.set_toolpath(world, key, v, ctx_cam),
        }
    }

    /// A changed field of the Machine draft.
    fn set_machine(&mut self, key: &str, v: &Value) {
        let Draft::Machine {
            machine,
            by_kind,
            from,
            ..
        } = self
        else {
            return;
        };
        let s = &mut machine.post;
        match (key, v) {
            ("by-kind", Value::Bool(b)) => *by_kind = *b,
            ("machine", Value::Index(i)) => {
                let cat = catalog();
                let mut order: Vec<usize> = (0..cat.len()).collect();
                if *by_kind {
                    order.sort_by_key(|i| index_of(&MachineKind::ALL, cat[*i].kind));
                } else {
                    order.sort_by_key(|i| cat[*i].title());
                }
                if let Some(&k) = order.get(*i) {
                    **machine = cat[k].clone();
                    *from = Some(k);
                }
            }
            (k, Value::Number(n)) if k.starts_with("travel-") => {
                if let Ok(i) = k[7..].parse::<usize>() {
                    machine.travel[i.min(2)] = *n;
                }
            }
            ("rapid", Value::Number(n)) => machine.rapid_rate = *n,
            ("max-feed", Value::Number(n)) => machine.max_feed = *n,
            ("max-spindle", Value::Number(n)) => machine.max_spindle = *n,
            ("changer", Value::Bool(b)) => machine.tool_changer = *b,
            ("indexing", Value::Bool(b)) => {
                machine.indexing = b.then_some(cadrs_core::cam::cadrs_cam::Indexing {
                    max_tilt: machine.indexing.map_or(110.0, |i| i.max_tilt),
                    rotary: machine.indexing.map_or(['A', 'C'], |i| i.rotary),
                })
            }
            ("rotary", Value::Index(i)) => {
                if let Some(ix) = &mut machine.indexing {
                    ix.rotary = if *i == 1 { ['B', 'C'] } else { ['A', 'C'] };
                }
            }
            ("max-tilt", Value::Number(n)) => {
                if let Some(i) = &mut machine.indexing {
                    i.max_tilt = n.clamp(0.0, 180.0)
                }
            }
            ("unit", Value::Index(i)) => {
                s.output_unit = if *i == 1 { LenUnit::Inch } else { LenUnit::Mm }
            }
            ("adv-wp", Value::Bool(b)) => s.advanced_work_planes = *b,
            ("seq", Value::Bool(b)) => s.sequence_numbers = *b,
            ("block-start", Value::Number(n)) => s.block_start = n.max(0.0) as u32,
            ("block-inc", Value::Number(n)) => s.block_increment = n.max(1.0) as u32,
            ("tools-only", Value::Bool(b)) => s.numbers_at_tools_only = *b,
            ("rapid-pos", Value::Bool(b)) => s.rapid_positioning = *b,
            ("post-rapid", Value::Number(n)) => s.rapid_rate = n.max(1.0),
            ("arcs", Value::Bool(b)) => s.output_arcs = *b,
            ("helices", Value::Bool(b)) => s.output_helices = *b,
            ("decimals", Value::Number(n)) => s.decimals = n.clamp(0.0, 6.0) as u32,
            ("dec-point", Value::Bool(b)) => s.decimal_point = *b,
            ("ret-tool", Value::Bool(b)) => s.force_retract_on_tool_change = *b,
            ("ret-plane", Value::Bool(b)) => s.force_retract_on_plane_change = *b,
            ("ret-end", Value::Bool(b)) => s.force_retract_on_end = *b,
            ("cy-drill", Value::Bool(b)) => s.canned_drill = *b,
            ("cy-peck", Value::Bool(b)) => s.canned_peck = *b,
            ("cy-tap", Value::Bool(b)) => s.canned_tap = *b,
            ("cy-chip", Value::Bool(b)) => s.canned_chip_break = *b,
            ("laser-mode", Value::Index(i)) => {
                s.laser_mode = if *i == 1 {
                    LaserMode::Constant
                } else {
                    LaserMode::Dynamic
                }
            }
            ("comments", Value::Bool(b)) => s.comments = *b,
            _ => {}
        }
    }

    /// A changed field of the Tool draft.
    fn set_tool(&mut self, world: &mut World, key: &str, v: &Value, ctx_cam: &CamStudio) {
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
        let machine = machine_of(ctx_cam, *setup);
        let name_was_default = match &op.kind {
            OpKind::Tool(t) => op.name == t.tool.name || op.name == t.tool.default_name(),
            _ => false,
        };
        let OpKind::Tool(t) = &mut op.kind else {
            return;
        };
        let tool = &mut t.tool;
        match (key, v) {
            ("library", Value::Bool(b)) => *library = *b,
            ("lib-type", Value::Index(i)) => {
                *filter = if *i == 0 {
                    None
                } else {
                    MachiningType::ALL.get(i - 1).copied()
                }
            }
            ("lib-tool", Value::Index(i)) => {
                let lib: Vec<Tool> = ctx_cam
                    .library
                    .filtered(*filter, None)
                    .into_iter()
                    .filter(|(_, x)| x.kind.fits(machine))
                    .map(|(_, x)| x.clone())
                    .collect();
                let cur = lib.iter().position(|x| x == tool);
                let i = if cur.is_none() {
                    i.checked_sub(1)
                } else {
                    Some(*i)
                };
                if let Some(x) = i.and_then(|i| lib.get(i)) {
                    *tool = x.clone();
                    op.name = tool.name.clone();
                }
            }
            ("lib-import", Value::Activate) => {
                let dir = crate::export_dir(world).unwrap_or_default();
                let theme = world.resource::<Theme>().clone();
                let mut commands = world.commands();
                cadrs_ui::open_file_picker(
                    &mut commands,
                    &theme,
                    "cam-library-import",
                    "Import tool library",
                    "cam-library-import",
                    dir,
                    &["json"],
                );
                world.flush();
            }
            ("lib-export", Value::Activate) => export_library(world, ctx_cam),
            ("name", Value::Text(s)) => op.name = s.clone(),
            ("kind", Value::Index(i)) => {
                let kinds = ToolKind::for_machine(machine);
                if let Some(k) = kinds.get(*i) {
                    let d = if k.is_beam() == tool.is_beam() {
                        tool.diameter
                    } else if k.is_beam() {
                        default_kerf(machine)
                    } else {
                        6.0
                    };
                    *tool = Tool::new(*k, d);
                }
            }
            ("unit", Value::Index(i)) => {
                tool.unit = if *i == 1 { LenUnit::Inch } else { LenUnit::Mm }
            }
            ("hand", Value::Index(i)) => tool.hand = if *i == 1 { Hand::Left } else { Hand::Right },
            ("diameter", Value::Number(n)) => tool.diameter = *n,
            ("corner", Value::Number(n)) => tool.corner_radius = *n,
            ("angle", Value::Number(n)) => tool.angle = *n,
            ("tip", Value::Number(n)) => tool.tip_diameter = *n,
            ("pitch", Value::Number(n)) => tool.pitch = *n,
            ("cut-len", Value::Number(n)) => tool.cutting_length = *n,
            ("overall", Value::Number(n)) => tool.overall_length = *n,
            ("flutes", Value::Number(n)) => tool.flutes = n.clamp(1.0, 32.0) as u32,
            ("center", Value::Bool(b)) => tool.center_cutting = *b,
            ("power", Value::Number(n)) => tool.beam.power = n.clamp(0.0, 100.0),
            ("pierce", Value::Number(n)) => tool.beam.pierce_delay = n.max(0.0),
            ("pierce-h", Value::Number(n)) => tool.beam.pierce_height = *n,
            ("cut-h", Value::Number(n)) => tool.beam.cut_height = *n,
            ("thc", Value::Bool(b)) => tool.beam.thc = *b,
            ("material", Value::Number(n)) => tool.beam.material = n.max(0.0) as u32,
            ("air", Value::Bool(b)) => tool.beam.air_assist = *b,
            ("beam-passes", Value::Number(n)) => tool.beam.passes = n.clamp(1.0, 50.0) as u32,
            ("rpm", Value::Number(n)) => tool.cutting.spindle_rpm = n.max(0.0),
            ("feed", Value::Number(n)) => tool.cutting.feed = *n,
            ("plunge", Value::Number(n)) => tool.cutting.plunge_feed = *n,
            ("stepdown", Value::Number(n)) => tool.cutting.step_down = *n,
            ("stepover", Value::Number(n)) => tool.cutting.step_over = *n,
            ("work-material", Value::Index(i)) => {
                tool.cutting.material = i
                    .checked_sub(1)
                    .and_then(|i| WorkMaterial::ALL.get(i).copied())
            }
            ("holder", Value::Bool(b)) => {
                tool.holder = b.then(|| {
                    ctx_cam
                        .library
                        .holders
                        .first()
                        .cloned()
                        .unwrap_or_else(|| Profile::cylinder("Holder", 32.0, 40.0))
                })
            }
            ("holder-lib", Value::Index(i)) => {
                tool.holder = Some(if *i == 0 {
                    Profile::cylinder("Holder", 32.0, 40.0)
                } else {
                    ctx_cam
                        .library
                        .holders
                        .get(i - 1)
                        .cloned()
                        .unwrap_or_else(|| Profile::cylinder("Holder", 32.0, 40.0))
                })
            }
            ("holder-d", Value::Number(n)) => {
                if let Some(h) = &mut tool.holder {
                    *h = Profile::cylinder(h.name.clone(), n.max(0.1), h.length().max(1.0));
                }
            }
            ("holder-l", Value::Number(n)) => {
                if let Some(h) = &mut tool.holder {
                    *h = Profile::cylinder(h.name.clone(), h.max_radius() * 2.0, n.max(0.1));
                }
            }
            ("number", Value::Number(n)) => t.number = n.clamp(1.0, 999.0) as u32,
            _ => {}
        }
        // Cutting data worked out for a material follows the tool (and the machine's limits);
        // typed over, it's the operator's own.
        if let OpKind::Tool(t) = &mut op.kind {
            let tool = &mut t.tool;
            if matches!(key, "rpm" | "feed" | "plunge" | "stepdown" | "stepover") {
                tool.cutting.material = None;
            } else if let Some(m) = tool.cutting.material
                && !tool.is_beam()
            {
                let (rpm, feed) = ctx_cam
                    .setup(*setup)
                    .and_then(|(j, _)| j.machine.as_ref())
                    .map_or((24_000.0, 10_000.0), |m| (m.max_spindle, m.max_feed));
                tool.cutting = feeds::cutting_data(tool, m, rpm, feed);
            }
        }
        // A name nobody typed follows the tool.
        if let OpKind::Tool(t) = &mut op.kind
            && key != "name"
            && name_was_default
        {
            t.tool.name = t.tool.default_name();
            op.name = t.tool.name.clone();
        }
    }

    /// A changed field of the Toolpath draft.
    fn set_toolpath(&mut self, world: &mut World, key: &str, v: &Value, ctx_cam: &CamStudio) {
        let Draft::Toolpath {
            op, kind, setup, ..
        } = self
        else {
            return;
        };
        let machine = machine_of(ctx_cam, *setup);
        let tool = tool_above(ctx_cam, *setup, op.id);
        let op_id = op.id;
        let name_edited = matches!(&op.kind, OpKind::Toolpath(t) if t.name_edited);
        let OpKind::Toolpath(tp) = &mut op.kind else {
            return;
        };
        let p = &mut tp.params;
        let tk = tool.as_ref().map_or(ToolKind::EndMill, |t| t.kind);
        let mut renamed = None;
        match (key, v) {
            ("name", Value::Text(s)) => {
                renamed = Some(s.clone());
            }
            ("template", Value::Index(i)) => {
                if let Some(t) = i
                    .checked_sub(1)
                    .and_then(|i| usable_templates(ctx_cam, tk, machine).get(i).copied())
                {
                    let ty = t.params.pattern.type_of();
                    let fallback = Tool::new(tk, 6.0);
                    t.apply_to(p, tool.as_ref().unwrap_or(&fallback), machine);
                    // Picks of another kind of geometry don't carry over.
                    if ty != *kind {
                        p.drive = Default::default();
                        *kind = ty;
                    }
                    if !name_edited {
                        renamed = Some(t.name.clone());
                    }
                }
            }
            ("save-template", Value::Activate) => {
                let name = op.name.clone();
                let mut lib = ctx_cam.library.clone();
                let fallback = Tool::new(tk, 6.0);
                lib.save_template(Template::new(&name, p, tool.as_ref().unwrap_or(&fallback)));
                edit(world, Edit::SetLibrary(Box::new(lib)));
                toast(
                    world,
                    &format!("Saved the template \"{name}\" to the library"),
                );
            }
            ("type", Value::Index(i)) => {
                let types: Vec<Type> = Type::ALL
                    .into_iter()
                    .filter(|t| !Pattern::available(*t, tk, machine).is_empty())
                    .collect();
                if let Some(t) = types.get(*i) {
                    *kind = *t;
                    let pat = Pattern::available(*t, tk, machine)[0];
                    if let Some(tool) = &tool {
                        let drive = std::mem::take(&mut p.drive);
                        *p = Params::new(pat, tool, machine);
                        // Picks of the same kind stay.
                        p.drive.faces = if *t == Type::Faces || *t == Type::Bodies {
                            drive.faces
                        } else {
                            Vec::new()
                        };
                        p.drive.edges = if *t == Type::Edges {
                            drive.edges
                        } else {
                            Vec::new()
                        };
                        p.drive.holes = if *t == Type::Holes {
                            drive.holes
                        } else {
                            Vec::new()
                        };
                    }
                }
            }
            ("pattern", Value::Index(i)) => {
                let pats = Pattern::available(*kind, tk, machine);
                if let (Some(pat), Some(tool)) = (pats.get(*i), &tool) {
                    let drive = std::mem::take(&mut p.drive);
                    *p = Params::new(*pat, tool, machine);
                    p.drive = drive;
                }
            }
            ("sub", Value::Index(i)) => p.sub_pattern = SubPattern::ALL[(*i).min(1)],
            ("drive", Value::Remove(i)) => match kind {
                Type::Edges => {
                    if *i < p.drive.edges.len() {
                        p.drive.edges.remove(*i);
                    }
                }
                Type::Holes => {
                    if *i < p.drive.holes.len() {
                        p.drive.holes.remove(*i);
                    }
                }
                _ => {
                    if *i < p.drive.faces.len() {
                        p.drive.faces.remove(*i);
                    }
                }
            },
            ("reverse", Value::Bool(b)) => p.drive.reverse = *b,
            ("side", Value::Index(i)) => p.side = Side::ALL[(*i).min(3)],
            ("comp", Value::Index(i)) => p.compensation = Compensation::ALL[(*i).min(1)],
            ("wall", Value::Number(n)) => p.wall_stock = *n,
            ("floor", Value::Number(n)) => p.floor_stock = *n,
            ("stepdown", Value::Number(n)) => p.step_down = *n,
            ("first", Value::Number(n)) => p.first_step_down = n.max(0.0),
            ("stepover", Value::Number(n)) => p.step_over = *n,
            ("passes", Value::Number(n)) => p.passes = n.clamp(1.0, 50.0) as u32,
            ("steep", Value::Number(n)) => p.steep_angle = n.clamp(0.0, 90.0),
            ("thread-d", Value::Number(n)) => p.thread_diameter = n.max(0.0),
            ("left-thread", Value::Bool(b)) => p.left_hand_thread = *b,
            ("direction", Value::Index(i)) => p.direction = Direction::ALL[(*i).min(1)],
            ("method", Value::Index(i)) => p.cutting_method = CuttingMethod::ALL[(*i).min(1)],
            ("angle", Value::Number(n)) => p.angle = *n,
            ("machine-by", Value::Index(i)) => p.machine_by = MachineBy::ALL[(*i).min(1)],
            ("top-from", Value::Index(i)) => p.heights.top.from = HeightRef::ALL[(*i).min(5)],
            ("top-off", Value::Number(n)) => p.heights.top.offset = *n,
            ("bottom-from", Value::Index(i)) => p.heights.bottom.from = HeightRef::ALL[(*i).min(5)],
            ("bottom-off", Value::Number(n)) => p.heights.bottom.offset = *n,
            ("depth", Value::Number(n)) => p.depth = n.max(0.0),
            ("tip-off", Value::Number(n)) => p.tip_offset = n.max(0.0),
            ("tabs", Value::Bool(b)) => p.tabs.enabled = *b,
            ("tab-count", Value::Number(n)) => p.tabs.count = n.clamp(1.0, 50.0) as usize,
            ("tab-w", Value::Number(n)) => p.tabs.width = *n,
            ("tab-h", Value::Number(n)) => p.tabs.height = *n,
            ("hole-select", Value::Index(i)) => p.cycle.select = HoleSelect::ALL[(*i).min(3)],
            ("cycle", Value::Index(i)) => {
                p.cycle.kind = [
                    CycleKind::Drill,
                    CycleKind::Peck,
                    CycleKind::ChipBreak,
                    CycleKind::Tap,
                ][(*i).min(3)]
            }
            ("peck", Value::Number(n)) => p.cycle.peck = n.max(0.0),
            ("dwell", Value::Number(n)) => p.cycle.dwell = n.max(0.0),
            ("retract-full", Value::Bool(b)) => p.cycle.retract_full = *b,
            ("break", Value::Number(n)) => p.cycle.break_through = n.max(0.0),
            ("rpm", Value::Number(n)) => p.speeds.spindle_rpm = n.max(0.0),
            ("feed", Value::Number(n)) => p.speeds.feed = *n,
            ("plunge", Value::Number(n)) => p.speeds.plunge = *n,
            ("ramp-feed", Value::Number(n)) => p.speeds.ramp = *n,
            ("lead-feed", Value::Number(n)) => p.speeds.lead = *n,
            ("coolant", Value::Index(i)) => p.posting.coolant = Coolant::ALL[(*i).min(3)],
            ("force-tc", Value::Bool(b)) => p.posting.force_tool_change = *b,
            ("stop", Value::Bool(b)) => p.posting.stop_before = *b,
            ("comment", Value::Text(s)) => p.posting.comment = s.clone(),
            ("rest", Value::Bool(b)) => p.rest = *b,
            ("clearance", Value::Number(n)) => p.links.clearance = n.max(0.0),
            ("rapid-d", Value::Number(n)) => p.links.rapid_distance = n.max(0.0),
            ("feed-d", Value::Number(n)) => p.links.feed_distance = n.max(0.0),
            ("gap", Value::Number(n)) => p.links.small_gap = n.max(0.0),
            ("small", Value::Index(i)) => p.links.small_gaps = LinkAction::ALL[(*i).min(3)],
            ("large", Value::Index(i)) => p.links.large_gaps = LinkAction::ALL[(*i).min(3)],
            ("entry", Value::Index(i)) => p.links.entry = Entry::ALL[(*i).min(1)],
            ("ramp-angle", Value::Number(n)) => p.links.ramp_angle = n.clamp(0.5, 89.0),
            ("lead-in", Value::Index(i)) => p.links.lead_in = LeadKind::ALL[(*i).min(2)],
            ("lead-out", Value::Index(i)) => p.links.lead_out = LeadKind::ALL[(*i).min(2)],
            ("lead-r", Value::Number(n)) => p.links.lead_radius = n.max(0.0),
            ("axis", Value::Index(i)) => {
                use cadrs_core::cam::cadrs_cam::strategy::ToolAxis;
                p.axis = if *i == 0 {
                    ToolAxis::WorkPlane
                } else {
                    // Until a face is picked: the work plane's own Z.
                    match p.axis {
                        ToolAxis::Direction(z) => ToolAxis::Direction(z),
                        ToolAxis::WorkPlane => ToolAxis::Direction(op_axis(ctx_cam, *setup, op_id)),
                    }
                };
            }
            ("coll", Value::Bool(b)) => p.collision.check = *b,
            ("coll-shank", Value::Number(n)) => p.collision.shank_clearance = n.max(0.0),
            ("coll-holder", Value::Number(n)) => p.collision.holder_clearance = n.max(0.0),
            ("coll-action", Value::Index(i)) => {
                use cadrs_core::cam::cadrs_cam::strategy::collision::CollisionAction;
                p.collision.action = CollisionAction::ALL[(*i).min(1)];
            }
            ("overlap", Value::Number(n)) => p.links.overlap = n.max(0.0),
            ("tolerance", Value::Number(n)) => p.tolerance = n.clamp(0.0005, 1.0),
            ("smooth", Value::Number(n)) => p.smooth_corners = n.max(0.0),
            ("repeat", Value::Index(i)) => {
                use cadrs_core::cam::cadrs_cam::strategy::Repeat;
                p.repeat = match i {
                    1 => Repeat::Linear {
                        count: 2,
                        step: [50.0, 0.0],
                    },
                    2 => Repeat::Circular {
                        count: 4,
                        center: [0.0, 0.0],
                        angle: 90.0,
                    },
                    _ => Repeat::None,
                };
            }
            (
                k @ ("rep-count" | "rep-dx" | "rep-dy" | "rep-cx" | "rep-cy" | "rep-angle"),
                Value::Number(n),
            ) => {
                use cadrs_core::cam::cadrs_cam::strategy::Repeat;
                match (&mut p.repeat, k) {
                    (
                        Repeat::Linear { count, .. } | Repeat::Circular { count, .. },
                        "rep-count",
                    ) => *count = n.clamp(1.0, 1000.0) as u32,
                    (Repeat::Linear { step, .. }, "rep-dx") => step[0] = *n,
                    (Repeat::Linear { step, .. }, "rep-dy") => step[1] = *n,
                    (Repeat::Circular { center, .. }, "rep-cx") => center[0] = *n,
                    (Repeat::Circular { center, .. }, "rep-cy") => center[1] = *n,
                    (Repeat::Circular { angle, .. }, "rep-angle") => *angle = *n,
                    _ => {}
                }
            }
            ("beam-power", Value::Number(n)) => p.beam.power = n.clamp(0.0, 100.0),
            ("beam-passes", Value::Number(n)) => p.beam.passes = n.clamp(1.0, 50.0) as u32,
            ("pass-depth", Value::Number(n)) => p.beam.pass_depth = n.max(0.0),
            ("micro", Value::Number(n)) => p.beam.microjoints = n.clamp(0.0, 64.0) as usize,
            ("micro-w", Value::Number(n)) => p.beam.microjoint_width = n.max(0.0),
            ("small-hole", Value::Number(n)) => p.beam.small_hole = n.max(0.0),
            ("small-feed", Value::Number(n)) => {
                p.beam.small_hole_feed = (n / 100.0).clamp(0.05, 1.0)
            }
            _ => {}
        }
        let pattern = p.pattern;
        if let Some(n) = renamed {
            op.name = n;
            if let OpKind::Toolpath(tp) = &mut op.kind {
                tp.name_edited = true;
            }
        } else if !name_edited && matches!(key, "type" | "pattern") {
            // A name nobody typed follows the pattern.
            let mut c2 = ctx_cam.clone();
            if let Some(NodeRef::Operation(j, s, o)) = c2.find(op.id) {
                c2.jobs[j].setups[s].operations.remove(o);
            }
            op.name = c2.next_name(pattern.label());
        }
    }

    /// Takes a pick in the view.
    pub(super) fn pick(
        &mut self,
        ctx: &Ctx,
        key: &str,
        pick: Pick,
        frame: Option<cadrs_core::cam::cadrs_cam::Frame>,
    ) {
        match self {
            Draft::Job { job, .. } => {
                let Some(part) = pick.part() else { return };
                let list = if key == "workholdings" {
                    &mut job.workholdings
                } else {
                    &mut job.bodies
                };
                match list.iter().position(|p| *p == part) {
                    Some(i) => {
                        list.remove(i);
                    }
                    None => list.push(part),
                }
            }
            Draft::Setup { setup, .. } => face_z(&mut setup.wcs, ctx, pick),
            Draft::WorkPlane { op, .. } => {
                if let OpKind::WorkPlane(w) = &mut op.kind {
                    face_z(w, ctx, pick);
                }
            }
            Draft::Toolpath { op, kind, .. } => {
                let OpKind::Toolpath(tp) = &mut op.kind else {
                    return;
                };
                if key == "axis-face" {
                    if let Some(n) = face_normal(ctx, pick) {
                        tp.params.axis =
                            cadrs_core::cam::cadrs_cam::strategy::ToolAxis::Direction(n);
                    }
                    return;
                }
                let d = &mut tp.params.drive;
                let toggle = |v: &mut Vec<String>, k: String| match v.iter().position(|x| *x == k) {
                    Some(i) => {
                        v.remove(i);
                    }
                    None => v.push(k),
                };
                match (kind, pick) {
                    (Type::Edges, Pick::Edge(part, name)) => {
                        toggle(&mut d.edges, geometry::edge_key(part, &name))
                    }
                    (Type::Edges, Pick::Face(part, name)) => {
                        // A face's boundary edges.
                        if let Some(p) = ctx.parts.iter().find(|p| p.id == part) {
                            for e in p
                                .solid
                                .edges
                                .iter()
                                .filter(|e| e.name.faces.contains(&name))
                            {
                                toggle(&mut d.edges, geometry::edge_key(part, &e.name));
                            }
                        }
                    }
                    (Type::Faces | Type::Bodies, Pick::Face(part, name)) => {
                        toggle(&mut d.faces, geometry::face_key(part, &name))
                    }
                    (Type::Holes, Pick::Face(part, name)) => {
                        // A cylindrical face's axis, in the toolpath's coordinates.
                        let axis = ctx
                            .parts
                            .iter()
                            .find(|p| p.id == part)
                            .and_then(|p| p.solid.faces.iter().find(|f| f.name == name))
                            .and_then(|f| f.axis);
                        if let (Some((pt, _)), Some(fr)) = (axis, frame) {
                            let q = fr.to_local(pt);
                            let c = [q[0], q[1]];
                            match d
                                .holes
                                .iter()
                                .position(|h| (h[0] - c[0]).hypot(h[1] - c[1]) < 1e-3)
                            {
                                Some(i) => {
                                    d.holes.remove(i);
                                }
                                None => d.holes.push(c),
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// The edits ✓ makes.
    pub(super) fn commit(&self, ctx: &Ctx, world: &mut World) -> Vec<Edit> {
        match self {
            Draft::Insert(d) => {
                let Some((src, links)) = source_of(world, d) else {
                    return Vec::new();
                };
                let components = d
                    .parts
                    .iter()
                    .filter(|p| p.2)
                    .map(|(id, name, _)| {
                        let mut f = src.clone();
                        f.selection = cadrs_core::derived::DerivedSelection {
                            all: false,
                            parts: vec![*id],
                            ..Default::default()
                        };
                        Component {
                            id: CamId::new(),
                            role: d.role,
                            name: name.clone(),
                            derived: f,
                            hidden: false,
                        }
                    })
                    .collect();
                vec![Edit::AddComponents { components, links }]
            }
            Draft::Component(c) => {
                let mut v = Vec::new();
                if let Some(old) = ctx.cam.component(c.id)
                    && old != &**c
                {
                    v.push(Edit::SetComponent {
                        component: c.clone(),
                        links: Vec::new(),
                    });
                }
                v
            }
            Draft::Job { job, is_new } => {
                if *is_new {
                    vec![Edit::AddJob(job.clone())]
                } else {
                    vec![Edit::SetJob(job.clone())]
                }
            }
            Draft::Machine { job, machine, .. } => vec![Edit::SetMachine {
                job: *job,
                machine: Some(machine.clone()),
            }],
            Draft::Setup { job, setup, is_new } => {
                if *is_new {
                    vec![Edit::AddSetup {
                        job: *job,
                        setup: setup.clone(),
                    }]
                } else {
                    vec![Edit::SetSetup {
                        job: *job,
                        setup: setup.clone(),
                    }]
                }
            }
            Draft::Tool {
                setup,
                after,
                op,
                is_new,
                ..
            }
            | Draft::Toolpath {
                setup,
                after,
                op,
                is_new,
                ..
            }
            | Draft::WorkPlane {
                setup,
                after,
                op,
                is_new,
            } => {
                if *is_new {
                    vec![Edit::AddOperation {
                        setup: *setup,
                        after: *after,
                        op: op.clone(),
                    }]
                } else {
                    vec![Edit::SetOperation(op.clone())]
                }
            }
        }
    }

    /// What the view's picks may be while `key` takes them.
    pub(super) fn filter(&self, key: &str) -> crate::parts::PickFilter {
        let none = crate::parts::PickFilter::none();
        match (self, key) {
            (Draft::Job { .. }, _) => crate::parts::PickFilter {
                faces: true,
                ..none
            },
            (_, "p-face" | "axis-face") => crate::parts::PickFilter {
                faces: true,
                planar_only: true,
                ..none
            },
            (
                Draft::Toolpath {
                    kind: Type::Edges, ..
                },
                _,
            ) => crate::parts::PickFilter {
                faces: true,
                edges: true,
                ..none
            },
            _ => crate::parts::PickFilter {
                faces: true,
                ..none
            },
        }
    }

    /// The draft as the studio would have it (for the live preview): the studio with the
    /// draft's operation in place.
    pub fn applied(&self, cam: &CamStudio) -> Option<(CamStudio, CamId)> {
        let Draft::Toolpath {
            setup,
            after,
            op,
            is_new,
            ..
        } = self
        else {
            return None;
        };
        let mut c = cam.clone();
        if *is_new {
            let Some(NodeRef::Setup(j, s)) = c.find(*setup) else {
                return None;
            };
            let ops = &mut c.jobs[j].setups[s].operations;
            let at = after
                .and_then(|a| ops.iter().position(|o| o.id == a))
                .map_or(ops.len(), |i| i + 1);
            ops.insert(at, (**op).clone());
        } else {
            let Some(NodeRef::Operation(j, s, o)) = c.find(op.id) else {
                return None;
            };
            c.jobs[j].setups[s].operations[o] = (**op).clone();
        }
        Some((c, op.id))
    }
}

/// Points a work plane's Z along a picked planar face's normal.
pub(super) fn face_z(w: &mut WorkPlane, ctx: &Ctx, pick: Pick) {
    let Some(n) = face_normal(ctx, pick) else {
        return;
    };
    w.z = n;
    if (w.z[0] * w.x[0] + w.z[1] * w.x[1] + w.z[2] * w.x[2]).abs() > 0.9 {
        w.x = if w.z[0].abs() > 0.5 {
            [0.0, 1.0, 0.0]
        } else {
            [1.0, 0.0, 0.0]
        };
    }
}

pub(super) fn export_library(world: &mut World, cam: &CamStudio) {
    let Some(dir) = crate::export_dir(world) else {
        toast(world, "Nowhere to export to");
        return;
    };
    let path = cadrs_core::export::unique_path_ext(&dir, "cadrs-tool-library", "json");
    match std::fs::write(&path, cam.library.to_json()) {
        Ok(()) => {
            let theme = world.resource::<Theme>().clone();
            let mut commands = world.commands();
            cadrs_ui::show_notification(
                &mut commands,
                &theme,
                cadrs_ui::Notification::info(format!(
                    "Exported {} tools to {}",
                    cam.library.tools.len(),
                    path.display()
                ))
                .name("cam-toast"),
            );
            world.flush();
        }
        Err(e) => toast(world, &format!("Couldn't write the tool library: {e}")),
    }
}

pub(super) fn import_library(world: &mut World, path: std::path::PathBuf) {
    let Some((_, cam)) = active_cam_owned(world) else {
        return;
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            toast(world, &format!("Couldn't read {}: {e}", path.display()));
            return;
        }
    };
    match ToolLibrary::from_json(&text) {
        Ok(lib) => {
            let mut merged = cam.library.clone();
            let n = merged.merge(&lib);
            edit(world, Edit::SetLibrary(Box::new(merged)));
            let theme = world.resource::<Theme>().clone();
            let mut commands = world.commands();
            cadrs_ui::show_notification(
                &mut commands,
                &theme,
                cadrs_ui::Notification::info(format!("Imported {n} new tools")).name("cam-toast"),
            );
            world.flush();
            if let Some(mut d) = world.get_resource_mut::<CamDialog>() {
                d.shown = None;
            }
        }
        Err(e) => toast(world, &e),
    }
}
