def annular_ring_with_embossed_spike_fin_array(
    outer_radius: float = 20.0,
    ring_thickness: float = 4.0,
    height: float = 5.0,
    spike_width: float = 1.0,
    spike_length: float = 2.0,
    emboss_height: float = 1.0,
    spike_count: int = 36,
    chamfer_size: float = 0.2
) -> "cq.Workplane":
    import cadquery as cq

    # Parameter validation
    if outer_radius <= 0:
        raise ValueError("outer_radius must be positive")
    if ring_thickness <= 0 or ring_thickness >= outer_radius:
        raise ValueError("ring_thickness must be >0 and <outer_radius")
    if height <= 0:
        raise ValueError("height must be positive")
    if spike_width <= 0:
        raise ValueError("spike_width must be positive")
    if spike_length <= 0:
        raise ValueError("spike_length must be positive")
    if emboss_height <= 0:
        raise ValueError("emboss_height must be positive")
    spike_count = int(spike_count)
    if spike_count < 1:
        raise ValueError("spike_count must be at least 1")
    if chamfer_size < 0:
        raise ValueError("chamfer_size must be non-negative")

    inner_radius = outer_radius - ring_thickness

    # 1) Create base annular ring
    base = cq.Workplane("XY").circle(outer_radius).circle(inner_radius).extrude(height)

    # 2) Build embossed spike-fins on the underside
    half_w = spike_width / 2.0
    pts = [
        (outer_radius, -half_w),
        (outer_radius,  half_w),
        (outer_radius + spike_length, 0.0),
    ]
    fins = None
    for i in range(spike_count):
        ang = 360.0 * i / spike_count
        fin_piece = (
            cq.Workplane("XY")
              .transformed(rotate=(0, 0, ang))
              .polyline(pts)
              .close()
              .extrude(-emboss_height, combine=False)
        )
        if chamfer_size > 0:
            fin_piece = fin_piece.edges("|Z").chamfer(chamfer_size)
        fins = fin_piece if fins is None else fins.union(fin_piece, clean=True)

    # 3) Union fins with base
    result = base.union(fins, clean=True) if fins is not None else base
    return result
