def hex_prism_with_mesh_infill_and_snap_features(
    side_length: float = 10.0,
    height: float = 20.0,
    wall_thickness: float = 2.0,
    panel_thickness: float = 2.0,
    mesh_type: str = "rectangular",
    mesh_cell_size: float = 5.0,
    mesh_beam_thickness: float = 1.0,
    snap_tab_width: float = 2.0,
    snap_tab_height: float = 3.0,
    snap_tab_depth: float = 1.0,
    magnet_diameter: float = 3.0,
    magnet_length: float = 1.5
) -> "cq.Workplane":
    import cadquery as cq
    import math
    from cadquery import Plane

    # parameter validation
    if side_length <= 0 or height <= 0 or wall_thickness <= 0 or panel_thickness <= 0:
        raise ValueError("Dimensions must be positive")
    if panel_thickness > wall_thickness:
        raise ValueError("panel_thickness must be <= wall_thickness")
    if mesh_type not in ("rectangular", "honeycomb"):
        raise ValueError("mesh_type must be 'rectangular' or 'honeycomb'")

    sqrt3 = math.sqrt(3.0)
    cos30 = sqrt3 / 2.0
    apothem_outer = side_length * cos30

    inner_apothem = apothem_outer - wall_thickness
    if inner_apothem <= 0:
        raise ValueError("wall_thickness too large for side_length")
    inner_r = inner_apothem / cos30

    core_height = height - 2.0 * wall_thickness
    if core_height <= 0:
        raise ValueError("wall_thickness too large for height")

    # build the frame
    outer = cq.Workplane("XY").polygon(6, 2.0 * side_length).extrude(height)
    inner = (
        cq.Workplane("XY")
          .workplane(offset=wall_thickness)
          .polygon(6, 2.0 * inner_r)
          .extrude(core_height)
    )
    frame = outer.cut(inner)

    # panel dims
    panel_width = 2.0 * inner_apothem
    panel_height = core_height

    # small fudge for booleans
    fudge = 1e-3
    window_cut_depth = wall_thickness + fudge

    # cut windows in side walls
    for i in range(6):
        phi = math.radians(60.0 * i + 30.0)
        nx, ny = math.cos(phi), math.sin(phi)
        xdir = (-ny, nx, 0)
        origin = (nx * apothem_outer, ny * apothem_outer, height / 2.0)
        wp = cq.Workplane(Plane(origin, normal=(-nx, -ny, 0), xDir=xdir))
        frame = frame.cut(
            wp.rect(panel_width, panel_height)
              .extrude(window_cut_depth, combine=False)
        )

    # cut magnet recesses in frame
    mag_yoff = panel_height / 4.0
    for i in range(6):
        phi = math.radians(60.0 * i + 30.0)
        nx, ny = math.cos(phi), math.sin(phi)
        xdir = (-ny, nx, 0)
        origin = (nx * apothem_outer, ny * apothem_outer, height / 2.0)
        wp = cq.Workplane(Plane(origin, normal=(-nx, -ny, 0), xDir=xdir))
        for yoff in (mag_yoff, -mag_yoff):
            frame = frame.cut(
                wp.center(0, yoff)
                  .circle(magnet_diameter / 2.0)
                  .extrude(magnet_length + fudge, combine=False)
            )

    # cut snap-tab slots in the side walls (bottom of each window)
    for i in range(6):
        phi = math.radians(60.0 * i + 30.0)
        nx, ny = math.cos(phi), math.sin(phi)
        xdir = (-ny, nx, 0)
        origin = (nx * apothem_outer, ny * apothem_outer, height / 2.0)
        wp = cq.Workplane(Plane(origin, normal=(-nx, -ny, 0), xDir=xdir))
        for xoff in (-panel_width / 4.0, panel_width / 4.0):
            # position at bottom edge of window
            yoff = -panel_height / 2.0 + snap_tab_height / 2.0
            frame = frame.cut(
                wp.center(xoff, yoff)
                  .rect(snap_tab_width, snap_tab_height)
                  .extrude(window_cut_depth + fudge, combine=False)
            )

    # build panels
    panels = None
    for i in range(6):
        phi = math.radians(60.0 * i + 30.0)
        nx, ny = math.cos(phi), math.sin(phi)
        xdir = (-ny, nx, 0)
        origin = (nx * apothem_outer, ny * apothem_outer, height / 2.0)
        panel_plane = Plane(origin, normal=(-nx, -ny, 0), xDir=xdir)

        # base panel plate
        panel = cq.Workplane(panel_plane).rect(panel_width, panel_height).extrude(panel_thickness)

        # infill
        if mesh_type == "rectangular":
            hole = mesh_cell_size - mesh_beam_thickness
            n_cols = int(math.floor(panel_width / mesh_cell_size))
            n_rows = int(math.floor(panel_height / mesh_cell_size))
            for col in range(n_cols):
                x = -panel_width/2 + mesh_cell_size/2 + col*mesh_cell_size
                for row in range(n_rows):
                    y = -panel_height/2 + mesh_cell_size/2 + row*mesh_cell_size
                    panel = panel.cut(
                        cq.Workplane(panel_plane)
                          .center(x, y)
                          .rect(hole, hole)
                          .extrude(panel_thickness + fudge, combine=False)
                    )
        else:
            hs = mesh_cell_size
            hole_flat = mesh_cell_size - mesh_beam_thickness
            h_spacing = hs
            v_spacing = sqrt3 * hs / 2.0
            tol = 1e-6
            max_j = int(math.floor((panel_height / 2.0) / v_spacing))
            max_i = int(math.floor((panel_width / 2.0) / h_spacing))
            for j in range(-max_j, max_j + 1):
                y = j * v_spacing
                if abs(y) > panel_height / 2.0 + tol:
                    continue
                x_off = (j % 2) * h_spacing / 2.0
                for k in range(-max_i, max_i + 1):
                    x = k * h_spacing + x_off
                    if abs(x) > panel_width / 2.0 + tol:
                        continue
                    panel = panel.cut(
                        cq.Workplane(panel_plane)
                          .center(x, y)
                          .polygon(6, hole_flat)
                          .extrude(panel_thickness + fudge, combine=False)
                    )

        # snap-fit tabs on panel (bottom edge)
        for xoff in (-panel_width / 4.0, panel_width / 4.0):
            yoff = -panel_height / 2.0 + snap_tab_height / 2.0
            panel = panel.union(
                cq.Workplane(panel_plane)
                  .center(xoff, yoff)
                  .rect(snap_tab_width, snap_tab_height)
                  .extrude(snap_tab_depth)
            )

        # magnet holes through panel
        for yoff in (panel_height / 4.0, -panel_height / 4.0):
            panel = panel.cut(
                cq.Workplane(panel_plane)
                  .center(0, yoff)
                  .circle(magnet_diameter / 2.0)
                  .extrude(panel_thickness + fudge, combine=False)
            )

        panels = panel if panels is None else panels.union(panel)

    return frame.union(panels)
