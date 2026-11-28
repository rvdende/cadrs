def dome_with_nested_lattice_and_snap_lug_array(
    R: float = 20.0,
    thickness: float = 1.0,
    lattice_cell: float = 5.0,
    lattice_thickness: float = 0.5,
    base_thickness: float = None,
    lug_count: int = 6,
    lug_length: float = 2.0,
    lug_width: float = 1.0,
    lug_height: float = 1.0,
    lug_z_offset: float = None,
    fillet_radius: float = 0.2
) -> "cq.Workplane":
    """
    Dome with internal orthogonal lattice and radial snap lugs.
    Creates a hollow hemisphere shell of radius R and wall thickness, cut at base_thickness.
    Builds a 3D orthogonal lattice inside the cavity with cell size lattice_cell and strut thickness lattice_thickness.
    Adds radial snap lugs around the rim.
    """
    import cadquery as cq
    import math

    # Parameter validation
    if R <= 0:
        raise ValueError(f"R must be > 0; got {R}")
    if thickness <= 0 or thickness >= R:
        raise ValueError(f"thickness must be > 0 and < R; got thickness={thickness}, R={R}")
    if lattice_cell <= 0:
        raise ValueError(f"lattice_cell must be > 0; got {lattice_cell}")
    if lattice_thickness <= 0:
        raise ValueError(f"lattice_thickness must be > 0; got {lattice_thickness}")
    if base_thickness is None:
        base_thickness = thickness
    if base_thickness <= 0:
        raise ValueError(f"base_thickness must be > 0; got {base_thickness}")
    if lug_count < 0 or not isinstance(lug_count, int):
        raise ValueError(f"lug_count must be a non-negative integer; got {lug_count}")
    if lug_count > 0 and (lug_length <= 0 or lug_width <= 0 or lug_height <= 0):
        raise ValueError("lug_length, lug_width, lug_height must all be > 0 when lug_count>0")
    if lug_z_offset is None:
        lug_z_offset = base_thickness - lug_height
    if lug_count > 0 and (lug_z_offset < 0 or lug_z_offset + lug_height > base_thickness):
        raise ValueError(
            f"lug_z_offset must satisfy 0 <= lug_z_offset <= base_thickness - lug_height; got {lug_z_offset}"
        )
    if fillet_radius < 0:
        raise ValueError(f"fillet_radius must be >= 0; got {fillet_radius}")

    inner_R = R - thickness
    if inner_R <= 0:
        raise ValueError(f"Inner radius must be > 0; check thickness vs R")

    # 1) Build hollow hemisphere shell, trimmed at base_thickness
    outer_cut_depth = R + base_thickness
    outer = (
        cq.Workplane("XY")
          .sphere(R)
          .cut(
              cq.Workplane("XY")
                .workplane(offset=base_thickness)
                .rect(2 * R, 2 * R)
                .extrude(-outer_cut_depth)
          )
    )
    inner_cut_depth = inner_R + base_thickness
    inner = (
        cq.Workplane("XY")
          .sphere(inner_R)
          .cut(
              cq.Workplane("XY")
                .workplane(offset=base_thickness)
                .rect(2 * inner_R, 2 * inner_R)
                .extrude(-inner_cut_depth)
          )
    )
    shell = outer.cut(inner)

    # 2) Create flat base ring
    outer_cyl = cq.Workplane("XY").circle(R).extrude(base_thickness)
    inner_cyl = cq.Workplane("XY").circle(inner_R).extrude(base_thickness)
    base_ring = outer_cyl.cut(inner_cyl)

    part = shell.union(base_ring)

    # 3) Build orthogonal lattice inside the cavity
    rodsX = None
    rodsY = None
    rodsZ = None

    # grid index range
    i_min = math.ceil(-inner_R / lattice_cell)
    i_max = math.floor(inner_R / lattice_cell)

    # a base workplane at z = base_thickness
    wp_base = cq.Workplane("XY").workplane(offset=base_thickness)

    # rods along X-axis (span X direction)
    for i in range(i_min, i_max + 1):
        y = i * lattice_cell
        if abs(y) > inner_R:
            continue
        rod = wp_base.transformed(offset=(0, y, 0)).rect(2 * inner_R, lattice_thickness).extrude(inner_R - base_thickness)
        rodsX = rod if rodsX is None else rodsX.union(rod)

    # rods along Y-axis (span Y direction)
    for i in range(i_min, i_max + 1):
        x = i * lattice_cell
        if abs(x) > inner_R:
            continue
        rod = wp_base.transformed(offset=(x, 0, 0)).rect(lattice_thickness, 2 * inner_R).extrude(inner_R - base_thickness)
        rodsY = rod if rodsY is None else rodsY.union(rod)

    # vertical rods (Z direction) at grid points inside circle
    for i in range(i_min, i_max + 1):
        x = i * lattice_cell
        if abs(x) > inner_R:
            continue
        for j in range(i_min, i_max + 1):
            y = j * lattice_cell
            if x * x + y * y > inner_R * inner_R:
                continue
            rod = wp_base.transformed(offset=(x, y, 0)).rect(lattice_thickness, lattice_thickness).extrude(inner_R - base_thickness)
            rodsZ = rod if rodsZ is None else rodsZ.union(rod)

    # union all rods
    rodsSolid = None
    if rodsX is not None:
        rodsSolid = rodsX
    if rodsY is not None:
        rodsSolid = rodsSolid.union(rodsY) if rodsY is not None else rodsSolid
    if rodsZ is not None:
        rodsSolid = rodsSolid.union(rodsZ) if rodsSolid is not None else rodsZ

    # intersect lattice with the inner cavity and add to part
    if rodsSolid is not None:
        lattice_inside = rodsSolid.intersect(inner)
        part = part.union(lattice_inside)

    # 4) Add radial snap lugs on the outer periphery
    if lug_count > 0:
        for k in range(lug_count):
            angle = 360.0 * k / lug_count
            z0 = lug_z_offset
            lug = (
                cq.Workplane("XY")
                  .transformed(rotate=(0, 0, angle), offset=(R, 0, z0))
                  .box(lug_length, lug_width, lug_height, centered=(False, True, False))
            )
            if fillet_radius > 0:
                lug = lug.edges().fillet(fillet_radius)
            part = part.union(lug)

    # 5) Return single merged solid
    return part.combine()
