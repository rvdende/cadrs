import cadquery as cq
import math

def cylinder_lattice_shell_with_access_notches(
    outer_radius: float = 10.0,
    wall_thickness: float = 2.0,
    height: float = 30.0,
    cell_size: float = 5.0,
    strut_thickness: float = 1.0,
    notch_width: float = 2.0,
    notch_depth: float = 2.0
) -> "cq.Workplane":
    import cadquery as cq
    import math
    # Parameter validation
    if outer_radius <= 0:
        raise ValueError("outer_radius must be positive")
    if wall_thickness <= 0 or wall_thickness >= outer_radius:
        raise ValueError("wall_thickness must be > 0 and < outer_radius")
    if height <= 0:
        raise ValueError("height must be positive")
    if cell_size <= 0:
        raise ValueError("cell_size must be positive")
    if strut_thickness <= 0 or strut_thickness >= cell_size:
        raise ValueError("strut_thickness must be > 0 and < cell_size")
    if notch_width <= 0:
        raise ValueError("notch_width must be positive")
    if notch_depth <= 0 or notch_depth > wall_thickness:
        raise ValueError("notch_depth must be > 0 and <= wall_thickness")

    inner_radius = outer_radius - wall_thickness

    # 1) Build the hollow cylinder shell (open ends)
    shell = (
        cq.Workplane("XY")
          .circle(outer_radius)
          .extrude(height)
          .faces("|Z")
          .shell(-wall_thickness)
    )

    # 2) Build triangular lattice at double the density
    lattice = None
    nodes = []
    node_map = {}
    if height > 2.0 * wall_thickness and inner_radius > strut_thickness:
        half_t = strut_thickness / 2.0
        # use half the cell_size to expand density
        dx = cell_size / 2.0
        dy = dx * math.sqrt(3.0) / 2.0
        rod_height = height - 2.0 * wall_thickness
        r_limit = inner_radius - half_t

        # collect node positions
        max_j = int(math.floor(r_limit / dy))
        for j in range(-max_j, max_j + 1):
            y = j * dy
            if abs(y) > r_limit:
                continue
            x_off = dx / 2.0 if (j % 2) != 0 else 0.0
            x_max = math.sqrt(max(r_limit**2 - y*y, 0.0))
            i_min = int(math.floor((-x_max - x_off) / dx))
            i_max = int(math.floor(( x_max - x_off) / dx))
            for i in range(i_min, i_max + 1):
                x = i * dx + x_off
                if x*x + y*y <= r_limit**2:
                    key = (round(x, 6), round(y, 6))
                    nodes.append((x, y))
                    node_map[key] = (x, y)

        # build struts
        directions = [(dx, 0.0), (dx/2.0, dy), (-dx/2.0, dy)]
        for x, y in nodes:
            for ddx, ddy in directions:
                key2 = (round(x + ddx, 6), round(y + ddy, 6))
                if key2 not in node_map:
                    continue
                x2, y2 = node_map[key2]
                vx, vy = x2 - x, y2 - y
                L = math.hypot(vx, vy)
                if L < 1e-6:
                    continue
                angle = math.degrees(math.atan2(vy, vx))
                xm, ym = 0.5 * (x + x2), 0.5 * (y + y2)
                beam = (
                    cq.Workplane("XY")
                      .workplane(offset=wall_thickness)
                      .transformed(offset=(xm, ym, 0), rotate=(0, 0, angle))
                      .rect(L + 1e-6, strut_thickness)
                      .extrude(rod_height)
                )
                lattice = beam if lattice is None else lattice.union(beam)

    # 3) Combine shell+lattice and cut a notch at the outermost peak(s)
    result = shell
    if lattice is not None:
        result = result.union(lattice, clean=True)
        # find all boundary nodes near the limit
        if nodes:
            # radius at which peaks lie
            peak_r = inner_radius - strut_thickness/2.0
            # tolerance for selecting peaks
            tol = (cell_size/2.0) * 0.75
            for x0, y0 in nodes:
                if abs(math.hypot(x0, y0) - peak_r) > tol:
                    continue
                phi = math.degrees(math.atan2(y0, x0))
                # notch dimensions
                dx_port = notch_depth
                dy_port = notch_width
                dz_port = height - 2.0 * wall_thickness
                # compute box center
                r_mid = inner_radius + notch_depth/2.0
                cx = r_mid * math.cos(math.radians(phi))
                cy = r_mid * math.sin(math.radians(phi))
                cz = wall_thickness + dz_port/2.0
                cutter = (
                    cq.Workplane("XY")
                      .transformed(offset=(cx, cy, cz), rotate=(0.0, 0.0, phi))
                      .box(dx_port, dy_port, dz_port)
                )
                result = result.cut(cutter, clean=True)
    return result
