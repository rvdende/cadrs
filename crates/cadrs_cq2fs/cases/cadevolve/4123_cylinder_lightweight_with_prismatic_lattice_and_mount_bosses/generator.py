def cylinder_lightweight_with_prismatic_lattice_and_mount_bosses(
    outer_radius: float = 20.0,
    wall_thickness: float = 2.0,
    height: float = 30.0,
    cell_size: float = 5.0,
    rib_thickness: float = 1.0,
    boss_count: int = 6,
    boss_diameter: float = 2.0,
    boss_height: float = None,
    boss_rotation_offset: float = 0.0,
    fillet_radius: float = 0.5
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Parameter validation
    if outer_radius <= 0 or wall_thickness <= 0 or wall_thickness >= outer_radius:
        raise ValueError("outer_radius must be > wall_thickness > 0")
    if height <= 0:
        raise ValueError("height must be positive")
    if cell_size <= 0 or rib_thickness <= 0:
        raise ValueError("cell_size and rib_thickness must be positive")

    inner_radius = outer_radius - wall_thickness
    if boss_height is None or boss_height <= 0:
        boss_height = wall_thickness

    # 1) Build and shell the outer cylinder (open ends)
    shell = (
        cq.Workplane("XY")
          .circle(outer_radius)
          .extrude(height)
          .faces("|Z")
          .shell(-wall_thickness)
    )
    # Fillet only the shell's vertical edges, if any
    if fillet_radius and fillet_radius > 0:
        try:
            shell = shell.edges("|Z").fillet(fillet_radius)
        except Exception:
            pass

    # 2) Build prismatic lattice inside
    lattice = None
    half_w = rib_thickness / 2.0
    if inner_radius > half_w:
        max_idx = int(math.floor(inner_radius / cell_size))
        coords = [i * cell_size for i in range(-max_idx, max_idx + 1)]
        # vertical struts
        for x in coords:
            if abs(x) + half_w > inner_radius:
                continue
            span = inner_radius**2 - (abs(x) + half_w)**2
            y_max = math.sqrt(span) if span > 0 else 0.0
            strut = (
                cq.Workplane("XY")
                  .polyline([
                      (x - half_w, -y_max),
                      (x + half_w, -y_max),
                      (x + half_w,  y_max),
                      (x - half_w,  y_max)
                  ])
                  .close()
                  .extrude(height)
            )
            lattice = strut if lattice is None else lattice.union(strut, clean=True)
        # horizontal struts
        for y in coords:
            if abs(y) + half_w > inner_radius:
                continue
            span = inner_radius**2 - (abs(y) + half_w)**2
            x_max = math.sqrt(span) if span > 0 else 0.0
            strut = (
                cq.Workplane("XY")
                  .polyline([
                      (-x_max, y - half_w),
                      ( x_max, y - half_w),
                      ( x_max, y + half_w),
                      (-x_max, y + half_w)
                  ])
                  .close()
                  .extrude(height)
            )
            lattice = strut if lattice is None else lattice.union(strut, clean=True)

    result = shell if lattice is None else shell.union(lattice, clean=True)

    # 3) Add radial mount bosses (each filleted separately if possible)
    bosses = None
    mid_z = height / 2.0
    for i in range(boss_count):
        phi = math.radians(360.0 * i / boss_count + boss_rotation_offset)
        cx = outer_radius * math.cos(phi)
        cy = outer_radius * math.sin(phi)
        plane = cq.Plane(
            origin=(cx, cy, mid_z),
            normal=(math.cos(phi), math.sin(phi), 0.0),
            xDir=(-math.sin(phi), math.cos(phi), 0.0),
        )
        boss = (
            cq.Workplane(plane)
              .circle(boss_diameter / 2.0)
              .extrude(boss_height)
        )
        if fillet_radius and fillet_radius > 0:
            try:
                boss = boss.edges("|Z").fillet(fillet_radius)
            except Exception:
                pass
        bosses = boss if bosses is None else bosses.union(boss, clean=True)

    if bosses is not None:
        result = result.union(bosses, clean=True)

    return result
