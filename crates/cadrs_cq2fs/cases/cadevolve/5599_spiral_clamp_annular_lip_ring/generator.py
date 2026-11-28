def spiral_clamp_annular_lip_ring(
    outer_radius: float = 30.0,
    ring_thickness: float = 6.0,
    ring_height: float = 8.0,
    lip_height: float = 3.0,
    flange_width: float = 8.0,
    flange_thickness: float = 2.5,
    helix_turns: float = 1.5,
    helix_start_angle: float = 0.0,
    helix_steps_per_turn: int = 36,
    channel_width: float = 6.0,
    channel_depth: float = 2.8,
    channel_radial_offset: float = None,
    gusset_count: int = 10,
    gusset_height: float = 3.0,
    gusset_depth: float = 3.5,
    gusset_width: float = 2.5,
    tooth_count: int = 24,
    tooth_width: float = 2.2,
    tooth_height: float = 2.6,
    tooth_recess: float = 0.6,
    tooth_tip: float = 1.6,
    bolt_count: int = 6,
    bolt_diameter: float = 3.5,
    bolt_countersink_dia: float = 7.0,
    bolt_countersink_depth: float = 1.8,
    fillet_radius: float = 0.6,
    chamfer_size: float = 0.5,
    helix_resolution_override: int = None
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Basic validation
    if outer_radius <= 0:
        raise ValueError("outer_radius must be > 0")
    if ring_thickness <= 0 or ring_thickness >= outer_radius:
        raise ValueError("ring_thickness must be > 0 and < outer_radius")
    if ring_height <= 0:
        raise ValueError("ring_height must be > 0")
    if lip_height <= 0 or lip_height > ring_height:
        raise ValueError("lip_height must be > 0 and <= ring_height")
    if helix_turns <= 0:
        raise ValueError("helix_turns must be > 0")
    if helix_steps_per_turn < 4:
        raise ValueError("helix_steps_per_turn must be >= 4")
    if channel_width <= 0 or channel_depth <= 0:
        raise ValueError("channel dimensions must be > 0")
    if gusset_count < 0:
        raise ValueError("gusset_count must be >= 0")
    if gusset_height <= 0 or gusset_depth <= 0:
        raise ValueError("gusset dimensions must be > 0")
    if tooth_count < 0:
        raise ValueError("tooth_count must be >= 0")
    if bolt_count < 0:
        raise ValueError("bolt_count must be >= 0")
    if fillet_radius < 0 or chamfer_size < 0:
        raise ValueError("fillet_radius and chamfer_size must be >= 0")

    # Derived defaults & clamps
    if channel_radial_offset is None:
        channel_radial_offset = max(0.2, ring_thickness * 0.35)
    if helix_resolution_override is None:
        steps_per_turn = helix_steps_per_turn
    else:
        steps_per_turn = max(4, helix_resolution_override)

    inner_radius = outer_radius - ring_thickness
    if inner_radius <= 0:
        raise ValueError("Resulting inner_radius must be > 0 (reduce ring_thickness)")

    # Base annular ring (body) - primary annulus (extrude of radial profile)
    result = cq.Workplane("XY").circle(outer_radius).circle(inner_radius).extrude(ring_height)

    # Add top flange (unioned)
    flange_outer = outer_radius + flange_width
    flange = cq.Workplane("XY").workplane(offset=ring_height).circle(flange_outer).circle(outer_radius).extrude(flange_thickness)
    result = result.union(flange)

    # Lip band extents
    lip_top_z = ring_height
    lip_bottom_z = lip_top_z - lip_height

    # ------------- Step 2: Discretized helical rectangular cut (spiral clamp channel) -------------
    total_angle = 360.0 * helix_turns
    steps = max(3, int(steps_per_turn * helix_turns))
    delta_angle = total_angle / (steps - 1) if steps > 1 else total_angle

    # radial center of channel
    r_channel = outer_radius - channel_radial_offset
    r_channel = max(inner_radius + 0.5, min(outer_radius - 0.5, r_channel))

    # create aggregated cutter by unioning many rectangular prisms positioned along helix
    channel_cutter = None
    # angular step in radians used to size circumferential box width so consecutive boxes overlap
    delta_ang_rad = math.radians(delta_angle)
    min_circum_width = max(channel_width, r_channel * delta_ang_rad * 1.6)
    rad_extent = ring_thickness + 2.0
    vert_extra = 0.6  # extra to guarantee overlap vertically

    for i in range(steps):
        angle_deg = helix_start_angle + i * delta_angle
        frac = (i / (steps - 1)) if steps > 1 else 0.5
        # Helix maps frac [0..1] to lip vertical span
        z_center = lip_top_z - frac * lip_height
        # place a rectangular prism aligned tangentially via rotation so as to approximate a helix sweep
        cutter = (
            cq.Workplane("XY")
            .workplane(offset=z_center + vert_extra * 0.5)
            .transformed(rotate=(0, 0, angle_deg))
            .center(r_channel, 0)
            .rect(min_circum_width, rad_extent)
            .extrude(- (channel_depth + vert_extra), combine=False)
        )
        channel_cutter = cutter if channel_cutter is None else channel_cutter.union(cutter)

    if channel_cutter is not None:
        result = result.cut(channel_cutter)

    # ------------- Step 3: Helical gusset ribs (triangular prisms) -------------
    gusset_count = int(max(0, gusset_count))
    if gusset_count > 0:
        gusset_union = None
        # Precompute helix pitch per radian (how z changes per rad)
        total_angle_rad = 2.0 * math.pi * helix_turns
        pitch_per_rad = (lip_height / total_angle_rad) if total_angle_rad != 0 else 0.0
        for j in range(gusset_count):
            # spread gussets along helix fractionally
            frac = (j + 0.5) / gusset_count
            angle_deg = helix_start_angle + frac * total_angle
            angle_rad = math.radians(angle_deg)
            # attach gusset base at lip_bottom_z (start) so the gusset spans upwards following helix slope
            # compute z at this helix angle
            z_at_angle = lip_top_z - (angle_rad / total_angle_rad) * lip_height if total_angle_rad != 0 else lip_bottom_z
            cx = (outer_radius + 0.001) * math.cos(angle_rad)
            cy = (outer_radius + 0.001) * math.sin(angle_rad)
            # create a plane whose normal points slightly inward and follows helix slope (gives gusset an angled orientation)
            # Use a normal vector combining inward radial and helix-slope z component
            nx = -math.cos(angle_rad)
            ny = -math.sin(angle_rad)
            nz = -pitch_per_rad  # small z component so the gusset follows the helix incline
            # normalize the normal to avoid degenerate planes
            norm_len = math.sqrt(nx * nx + ny * ny + nz * nz)
            if norm_len == 0:
                norm_len = 1.0
            nx /= norm_len
            ny /= norm_len
            nz /= norm_len
            plane = cq.Plane(
                origin=cq.Vector(cx, cy, z_at_angle),
                normal=cq.Vector(nx, ny, nz),
                xDir=cq.Vector(0, 0, 1),
            )
            half_w = gusset_width / 2.0
            # triangle profile in the local plane coordinates: base at plane origin, tip extends inward/up in plane coordinates
            tri_pts = [(0.0, -half_w), (0.0, half_w), (gusset_height, 0.0)]
            gus = cq.Workplane(plane).polyline(tri_pts).close().extrude(gusset_depth, combine=False)
            gusset_union = gus if gusset_union is None else gusset_union.union(gus)
        if gusset_union is not None:
            result = result.union(gusset_union)

    # ------------- Step 4: Asymmetric one-way serrated teeth on the lip -------------
    if tooth_count > 0:
        tooth_union = None
        # place teeth so that they sit on the lip outer rim (slightly inset radially so they are fused)
        r_tooth = outer_radius - max(0.2, ring_thickness * 0.08)
        for i in range(tooth_count):
            angle_deg = i * 360.0 / tooth_count
            half_tw = tooth_width / 2.0
            # asymmetric tooth: slanted tip
            skew = 0.35
            tip_offset = half_tw * skew
            pts = [
                (-tooth_recess, -half_tw),
                (-tooth_recess, half_tw),
                (tooth_tip, tip_offset),
            ]
            tooth = (
                cq.Workplane("XY")
                .workplane(offset=lip_top_z - 0.05)
                .transformed(rotate=(0, 0, angle_deg))
                .center(r_tooth, 0)
                .polyline(pts)
                .close()
                .extrude(-tooth_height, combine=False)
            )
            tooth_union = tooth if tooth_union is None else tooth_union.union(tooth)
        if tooth_union is not None:
            result = result.union(tooth_union)

    # ------------- Step 5: Countersunk bolt holes on the flange -------------
    if bolt_count > 0 and bolt_diameter > 0:
        hole_union = None
        bolt_radius_pos = outer_radius + flange_width * 0.5
        bolt_radius_pos = max(bolt_radius_pos, outer_radius + 0.8)
        # place countersinks from the top of flange downwards
        top_of_flange = ring_height + flange_thickness
        # adequate total pilot depth to fully clear
        total_cut_depth = ring_height + flange_thickness + 2.0
        for k in range(bolt_count):
            ang = math.radians(k * 360.0 / bolt_count)
            bx = bolt_radius_pos * math.cos(ang)
            by = bolt_radius_pos * math.sin(ang)
            countersink = (
                cq.Workplane("XY")
                .workplane(offset=top_of_flange)
                .center(bx, by)
                .circle(bolt_countersink_dia / 2.0)
                .extrude(-bolt_countersink_depth, combine=False)
            )
            pilot = (
                cq.Workplane("XY")
                .workplane(offset=top_of_flange)
                .center(bx, by)
                .circle(bolt_diameter / 2.0)
                .extrude(-total_cut_depth, combine=False)
            )
            piece = countersink.union(pilot)
            hole_union = piece if hole_union is None else hole_union.union(piece)
        if hole_union is not None:
            result = result.cut(hole_union)

    # ------------- Step 6: Fillet and chamfer transitions; ensure single solid -------------
    try:
        result = result.combineSolids()
    except Exception:
        pass

    # Fillet vertical edges (limit radius based on geometry)
    if fillet_radius > 0:
        try:
            max_possible = min(0.5 * ring_thickness, 0.5 * max(1e-6, gusset_width), 0.5 * max(1e-6, tooth_height))
            fr = fillet_radius if fillet_radius <= max_possible else max_possible
            if fr > 1e-6:
                result = result.edges("|Z").fillet(fr)
        except Exception:
            pass

    # Chamfer or fallback to fillet on top outer rim edges
    if chamfer_size > 0:
        try:
            cs = min(chamfer_size, outer_radius * 0.2)
            try:
                result = result.edges(">Z").chamfer(cs)
            except Exception:
                result = result.edges(">Z").fillet(min(cs, 0.5))
        except Exception:
            pass

    # Final combine and return as single solid Workplane
    try:
        result = result.combineSolids()
    except Exception:
        pass

    # Ensure we return a Workplane containing a single solid
    try:
        solid = result.val()
        return cq.Workplane("XY").add(solid)
    except Exception:
        return result
