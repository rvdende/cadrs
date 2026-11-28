def snap_clip_with_serpentine_living_strut_and_reinforced_pad(
    pad_length: float = 60.0,
    pad_width: float = 40.0,
    pad_thickness: float = 5.0,
    gusset_width: float = 10.0,
    gusset_height: float = 10.0,
    gusset_depth: float = None,
    mounting_hole_diameter: float = 4.0,
    countersink_diameter: float = 8.0,
    countersink_depth: float = 2.5,
    beam_total_length: float = 70.0,
    beam_amplitude: float = 6.0,
    beam_width: float = 6.0,
    beam_thickness: float = 1.8,
    beam_root_overlap: float = 6.0,
    num_meanders: int = 3,
    relief_slot_width: float = 1.0,
    relief_slot_spacing: float = 12.0,
    relief_slot_count: int = 3,
    lightening_hole_diameter: float = 4.0,
    fillet_radius: float = 1.0
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Parameter validation
    if pad_length <= 0 or pad_width <= 0 or pad_thickness <= 0:
        raise ValueError("Pad dimensions must be positive")
    if gusset_width < 0 or gusset_height < 0:
        raise ValueError("gusset dimensions must be non-negative")
    if mounting_hole_diameter <= 0:
        raise ValueError("mounting_hole_diameter must be positive")
    if countersink_diameter <= 0 or countersink_depth < 0:
        raise ValueError("countersink parameters must be non-negative (diameter > 0)")
    if beam_total_length <= 0 or beam_width <= 0 or beam_thickness <= 0:
        raise ValueError("Beam dimensions must be positive")
    if num_meanders < 1 or not isinstance(num_meanders, int):
        raise ValueError("num_meanders must be integer >= 1")
    if relief_slot_width < 0 or relief_slot_count < 0:
        raise ValueError("relief slot parameters must be non-negative")
    if lightening_hole_diameter < 0:
        raise ValueError("lightening hole diameter must be non-negative")
    if fillet_radius < 0:
        raise ValueError("fillet_radius must be non-negative")

    # Derived defaults / guards
    if gusset_depth is None:
        gusset_depth = max(beam_width * 1.5, 6.0)
    beam_root_overlap = max(beam_root_overlap, beam_width * 0.5, 1.0)

    # Basic geometry helpers
    pad_top_z = pad_thickness / 2.0  # top face Z (box is centered at origin)
    pad_front_y = pad_width / 2.0    # Y coordinate of front edge of pad (positive Y)

    # 1) Pad base (centered on origin)
    part = cq.Workplane("XY").box(pad_length, pad_width, pad_thickness, centered=(True, True, True))

    # 2) Mounting hole (through) and optional counterbore (from top)
    # Use the top face to create the hole so feature is merged into 'part'
    if countersink_diameter > mounting_hole_diameter and countersink_depth > 0:
        # cboreHole(diameter, cbore_diameter, cbore_depth, depth=None)
        part = part.faces(">Z").workplane().cboreHole(
            mounting_hole_diameter,
            countersink_diameter,
            countersink_depth,
            depth=(pad_thickness + 1.0),
        )
    else:
        part = part.faces(">Z").workplane().hole(mounting_hole_diameter, depth=(pad_thickness + 1.0))

    # 3) Lightening holes confined to interior area (operates on pad top face so merges)
    hole_margin = max(6.0, beam_width + 2.0)
    usable_x = pad_length - 2.0 * hole_margin
    usable_y = pad_width - 2.0 * hole_margin
    if usable_x > 0 and usable_y > 0 and lightening_hole_diameter > 0:
        cols = 3
        rows = 2
        xs = [(-usable_x / 2.0) + i * (usable_x / max(1, cols - 1)) for i in range(cols)]
        ys = [(-usable_y / 2.0) + j * (usable_y / max(1, rows - 1)) for j in range(rows)]
        pts = []
        safe_radius = max(mounting_hole_diameter * 1.5, 6.0)
        for x in xs:
            for y in ys:
                # avoid the central mounting hole region and keep holes away from leading edge
                if math.hypot(x, y) < safe_radius:
                    continue
                if y > (pad_width / 2.0 - hole_margin * 0.8):
                    continue
                pts.append((x, y))
        if pts:
            part = part.faces(">Z").workplane().pushPoints(pts).hole(lightening_hole_diameter, depth=max(1.0, pad_thickness - 1.0))

    # 4) Serpentine living strut path sampling (centerline in X-Y at a height above pad top)
    # Ensure beam root overlaps pad significantly to fuse.
    strut_start_y = pad_front_y - beam_root_overlap
    strut_end_y = strut_start_y + beam_total_length
    start_y = strut_start_y
    final_end_y = strut_end_y + max(beam_total_length * 0.02, 2.0)

    # higher sampling for smooth path
    sample_count = max(int(num_meanders * 40), 48)
    # embed so beam centerline slightly intersects the pad top to guarantee fusion
    embed = min(max(beam_thickness * 0.75, 0.5), beam_thickness)
    # beam centerline z (slightly into the pad top to ensure robust fusion)
    z_center = pad_top_z + beam_thickness / 2.0 - embed

    # sample the S-shaped centerline (x,y)
    path_samples = []
    for i in range(sample_count + 1):
        t = i / float(sample_count)
        y = start_y + t * (final_end_y - start_y)
        if beam_total_length > 1e-9:
            t_wave = (y - strut_start_y) / beam_total_length
        else:
            t_wave = 0.0
        t_wave = max(0.0, min(1.0, t_wave))
        x = beam_amplitude * math.sin(2.0 * math.pi * float(num_meanders) * t_wave)
        path_samples.append((x, y))

    # 5) Build beam from many slightly overlapping oriented boxes (robust approach)
    beam_wp = None
    overlap_const = max(beam_width * 1.2, 2.0)
    min_seg_len = 1e-6
    for i in range(len(path_samples) - 1):
        x0, y0 = path_samples[i]
        x1, y1 = path_samples[i + 1]
        dx = x1 - x0
        dy = y1 - y0
        seg_length = math.hypot(dx, dy)
        if seg_length < min_seg_len:
            continue
        angle = math.degrees(math.atan2(dy, dx))
        mx = (x0 + x1) / 2.0
        my = (y0 + y1) / 2.0
        box_length = max(seg_length + overlap_const, beam_width * 0.8, 1.0)
        seg_box = cq.Workplane("XY").transformed(offset=(mx, my, z_center), rotate=(0, 0, angle)).box(
            box_length, max(beam_width, 0.5), beam_thickness, centered=(True, True, True)
        )
        if beam_wp is None:
            beam_wp = seg_box
        else:
            try:
                beam_wp = beam_wp.union(seg_box)
            except Exception:
                # best-effort repeated union (OCC sometimes flaky)
                beam_wp = beam_wp.union(seg_box)

    # 6) Add a stout root anchor to guarantee robust fusion to pad and to carry gusset intersections
    anchor_length = beam_root_overlap * 1.4
    anchor = cq.Workplane("XY").transformed(
        offset=(0.0, pad_front_y - beam_root_overlap / 2.0, z_center - beam_thickness * 0.05)
    ).box(
        max(beam_width * 1.5, 4.0), anchor_length, beam_thickness * 1.4, centered=(True, True, True)
    )
    if beam_wp is None:
        beam_wp = anchor
    else:
        try:
            beam_wp = beam_wp.union(anchor)
        except Exception:
            beam_wp = beam_wp.union(anchor)

    # 7) Hook at the free end of the strut — ensure solid overlap with the last beam segment
    hook_width = beam_width * 1.2
    hook_depth = max(2.0, beam_total_length * 0.05, 4.0)
    hook_height = max(beam_thickness * 1.25, 1.0)
    hook_overlap = max(beam_width * 1.0, 3.0)
    hook_center_y = final_end_y - hook_depth / 2.0 - hook_overlap
    hook_z_center = pad_top_z + beam_thickness / 2.0 + hook_height / 2.0 - (hook_overlap * 0.1)
    hook = cq.Workplane("XY").transformed(offset=(0.0, hook_center_y, hook_z_center)).box(
        hook_width, hook_depth, hook_height, centered=(True, True, True)
    )
    try:
        beam_wp = beam_wp.union(hook)
    except Exception:
        beam_wp = beam_wp.union(hook)

    # 8) Relief slots confined to the strut: perform cuts on beam_wp only before fusing with pad
    slot_base_y = pad_front_y + max(2.0, beam_thickness)
    slot_center_z = pad_top_z + beam_thickness * 0.5
    for idx in range(relief_slot_count):
        y_slot = slot_base_y + idx * relief_slot_spacing
        if y_slot - 0.5 * relief_slot_width >= final_end_y:
            continue
        cutter = cq.Workplane("XY").transformed(offset=(0.0, y_slot, slot_center_z)).box(
            beam_width + 2.0, relief_slot_width, beam_thickness * 1.2, centered=(True, True, True)
        )
        # Cut directly on beam_wp; confine relief to beam by doing the cut before union with pad.
        try:
            beam_wp = beam_wp.cut(cutter)
        except Exception:
            # best-effort: try intersect-first and then cut
            try:
                cutter_only_on_beam = cutter.intersect(beam_wp)
                if cutter_only_on_beam is not None:
                    beam_wp = beam_wp.cut(cutter_only_on_beam)
            except Exception:
                pass

    # 9) Fuse beam assembly with pad part (after reliefs done) so pad doesn't get cut by slots
    try:
        part = part.union(beam_wp)
    except Exception:
        part = part.union(beam_wp)

    # 10) Triangular gussets (left and right) supporting the strut root region.
    # Place gussets at the front edge, rising from the pad top toward the strut.
    if gusset_width > 0 and gusset_height > 0 and gusset_depth > 0:
        # define a plane at the pad front face top (so gussets sit on pad top near the front edge)
        gus_plane = cq.Plane(origin=(0.0, pad_front_y, pad_top_z), normal=(0.0, 1.0, 0.0), xDir=(1.0, 0.0, 0.0))
        # place left and right gussets a quarter in from edges to avoid extreme corner issues
        left_center_x = -pad_length * 0.25
        right_center_x = pad_length * 0.25
        left_pts = [
            (left_center_x - gusset_width / 2.0, 0.0),
            (left_center_x + gusset_width / 2.0, 0.0),
            (left_center_x, gusset_height),
        ]
        right_pts = [
            (right_center_x - gusset_width / 2.0, 0.0),
            (right_center_x + gusset_width / 2.0, 0.0),
            (right_center_x, gusset_height),
        ]
        left_gusset = cq.Workplane(gus_plane).polyline(left_pts).close().extrude(gusset_depth)
        right_gusset = cq.Workplane(gus_plane).polyline(right_pts).close().extrude(gusset_depth)
        try:
            part = part.union(left_gusset).union(right_gusset)
        except Exception:
            part = part.union(left_gusset).union(right_gusset)

    # 11) Fillet intersections (best-effort safe attempt)
    if fillet_radius > 0:
        try:
            part = part.edges().fillet(fillet_radius)
        except Exception:
            try:
                part = part.edges("|Z").fillet(min(fillet_radius, 0.5))
            except Exception:
                pass

    return part
