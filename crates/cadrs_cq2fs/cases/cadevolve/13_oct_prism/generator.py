def oct_prism(A=8, H=20):
    return cq.Workplane('XY').polygon(8, A / math.sin(math.pi / 8)).extrude(H)
