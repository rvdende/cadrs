def torus_revolve(R=20, r=5):
    return (cq.Workplane('XZ')
              .moveTo(R, 0)
              .circle(r)
              .revolve(360, (0, 0, 0), (0, 1, 0)))
