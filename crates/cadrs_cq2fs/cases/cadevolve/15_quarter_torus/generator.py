def quarter_torus(R=25, r=6):
    p = cq.Workplane('XZ').moveTo(R, 0).circle(r)
    return p.revolve(90)
