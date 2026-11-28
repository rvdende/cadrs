def dome_hemisphere(R=12):
    return (cq.Workplane('XZ')
              .moveTo(-R, 0)
              .threePointArc((0, R), (R, 0))
              .close()
              .revolve(180, (0,0,0), (1,0,0)))
