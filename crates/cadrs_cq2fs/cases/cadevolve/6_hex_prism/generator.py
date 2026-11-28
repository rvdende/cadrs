def hex_prism(A=10, H=20):
    return cq.Workplane('XY').polygon(6, 2*A*math.sqrt(3)).extrude(H)
