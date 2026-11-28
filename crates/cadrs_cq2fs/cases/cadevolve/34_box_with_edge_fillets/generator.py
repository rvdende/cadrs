def box_with_edge_fillets(L=10.0, W=10.0, H=10.0, r=1.0):
    """
    Box with rounded vertical edges – A box_prism with constant-radius fillets 
    applied along all edges parallel to the Z-axis.
    """
    solid = cq.Workplane("XY").box(L, W, H)
    return solid.edges("|Z").fillet(r)
