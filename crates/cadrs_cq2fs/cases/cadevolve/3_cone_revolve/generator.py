def cone_revolve(R=10, H=20):
    return (cq.Workplane('XZ')
            .moveTo(0, 0)
            .lineTo(0, H)
            .lineTo(R, 0)
            .close()
            .revolve(360) )
