def sphere_cap(R=12, h=4):
    sphere = cq.Workplane('XY').sphere(R)
    cutter = cq.Workplane('XY').workplane(offset=R-h).box(2*R,2*R,2*R)
    return sphere.intersect(cutter)
