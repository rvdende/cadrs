def triangular_prism(A=10, H=25):
    h = A * math.sqrt(3)  # triangle height
    return (cq.Workplane('XY')
              .polyline([(0,0), (A,0), (A/2,h)]).close()
              .extrude(H))
