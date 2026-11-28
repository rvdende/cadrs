def cylinder_chamfered_grip_ribs(
    inner_radius: float = 5.0,
    wall_thickness: float = 2.0,
    height: float = 20.0,
    rib_count: int = 16,
    rib_width: float = 1.0,
    rib_depth: float = 0.5,
    chamfer: float = 0.2,
    fillet_radius: float = 0.3,
    slot_count: int = 8,
    slot_width: float = 1.0,
    slot_axial_length: float = None,
    slot_center_z: float = None,
    extra_groove_depth: float = None
) -> "cq.Workplane":
    import cadquery as cq

    # Validate parameters
    if inner_radius < 0:
        raise ValueError("inner_radius must be >= 0")
    if wall_thickness <= 0:
        raise ValueError("wall_thickness must be > 0")
    if height <= 0:
        raise ValueError("height must be > 0")
    if rib_count < 0 or rib_width < 0 or rib_depth < 0:
        raise ValueError("rib_count >= 0; rib_width, rib_depth >= 0")
    if slot_count < 0 or slot_width < 0:
        raise ValueError("slot_count >= 0; slot_width >= 0")
    if chamfer < 0 or fillet_radius < 0:
        raise ValueError("chamfer and fillet_radius must be >= 0")

    # Compute key dimensions
    outer_radius = inner_radius + wall_thickness

    # Defaults for grooves
    if extra_groove_depth is None or extra_groove_depth <= 0:
        extra_groove_depth = rib_depth
    if slot_axial_length is None or slot_axial_length <= 0 or slot_axial_length > height:
        slot_axial_length = height / 2.0
    half_slot = slot_axial_length / 2.0
    if slot_center_z is None or slot_center_z < half_slot or slot_center_z > (height - half_slot):
        slot_center_z = height / 2.0
    slot_radial_depth = min(wall_thickness + rib_depth + extra_groove_depth, outer_radius)

    # 1) Base hollow cylinder
    result = cq.Workplane("XY").circle(outer_radius).extrude(height)
    if inner_radius > 0:
        result = result.faces(">Z").workplane().circle(inner_radius).cutThruAll()

    # 2) Add ribs with chamfered horizontal and filleted vertical edges
    if rib_count and rib_width and rib_depth:
        for i in range(rib_count):
            angle = 360.0 * i / rib_count
            rib = cq.Workplane("YZ").rect(rib_width, height).extrude(rib_depth, combine=False)
            if chamfer > 0:
                # chamfer all horizontal edges of the rib
                rib = rib.edges("not |Z").chamfer(chamfer)
            if fillet_radius > 0:
                # fillet all vertical edges of the rib
                rib = rib.edges("|Z").fillet(fillet_radius)
            rib = rib.translate((outer_radius, 0, height / 2.0))
            rib = rib.rotate((0, 0, 0), (0, 0, 1), angle)
            result = result.union(rib)

    # 3) Cut radial snap‐fit grooves
    if slot_count and slot_width and slot_axial_length and slot_radial_depth:
        for j in range(slot_count):
            angle = 360.0 * j / slot_count
            groove = (
                cq.Workplane("YZ")
                  .rect(slot_width, slot_axial_length)
                  .extrude(-slot_radial_depth, combine=False)
                  .translate((outer_radius, 0, slot_center_z))
                  .rotate((0, 0, 0), (0, 0, 1), angle)
            )
            result = result.cut(groove)

    return result
