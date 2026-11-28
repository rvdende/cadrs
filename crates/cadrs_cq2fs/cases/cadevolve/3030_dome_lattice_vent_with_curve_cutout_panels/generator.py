def dome_lattice_vent_with_curve_cutout_panels(
    base_radius=50.0,
    thickness=2.0,
    lattice_cell_size=10.0,
    strut_radius=0.5,
    num_panels=8,
    panel_angle_width=20.0,
    clearance=1.0
):
    import cadquery as cq
    import math

    # parameter validation
    base_radius = float(base_radius)
    thickness = float(thickness)
    lattice_cell_size = float(lattice_cell_size)
    strut_radius = float(strut_radius)
    num_panels = int(num_panels)
    panel_angle_width = float(panel_angle_width)
    clearance = float(clearance)

    if base_radius <= 0 or thickness <= 0 or thickness >= base_radius:
        raise ValueError("base_radius must be > thickness > 0")
    if lattice_cell_size <= 0:
        raise ValueError("lattice_cell_size must be positive")
    if strut_radius <= 0:
        raise ValueError("strut_radius must be positive")
    if num_panels < 1:
        raise ValueError("num_panels must be at least 1")
    if panel_angle_width <= 0 or panel_angle_width >= 360.0 / num_panels:
        raise ValueError("panel_angle_width must be > 0 and < 360/num_panels")
    if clearance < 0:
        raise ValueError("clearance must be >= 0")

    inner_radius = base_radius - thickness

    # 1) Build a hemispherical shell by splitting two spheres at Z=0
    outer_hemi = cq.Workplane("XY").sphere(base_radius).split(keepTop=True)
    inner_hemi = cq.Workplane("XY").sphere(inner_radius).split(keepTop=True)
    dome_shell = outer_hemi.cut(inner_hemi)

    # 2) Create vertical struts within the interior hemisphere
    max_r = inner_radius - strut_radius
    n_max = int(math.floor(max_r / lattice_cell_size))
    points = [
        (i * lattice_cell_size, j * lattice_cell_size)
        for i in range(-n_max, n_max + 1)
        for j in range(-n_max, n_max + 1)
        if (i * lattice_cell_size) ** 2 + (j * lattice_cell_size) ** 2 <= max_r**2
    ]
    if points:
        rods = (
            cq.Workplane("XY")
            .pushPoints(points)
            .circle(strut_radius)
            .extrude(base_radius, combine=False)
        )
        rods_inside = rods.intersect(inner_hemi, clean=True)
        dome_with_lattice = dome_shell.union(rods_inside, clean=True)
    else:
        dome_with_lattice = dome_shell

    # 3) Build curved‐wedge cutters and subtract
    cut_union = None
    for k in range(num_panels):
        center_ang = 360.0 * k / num_panels
        half_ang = panel_angle_width / 2.0
        a1 = math.radians(center_ang - half_ang)
        amid = math.radians(center_ang)
        a2 = math.radians(center_ang + half_ang)
        r_cut = base_radius + clearance
        x1, y1 = r_cut * math.cos(a1), r_cut * math.sin(a1)
        xm, ym = r_cut * math.cos(amid), r_cut * math.sin(amid)
        x2, y2 = r_cut * math.cos(a2), r_cut * math.sin(a2)
        cutter = (
            cq.Workplane("XY")
            .moveTo(x1, y1)
            .threePointArc((xm, ym), (x2, y2))
            .lineTo(0, 0)
            .close()
            .extrude(base_radius * 1.1, combine=False)
        )
        if cut_union is None:
            cut_union = cutter
        else:
            cut_union = cut_union.union(cutter, clean=True)

    if cut_union is not None:
        result = dome_with_lattice.cut(cut_union, clean=True)
    else:
        result = dome_with_lattice

    return result
