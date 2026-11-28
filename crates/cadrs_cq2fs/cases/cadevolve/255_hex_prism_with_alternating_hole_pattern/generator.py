def hex_prism_with_alternating_hole_pattern(A=10.0, H=5.0, triangle_side=3.0, hole_diameter=2.0):
    import cadquery as cq
    import math

    # build the base prism
    prism = cq.Workplane("XY").polygon(6, 2 * A).extrude(H)

    # compute face parameters
    apothem = A * math.cos(math.pi / 6.0)
    z0 = H / 2.0

    # determine a safe lateral offset for two holes per face
    half_side = A / 2.0
    tri_bound = triangle_side / math.sqrt(3.0)
    circ_bound = hole_diameter / 2.0
    offset_x = min(half_side - tri_bound, half_side - circ_bound)

    cutters = []
    for i in range(6):
        theta = math.radians(60.0 * i + 30.0)
        nx, ny = math.cos(theta), math.sin(theta)
        x_c, y_c = apothem * nx, apothem * ny
        plane = cq.Plane(origin=cq.Vector(x_c, y_c, z0), normal=cq.Vector(nx, ny, 0))

        # triangle vertices in the local plane
        h_tri = triangle_side * math.sqrt(3.0) / 2.0
        v1 = (-triangle_side / 2.0, -h_tri / 3.0)
        v2 = ( triangle_side / 2.0, -h_tri / 3.0)
        v3 = (0.0,               2.0 * h_tri / 3.0)

        # extrude depth to cut through prism thickness
        depth = -2 * apothem - 1.0

        if i % 2 == 0:
            c1 = cq.Workplane(plane).center(-offset_x, 0).circle(circ_bound).extrude(depth)
            c2 = cq.Workplane(plane).center( offset_x, 0).polyline([v1, v2, v3]).close().extrude(depth)
        else:
            c1 = cq.Workplane(plane).center(-offset_x, 0).polyline([v1, v2, v3]).close().extrude(depth)
            c2 = cq.Workplane(plane).center( offset_x, 0).circle(circ_bound).extrude(depth)

        cutters.extend([c1, c2])

    # subtract all cutters
    for c in cutters:
        prism = prism.cut(c)

    return prism
