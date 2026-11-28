import math
def wedge_basic(L=30, W=20, H=15, theta=15):
    base = cq.Workplane('XY').box(L, W, H)
    cut = (cq.Workplane('XZ')
             .transformed(offset=(0,0,H/2), rotate=(0,theta,0))
             .rect(L*2, H*2)
             .extrude(W, both=True))
    return base.cut(cut)
