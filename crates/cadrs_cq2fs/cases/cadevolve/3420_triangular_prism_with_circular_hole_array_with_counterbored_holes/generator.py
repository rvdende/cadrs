import cadquery as cq
import math

def triangular_prism_with_circular_hole_array_with_counterbored_holes(
    side_length: float = 50.0,
    height: float = 20.0,
    hole_diameter: float = 2.0,
    cbore_diameter: float = 4.0,
    cbore_depth: float = 1.0,
    nx: int = 4,
    ny: int = 5,
    border: float = 2.0,
    stagger: float = 2.0
) -> cq.Workplane:
    # Parameter validation
    if side_length <= 0 or height <= 0:
        raise ValueError("side_length and height must be positive")
    if hole_diameter <= 0:
        raise ValueError("hole_diameter must be positive")
    if cbore_diameter <= hole_diameter:
        raise ValueError("cbore_diameter must exceed hole_diameter")
    if cbore_depth <= 0:
        raise ValueError("cbore_depth must be positive")
    if nx < 1 or ny < 1:
        raise ValueError("nx and ny must be at least 1")
    if border < 0 or stagger < 0:
        raise ValueError("border and stagger must be non-negative")

    # Triangle vertices (centroid at origin)
    h_tri = side_length * math.sqrt(3.0) / 2.0
    cx = side_length / 2.0
    cy = h_tri / 3.0
    outer_pts = [
        (-cx,               -cy),
        ( side_length - cx, -cy),
        ( side_length/2.0 - cx, h_tri - cy),
    ]

    # Build prism
    prism = cq.Workplane("XY").polyline(outer_pts).close().extrude(height)

    # Spacing and margins
    margin_u = border
    if nx > 1:
        spacing_u = (side_length - 2.0 * margin_u - stagger) / float(nx - 1)
        if spacing_u < 0:
            raise ValueError("border and stagger too large for nx and side_length")
    else:
        spacing_u = 0.0

    margin_v = border
    if ny > 1:
        spacing_v = (height - 2.0 * margin_v) / float(ny - 1)
        if spacing_v < 0:
            raise ValueError("border too large for ny and height")
    else:
        spacing_v = 0.0

    hole_radius = hole_diameter / 2.0
    cbore_radius = cbore_diameter / 2.0
    extrude_len = h_tri * 1.1  # ensure full penetration

    # Cut counterbore + through hole on each of the three side faces
    for f_idx in range(3):
        x0, y0 = outer_pts[f_idx]
        x1, y1 = outer_pts[(f_idx + 1) % 3]
        dx, dy = x1 - x0, y1 - y0
        u_len = math.hypot(dx, dy)
        ux, uy = dx / u_len, dy / u_len
        nx_f, ny_f = uy, -ux  # outward normal in XY

        for j in range(ny):
            z_off = margin_v + j * spacing_v
            row_offset = stagger if (j % 2 == 1) else 0.0

            for i in range(nx):
                u_off = margin_u + i * spacing_u + row_offset
                if u_off < margin_u - 1e-6 or u_off > u_len - margin_u + 1e-6:
                    continue

                cx_h = x0 + ux * u_off
                cy_h = y0 + uy * u_off
                cz_h = z_off

                plane = cq.Plane(
                    origin=(cx_h, cy_h, cz_h),
                    normal=(nx_f, ny_f, 0.0),
                    xDir=(ux, uy, 0.0),
                )
                # counterbore pocket
                cutter_cb = (
                    cq.Workplane(plane)
                      .circle(cbore_radius)
                      .extrude(-cbore_depth)
                )
                prism = prism.cut(cutter_cb)
                # through-hole
                cutter_h = (
                    cq.Workplane(plane)
                      .circle(hole_radius)
                      .extrude(-extrude_len)
                )
                prism = prism.cut(cutter_h)

    return prism
