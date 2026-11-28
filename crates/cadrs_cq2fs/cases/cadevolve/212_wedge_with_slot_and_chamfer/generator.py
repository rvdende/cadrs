def wedge_with_slot_and_chamfer(L=20.0, W=10.0, H=5.0, theta=30.0, slot_width=2.0, chamfer_size=0.5):
    import cadquery as cq
    # Create base wedge
    base = cq.Workplane("XY").box(L, W, H)
    wedge_cut = (
        cq.Workplane("XZ")
          .transformed(offset=(0, 0, H/2.0), rotate=(0, theta, 0))
          .rect(L*2.0, H*2.0)
          .extrude(W, both=True)
    )
    wedge = base.cut(wedge_cut)
    # Cut slot
    slot_cut = (
        cq.Workplane("XZ")
          .transformed(offset=(0, 0, H/2.0), rotate=(0, theta, 0))
          .rect(L, slot_width)
          .extrude(W, both=True)
    )
    wedge = wedge.cut(slot_cut)
    # Chamfer all edges
    return wedge.edges().chamfer(chamfer_size)
