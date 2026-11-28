def chamfered_box(L=20, W=15, H=10, c=1.5):
    solid = cq.Workplane('XY').box(L, W, H)
    return solid.edges().chamfer(c)
