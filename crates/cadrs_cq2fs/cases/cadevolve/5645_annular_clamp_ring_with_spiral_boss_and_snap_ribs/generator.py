def annular_clamp_ring_with_spiral_boss_and_snap_ribs(
    outer_radius: float = 30.0,
    ring_radial_thickness: float = 6.0,
    ring_height: float = 8.0,
    flange_width: float = 6.0,
    flange_thickness: float = 2.5,
    shell_thickness: float = 1.5,
    # Spiral boss params
    spiral_boss_turns: float = 1.0,
    spiral_boss_height: float = 4.0,
    spiral_boss_profile_radial: float = 0.9,
    spiral_boss_profile_tangential: float = 1.4,
    spiral_steps_per_turn: int = 48,
    spiral_start_angle: float = 0.0,
    # Snap rib params (local circular array about a seed angle)
    seed_rib_angle: float = -90.0,
    rib_pattern_count: int = 5,
    rib_pattern_span_deg: float = 30.0,
    rib_radial_depth: float = 1.2,
    rib_tangential_width: float = 4.0,
    rib_height: float = 1.0,
    rib_root_fillet: float = 0.35,
    # Wave notches and flange pockets
    wave_notch_count: int = 8,
    wave_notch_depth: float = 1.0,
    wave_notch_width: float = 6.0,
    flange_hole_count: int = 6,
    flange_hole_dia: float = 3.0,
    flange_cbore_dia: float = 6.0,
    flange_cbore_depth: float = 1.5,
    # Clamp notch (local removal on the flange)
    clamp_notch_angle_deg: float = 20.0,
    clamp_notch_extra_radius: float = 6.0,
    # finishing
    fillet_radius: float = 0.4,
    chamfer_size: float = 0.15,
) -> 'cq.Workplane':
    import cadquery as cq
    import math

    # --- Parameter normalization & validation ---
    outer_radius = float(outer_radius)
    ring_radial_thickness = float(ring_radial_thickness)
    ring_height = float(ring_height)
    flange_width = float(flange_width)
    flange_thickness = float(flange_thickness)
    shell_thickness = float(shell_thickness)
    spiral_boss_turns = float(spiral_boss_turns)
    spiral_boss_height = float(spiral_boss_height)
    spiral_boss_profile_radial = float(spiral_boss_profile_radial)
    spiral_boss_profile_tangential = float(spiral_boss_profile_tangential)
    spiral_steps_per_turn = int(spiral_steps_per_turn)
    spiral_start_angle = float(spiral_start_angle)
    seed_rib_angle = float(seed_rib_angle)
    rib_pattern_count = int(rib_pattern_count)
    rib_pattern_span_deg = float(rib_pattern_span_deg)
    rib_radial_depth = float(rib_radial_depth)
    rib_tangential_width = float(rib_tangential_width)
    rib_height = float(rib_height)
    rib_root_fillet = float(rib_root_fillet)
    wave_notch_count = int(wave_notch_count)
    wave_notch_depth = float(wave_notch_depth)
    wave_notch_width = float(wave_notch_width)
    flange_hole_count = int(flange_hole_count)
    flange_hole_dia = float(flange_hole_dia)
    flange_cbore_dia = float(flange_cbore_dia)
    flange_cbore_depth = float(flange_cbore_depth)
    clamp_notch_angle_deg = float(clamp_notch_angle_deg)
    clamp_notch_extra_radius = float(clamp_notch_extra_radius)
    fillet_radius = float(fillet_radius)
    chamfer_size = float(chamfer_size)

    if outer_radius <= 0:
        raise ValueError("outer_radius must be > 0")
    if ring_radial_thickness <= 0 or ring_radial_thickness >= outer_radius:
        raise ValueError("ring_radial_thickness must be > 0 and < outer_radius")
    if ring_height <= 0:
        raise ValueError("ring_height must be > 0")
    if flange_thickness < 0 or flange_width < 0:
        raise ValueError("flange dimensions must be >= 0")
    if shell_thickness <= 0 or shell_thickness >= ring_radial_thickness:
        raise ValueError("shell_thickness must be > 0 and < ring_radial_thickness")
    if spiral_boss_turns <= 0:
        raise ValueError("spiral_boss_turns must be > 0")
    if spiral_steps_per_turn < 4:
        raise ValueError("spiral_steps_per_turn must be >= 4")
    if rib_pattern_count < 1:
        raise ValueError("rib_pattern_count must be >= 1")
    if rib_radial_depth <= 0 or rib_tangential_width <= 0 or rib_height <= 0:
        raise ValueError("rib dimensions must be > 0")
    if flange_cbore_dia <= flange_hole_dia:
        raise ValueError("flange_cbore_dia must be larger than flange_hole_dia")
    if clamp_notch_angle_deg <= 0 or clamp_notch_angle_deg >= 360:
        raise ValueError("clamp_notch_angle_deg must be >0 and <360")
    if fillet_radius < 0 or chamfer_size < 0:
        raise ValueError("fillet_radius and chamfer_size must be >= 0")

    # --- Derived geometry ---
    inner_radius = outer_radius - ring_radial_thickness
    flange_outer_radius = outer_radius + flange_width
    ring_bottom_z = 0.0
    ring_top_z = ring_height
    flange_top_z = ring_height + flange_thickness

    # --- 1) Revolve a 2D cross-section about Z to form the base ring + flange ---
    # Profile in X-Z plane (radial X, vertical Z). Start at inner bore bottom and go outward/up.
    prof = []
    prof.append((inner_radius, ring_bottom_z))                       # inner bore bottom
    prof.append((inner_radius, ring_top_z))                          # inner bore up to top
    # small inner relief to allow O-ring or counterbore region
    prof.append((inner_radius + 0.6, ring_top_z + 0.05))
    prof.append((outer_radius - 0.01, ring_top_z + 0.05))
    # flange outer edge
    prof.append((flange_outer_radius, flange_top_z))
    prof.append((flange_outer_radius, ring_bottom_z))
    base = cq.Workplane("XZ").polyline(prof).close().revolve(360.0)

    ring = base

    # --- 2) Hollow the ring to controlled shell_thickness by cutting a concentric cylinder ---
    # inner cavity radius measured from center
    # radial wall thickness desired from outer rim inward is 'shell_thickness' so inner cavity at:
    inner_cavity_radius = outer_radius - shell_thickness
    if inner_cavity_radius <= inner_radius + 1e-6:
        raise ValueError("shell_thickness is too large compared to ring_radial_thickness")
    cavity_cut = cq.Workplane("XY").circle(inner_cavity_radius).extrude(ring_top_z + flange_thickness + 5.0)
    ring = ring.cut(cavity_cut)

    # --- 3) Create clamp notch wedge removed from the flange (local removal) ---
    half_w = math.radians(clamp_notch_angle_deg) / 2.0
    ext_r = flange_outer_radius + clamp_notch_extra_radius
    wedge_pts = [
        (0.0, 0.0),
        (ext_r * math.cos(-half_w), ext_r * math.sin(-half_w)),
        (ext_r * math.cos(half_w), ext_r * math.sin(half_w)),
    ]
    wedge_cut = cq.Workplane("XY").polyline(wedge_pts).close().extrude(ring_top_z + flange_thickness + 5.0)
    ring = ring.cut(wedge_cut)

    # --- 4) Add a small shallow counterbore/pocket at the top of the internal bore (local subtract) ---
    if flange_cbore_dia > 0 and flange_cbore_depth > 0:
        # place the counterbore inside the bore at the center - ensure it subtracts from flange top area only
        cbore = (
            cq.Workplane("XY")
            .workplane(offset=flange_top_z)
            .circle(flange_cbore_dia / 2.0)
            .extrude(-min(flange_cbore_depth, flange_thickness + 0.2))
        )
        ring = ring.cut(cbore)

    # --- 5) Build the spiral boss hugging the inner bore by placing overlapping rounded prisms along a helix ---
    spiral_union = None
    steps = max(4, int(spiral_steps_per_turn * spiral_boss_turns))
    total_angle = 360.0 * spiral_boss_turns
    delta_angle = total_angle / max(1, (steps - 1))
    # helix vertical span: start just below flange top and extend down by spiral_boss_height
    helix_top_z = ring_top_z - 0.15
    helix_bottom_z = max(ring_bottom_z + 0.2, helix_top_z - spiral_boss_height)
    # radial center for boss pieces (slightly embedded into inner wall so they fuse)
    boss_r_center = inner_cavity_radius - (spiral_boss_profile_radial / 2.0) - 0.02
    boss_piece_height = max(0.6, (spiral_boss_height / max(1.0, steps)) * 2.2)
    for i in range(steps):
        ang = spiral_start_angle + i * delta_angle
        frac = 0.0 if steps == 1 else (i / (steps - 1))
        zc = (1.0 - frac) * helix_top_z + frac * helix_bottom_z
        seg = (
            cq.Workplane("XY")
            .workplane(offset=zc - boss_piece_height / 2.0)
            .transformed(rotate=(0, 0, ang))
            .center(boss_r_center, 0)
            .rect(spiral_boss_profile_radial, spiral_boss_profile_tangential)
            .extrude(boss_piece_height, combine=False)
        )
        # add small fillet on each piece to smooth union points (best-effort)
        try:
            seg = seg.combineSolids()
            seg = seg.edges("|Z").fillet(min(spiral_boss_profile_radial * 0.45, 0.8))
        except Exception:
            pass
        spiral_union = seg if spiral_union is None else spiral_union.union(seg)
    if spiral_union is not None:
        try:
            spiral_union = spiral_union.combineSolids()
        except Exception:
            pass
        ring = ring.union(spiral_union)

    # --- 6) Local circular array of snap ribs near clamp notch (create seed ribs and union them) ---
    ribs_union = None
    # place ribs centered on the flange outer region, slightly inset radially so they fuse cleanly
    rib_r_center = outer_radius + flange_width * 0.5
    rib_embed = min(rib_height * 0.5, flange_thickness * 0.65)
    if rib_pattern_count > 1:
        step_span = float(rib_pattern_span_deg) / max(1, (rib_pattern_count - 1))
    else:
        step_span = 0.0
    for j in range(rib_pattern_count):
        ang = seed_rib_angle + (j - (rib_pattern_count - 1) / 2.0) * step_span
        rib_piece = (
            cq.Workplane("XY")
            .workplane(offset=flange_top_z - rib_embed)
            .transformed(rotate=(0, 0, ang))
            .center(rib_r_center, 0)
            .rect(rib_radial_depth, rib_tangential_width)
            .extrude(rib_height, combine=False)
        )
        # fillet rib root to reduce stress
        try:
            rib_piece = rib_piece.combineSolids()
            rib_piece = rib_piece.edges("|Z").fillet(min(rib_root_fillet, rib_radial_depth * 0.45))
        except Exception:
            pass
        ribs_union = rib_piece if ribs_union is None else ribs_union.union(rib_piece)
    if ribs_union is not None:
        try:
            ribs_union = ribs_union.combineSolids()
        except Exception:
            pass
        ring = ring.union(ribs_union)

    # --- 7) Wave notches on flange (distributed around flange) ---
    if wave_notch_count > 0 and wave_notch_depth > 0 and wave_notch_width > 0:
        notch_union = None
        notch_center_r = outer_radius + flange_width - (wave_notch_depth / 2.0)
        for k in range(wave_notch_count):
            ang = k * 360.0 / max(1, wave_notch_count)
            cutter = (
                cq.Workplane("XY")
                .workplane(offset=flange_top_z - (wave_notch_depth / 2.0))
                .transformed(rotate=(0, 0, ang))
                .center(notch_center_r, 0)
                .rect(wave_notch_depth, wave_notch_width)
                .extrude(-(flange_thickness + 2.0), combine=False)
            )
            notch_union = cutter if notch_union is None else notch_union.union(cutter)
        if notch_union is not None:
            ring = ring.cut(notch_union)

    # --- 8) Counterbore pockets (mounting holes) on flange ---
    if flange_hole_count > 0 and flange_hole_dia > 0 and flange_cbore_dia > 0:
        hole_positions = []
        hole_radius_pos = outer_radius + flange_width * 0.5
        for i in range(flange_hole_count):
            theta = 2.0 * math.pi * i / flange_hole_count
            hole_positions.append((hole_radius_pos * math.cos(theta), hole_radius_pos * math.sin(theta)))
        # Try to place cbore holes on the largest top face (flange) using center of mass workplane
        try:
            ring = ring.faces(">Z").workplane(centerOption="CenterOfMass").pushPoints(hole_positions).cboreHole(
                flange_hole_dia, flange_cbore_dia, flange_cbore_depth
            )
        except Exception:
            # fallback manual subtraction
            hole_union = None
            for (hx, hy) in hole_positions:
                pilot = (
                    cq.Workplane("XY")
                    .workplane(offset=flange_top_z)
                    .center(hx, hy)
                    .circle(flange_hole_dia / 2.0)
                    .extrude(-(ring_height + flange_thickness + 2.0), combine=False)
                )
                countersink = (
                    cq.Workplane("XY")
                    .workplane(offset=flange_top_z)
                    .center(hx, hy)
                    .circle(flange_cbore_dia / 2.0)
                    .extrude(-flange_cbore_depth, combine=False)
                )
                piece = countersink.union(pilot)
                hole_union = piece if hole_union is None else hole_union.union(piece)
            if hole_union is not None:
                ring = ring.cut(hole_union)

    # --- 9) Final smoothing and ensure single solid ---
    try:
        ring = ring.combineSolids()
    except Exception:
        pass

    if fillet_radius > 0:
        try:
            max_possible = min(0.5 * shell_thickness, 0.5 * max(1e-6, rib_radial_depth), 0.5 * max(1e-6, spiral_boss_profile_radial))
            fr = fillet_radius if fillet_radius <= max_possible else max_possible
            if fr > 1e-6:
                ring = ring.edges("|Z or >Z or #Z").fillet(fr)
        except Exception:
            pass

    if chamfer_size > 0:
        try:
            cs = min(chamfer_size, outer_radius * 0.2)
            ring = ring.edges(">Z").chamfer(cs)
        except Exception:
            try:
                ring = ring.edges(">Z").fillet(min(chamfer_size, 0.5))
            except Exception:
                pass

    try:
        ring = ring.combineSolids()
    except Exception:
        pass

    # return as a Workplane containing a single solid if possible
    try:
        solid = ring.val()
        return cq.Workplane("XY").add(solid)
    except Exception:
        return ring
