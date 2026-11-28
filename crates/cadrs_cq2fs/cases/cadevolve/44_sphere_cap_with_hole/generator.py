def sphere_cap_with_hole(R=12.0, h=4.0, hole_diameter=2.0):
    sphere = cq.Workplane("XY").sphere(R)
    cutter = cq.Workplane("XY").workplane(offset=R-h).rect(2*R, 2*R).extrude(h)
    cap = sphere.intersect(cutter)
    result = cap.faces("<Z").workplane().hole(hole_diameter)
    return result
