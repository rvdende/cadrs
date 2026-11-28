def annular_ring(R=20, t=4, H=5):
    return (cq.Workplane('XY')
              .circle(R)
              .circle(R - t)
              .extrude(H))
