def half_cylinder(R=10, L=30):
    return (cq.Workplane('XY')
              .center(0, -R)
              .moveTo(-R, 0)
              .lineTo(R, 0)
              .threePointArc((0, 2*R), (-R, 0))
              .close()
              .extrude(L))
