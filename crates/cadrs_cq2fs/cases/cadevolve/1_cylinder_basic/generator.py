def cylinder_basic(R=5, H=20):
    return cq.Workplane('XY').circle(R).extrude(H)
