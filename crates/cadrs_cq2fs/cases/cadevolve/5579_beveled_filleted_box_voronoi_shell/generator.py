def beveled_filleted_box_voronoi_shell(
    length: float = 100.0,
    width: float = 60.0,
    height: float = 40.0,
    shell_thickness: float = 3.0,
    chamfer_dist: float = 5.0,
    fillet_radius: float = 2.0,
    cell_count: int = 20,
    beam_width: float = None,
    seed: int = None
) -> "cq.Workplane":
    """
    Box prism with truncated corners, smooth filleted edges, and a Voronoi-patterned cutout shell.
    - length, width, height: outer dimensions of the box.
    - shell_thickness: uniform wall thickness after hollowing.
    - chamfer_dist: distance to bevel (chamfer) each vertical edge at 45°.
    - fillet_radius: radius to fillet all edges after chamfering.
    - cell_count: number of seed points for the Voronoi diagram on each face.
    - beam_width: width of each Voronoi ridge cut (defaults to half shell_thickness).
    - seed: optional random seed for reproducibility.
    Returns a single watertight solid Workplane.
    """
    import cadquery as cq
    import random, math
    import numpy as np
    from scipy.spatial import Voronoi

    # Parameter validation
    if length <= 2 * shell_thickness or width <= 2 * shell_thickness or height <= 2 * shell_thickness:
        raise ValueError("Box dimensions must exceed twice the shell_thickness")
    if shell_thickness <= 0:
        raise ValueError("shell_thickness must be positive")
    if chamfer_dist < 0:
        raise ValueError("chamfer_dist must be non-negative")
    if fillet_radius < 0:
        raise ValueError("fillet_radius must be non-negative")
    if cell_count < 1:
        raise ValueError("cell_count must be at least 1")
    if beam_width is None:
        beam_width = shell_thickness * 0.5
    if beam_width <= 0:
        raise ValueError("beam_width must be positive")

    if seed is not None:
        random.seed(seed)

    # 1) Build outer box, chamfer vertical edges, then fillet all edges
    outer = cq.Workplane("XY").box(length, width, height)
    if chamfer_dist > 0:
        outer = outer.edges("|Z").chamfer(chamfer_dist)
    if fillet_radius > 0:
        outer = outer.edges().fillet(fillet_radius)

    # 2) Hollow out by subtracting a slightly smaller inner box
    inner_l = length - 2.0 * shell_thickness
    inner_w = width - 2.0 * shell_thickness
    inner_h = height - 2.0 * shell_thickness
    inner = cq.Workplane("XY").box(inner_l, inner_w, inner_h)
    shell = outer.cut(inner)

    result = shell

    # 3) Define the four side faces for Voronoi cuts
    face_params = [
        # +X face spans Y × Z
        (( length/2.0, 0.0, 0.0), ( 1.0, 0.0, 0.0), (0.0, 1.0, 0.0), width,  height),
        # -X face spans Y × Z
        ((-length/2.0, 0.0, 0.0), (-1.0, 0.0, 0.0), (0.0,-1.0, 0.0), width,  height),
        # +Y face spans X × Z
        ((0.0,  width/2.0, 0.0), (0.0, 1.0, 0.0), (-1.0,0.0, 0.0), length,  height),
        # -Y face spans X × Z
        ((0.0, -width/2.0, 0.0), (0.0,-1.0, 0.0), (1.0, 0.0, 0.0), length,  height),
    ]

    for origin, normal, xDir, span_u, span_v in face_params:
        # bounds in face-local u,v coordinates
        x_min, x_max = -span_u/2.0, span_u/2.0
        y_min, y_max = -span_v/2.0, span_v/2.0

        # generate random seed points in the face rectangle
        seeds = [(random.uniform(x_min, x_max), random.uniform(y_min, y_max))
                 for _ in range(cell_count)]
        pts = np.array(seeds)
        vor = Voronoi(pts)

        # cut each finite ridge through the wall
        for vpair in vor.ridge_vertices:
            # skip infinite ridges or degenerate
            if len(vpair) != 2 or vpair[0] < 0 or vpair[1] < 0:
                continue
            p1 = vor.vertices[vpair[0]]
            p2 = vor.vertices[vpair[1]]
            # keep only ridges fully inside the face rectangle
            if (p1[0] < x_min or p1[0] > x_max or p1[1] < y_min or p1[1] > y_max
             or p2[0] < x_min or p2[0] > x_max or p2[1] < y_min or p2[1] > y_max):
                continue
            dx, dy = p2[0] - p1[0], p2[1] - p1[1]
            L = math.hypot(dx, dy)
            if L < 1e-6:
                continue
            mx, my = 0.5*(p1[0] + p2[0]), 0.5*(p1[1] + p2[1])
            ang = math.degrees(math.atan2(dy, dx))

            # build and extrude a beam perpendicular into the shell
            plane = cq.Plane(origin=origin, normal=normal, xDir=xDir)
            beam = (
                cq.Workplane(plane)
                  .transformed(offset=cq.Vector(mx, my, 0), rotate=cq.Vector(0, 0, ang))
                  .rect(L, beam_width)
                  .extrude(-shell_thickness)
            )
            result = result.cut(beam)

    return result
