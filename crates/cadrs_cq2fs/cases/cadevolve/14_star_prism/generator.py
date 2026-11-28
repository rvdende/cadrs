import math
def star_prism(R=15, H=10):
    pts = []
    for i in range(10):
        ang = math.pi/5 * i
        r   = R if i % 2 == 0 else R*0.382
        pts.append((r*math.cos(ang), r*math.sin(ang)))
    return cq.Workplane('XY').polyline(pts).close().extrude(H)
