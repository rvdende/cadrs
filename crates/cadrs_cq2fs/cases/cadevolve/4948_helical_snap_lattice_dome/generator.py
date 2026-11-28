def helical_snap_lattice_dome(
    R: float = 12.0,
    thickness: float = 1.0,
    cell_size: float = 5.0,
    base_thickness: float = None,
    lug_count: int = 6,
    lug_depth: float = 1.0,
    lug_width: float = 2.0,
    lug_height: float = 1.0,
    lug_z_offset: float = None,
    fillet_radius: float = 0.2,
    helix_turns: float = 3.0,
    helix_count: int = 6,
    sample_count: int = 40
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Parameter validation
    if R <= 0:
        raise ValueError(f"R must be > 0; got R={R}")
    if thickness <= 0 or thickness >= R:
        raise ValueError(f"thickness must be > 0 and < R; got thickness={thickness}, R={R}")
    if cell_size <= 0:
        raise ValueError(f"cell_size must be > 0; got {cell_size}")
    if base_thickness is None:
        base_thickness = thickness
    if base_thickness <= 0:
        raise ValueError(f"base_thickness must be > 0; got {base_thickness}")
    lug_count = int(lug_count)
    if lug_count < 0:
        raise ValueError(f"lug_count must be >= 0; got {lug_count}")
    if lug_count > 0 and (lug_depth <= 0 or lug_width <= 0 or lug_height <= 0):
        raise ValueError("lug_depth, lug_width, lug_height must all be > 0 when lug_count>0")
    if lug_z_offset is None:
        lug_z_offset = base_thickness - lug_height
    if lug_z_offset < 0 or lug_z_offset + lug_height > base_thickness:
        raise ValueError(f"lug_z_offset must satisfy 0 <= lug_z_offset <= base_thickness - lug_height; got {lug_z_offset}")
    if fillet_radius < 0:
        raise ValueError(f"fillet_radius must be >= 0; got {fillet_radius}")
    if helix_turns < 0:
        raise ValueError(f"helix_turns must be >= 0; got {helix_turns}")
    helix_count = int(helix_count)
    if helix_count < 1:
        raise ValueError(f"helix_count must be >= 1; got {helix_count}")
    sample_count = int(sample_count)
    if sample_count < 2:
        raise ValueError(f"sample_count must be >= 2; got {sample_count}")

    inner_R = R - thickness

    # 1) Build hollow dome shell by revolving a cross‐section
    r2_out = max(0.0, R * R - base_thickness * base_thickness)
    bx_out = math.sqrt(r2_out)
    r2_in = max(0.0, inner_R * inner_R - base_thickness * base_thickness)
    bx_in = math.sqrt(r2_in)

    profile = (
        cq.Workplane("XZ")
          .moveTo(bx_out, base_thickness)
          .threePointArc((0.0, R), (-bx_out, base_thickness))
          .lineTo(-bx_in, base_thickness)
          .threePointArc((0.0, inner_R), (bx_in, base_thickness))
          .close()
    )
    shell = profile.revolve(angleDegrees=360, axisStart=(0, 0, 0), axisEnd=(0, 0, 1))

    # 2) Carve helical lattice runs
    hole_r = max(0.0, (cell_size - thickness) / 2.0)
    if hole_r > 1e-6:
        for i in range(helix_count):
            phi_offset = 2.0 * math.pi * i / helix_count
            for k in range(sample_count):
                t = k / float(sample_count - 1)
                z = base_thickness + (R - base_thickness) * (1.0 - t)
                if z <= base_thickness + 1e-6:
                    continue
                r2 = R * R - z * z
                if r2 <= 0:
                    continue
                r_xy = math.sqrt(r2)
                phi = 2.0 * math.pi * helix_turns * t + phi_offset
                x = r_xy * math.cos(phi)
                y = r_xy * math.sin(phi)
                nx, ny, nz = x / R, y / R, z / R
                if abs(nx) + abs(ny) < 1e-6:
                    x_dir = (1.0, 0.0, 0.0)
                else:
                    x_dir = (-ny, nx, 0.0)
                plane = cq.Plane(origin=(x, y, z), normal=(nx, ny, nz), xDir=x_dir)
                hole = cq.Workplane(plane).circle(hole_r).extrude(thickness * 2.0, both=True)
                shell = shell.cut(hole)

    # 3) Create flat base ring
    ring_profile = (
        cq.Workplane("XZ")
          .moveTo(inner_R, 0.0)
          .lineTo(inner_R, base_thickness)
          .lineTo(R, base_thickness)
          .lineTo(R, 0.0)
          .close()
    )
    base_ring = ring_profile.revolve(angleDegrees=360, axisStart=(0, 0, 0), axisEnd=(0, 0, 1))

    # 4) Combine dome shell and base ring
    part = shell.union(base_ring)

    # 5) Add radial snap lugs
    if lug_count > 0:
        for k in range(lug_count):
            angle = 360.0 * k / lug_count
            z0 = lug_z_offset
            lug_wp = cq.Workplane("XY").transformed(offset=(R, 0, z0), rotate=(0, 0, angle))
            lug = lug_wp.box(lug_depth, lug_width, lug_height, centered=(False, True, False))
            if fillet_radius > 0:
                lug = lug.edges().fillet(fillet_radius)
            part = part.union(lug)

    return part.combine()
