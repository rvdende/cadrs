def hex_prism_with_spike_array_and_internal_ribs(
    side_length: float = 10.0,
    height: float = 20.0,
    wall_thickness: float = 1.0,
    spike_base_diameter: float = 2.0,
    spike_tip_diameter: float = 0.5,
    spike_height: float = 3.0,
    spikes_per_edge: int = 3,
    spike_rows: int = 2,
    rib_thickness: float = None,
    rib_height: float = None,
    fillet_radius: float = None
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Parameter validation
    if side_length <= 0:
        raise ValueError("side_length must be positive")
    if height <= 0:
        raise ValueError("height must be positive")
    if wall_thickness <= 0:
        raise ValueError("wall_thickness must be positive")
    if height <= 2.0 * wall_thickness:
        raise ValueError("height must exceed twice wall_thickness")
    if spike_base_diameter <= 0:
        raise ValueError("spike_base_diameter must be positive")
    if spike_tip_diameter < 0 or spike_tip_diameter > spike_base_diameter:
        raise ValueError("spike_tip_diameter must be >=0 and <= spike_base_diameter")
    if spike_height <= 0:
        raise ValueError("spike_height must be positive")
    if spikes_per_edge < 1 or spike_rows < 1:
        raise ValueError("spikes_per_edge and spike_rows must be at least 1")

    # Default ribs
    if rib_thickness is None:
        rib_thickness = 2.0 * wall_thickness
    if rib_thickness <= 0:
        raise ValueError("rib_thickness must be positive")
    if rib_height is None:
        rib_height = height - 2.0 * wall_thickness
    if rib_height <= 0 or rib_height > (height - 2.0 * wall_thickness):
        raise ValueError("rib_height must be >0 and <= height - 2*wall_thickness")
    if fillet_radius is None:
        fillet_radius = min(wall_thickness, rib_thickness) / 2.0
    if fillet_radius < 0:
        raise ValueError("fillet_radius must be non-negative")

    # Compute hexagon dimensions
    outer_dia = 2.0 * side_length
    offset = wall_thickness / math.cos(math.pi / 6.0)
    r_inner = side_length - offset
    if r_inner <= 0:
        raise ValueError("wall_thickness too large for given side_length")
    inner_dia = 2.0 * r_inner
    inradius = r_inner * math.cos(math.pi / 6.0)

    # Build the outer solid prism
    result = cq.Workplane("XY").polygon(6, outer_dia).extrude(height)

    # Prepare to add spikes
    spike_base_radius = spike_base_diameter / 2.0
    spike_tip_radius = spike_tip_diameter / 2.0

    # Loop over the 6 faces for spikes
    for i in range(6):
        face_normal_angle_deg = i * 60.0 + 30.0
        v1_angle = math.radians(i * 60.0)
        v2_angle = math.radians((i + 1) * 60.0)
        v1 = (side_length * math.cos(v1_angle), side_length * math.sin(v1_angle))
        v2 = (side_length * math.cos(v2_angle), side_length * math.sin(v2_angle))
        for j in range(spikes_per_edge):
            t_j = (j + 1) / float(spikes_per_edge + 1)
            x_edge = v1[0] * (1.0 - t_j) + v2[0] * t_j
            y_edge = v1[1] * (1.0 - t_j) + v2[1] * t_j
            for k in range(spike_rows):
                t_k = (k + 1) / float(spike_rows + 1)
                z_pos = t_k * height
                sp = (
                    cq.Workplane("XY")
                    .circle(spike_base_radius)
                    .workplane(offset=spike_height)
                    .circle(spike_tip_radius)
                    .loft()
                )
                sp = sp.rotate((0, 0, 0), (0, 1, 0), 90)
                sp = sp.rotate((0, 0, 0), (0, 0, 1), face_normal_angle_deg)
                sp = sp.translate((x_edge, y_edge, z_pos))
                result = result.union(sp)

    # Carve out the internal cavity
    cavity = (
        result
        .faces(">Z")
        .workplane()
        .polygon(6, inner_dia)
        .extrude(-(height - wall_thickness), combine=False)
    )
    shell = result.cut(cavity)

    # Build internal ribs along the edges of the inner hexagon
    ribs = None
    base_wp = cq.Workplane("XY").workplane(offset=wall_thickness)
    for i in range(6):
        angle_deg = i * 60.0 + 30.0
        rib = (
            base_wp
            .transformed(rotate=(0, 0, angle_deg))
            .center(inradius, 0)
            .rect(rib_thickness, r_inner)
            .extrude(rib_height, combine=False)
        )
        ribs = rib if ribs is None else ribs.union(rib)

    # Fillet the vertical edges of the ribs if requested
    if fillet_radius > 0:
        ribs = ribs.edges("|Z").fillet(fillet_radius)

    return shell.union(ribs)
