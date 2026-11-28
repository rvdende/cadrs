def annular_ring_embossed_gear_teeth_and_mounts(
    n_teeth: int = 32,
    module: float = 2.0,
    thickness: float = 6.0,
    hub_radius: float = None,
    lug_count: int = 3,
    lug_width: float = None,
    fillet_radius: float = 0.0
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Parameter validation
    if n_teeth < 1 or module <= 0 or thickness <= 0 or lug_count < 1:
        raise ValueError("n_teeth>=1, module>0, thickness>0, lug_count>=1 required")

    # Compute gear radii
    pitch_dia = module * n_teeth
    r_pitch = pitch_dia / 2.0
    addendum = module
    dedendum = 1.25 * module
    r_outer = r_pitch + addendum
    r_root = r_pitch - dedendum
    if r_root <= 0:
        raise ValueError("module or n_teeth too small: computed root radius <= 0")

    # Default hub radius
    if hub_radius is None:
        hub_radius = 0.3 * pitch_dia
    if hub_radius < 0 or hub_radius >= r_root:
        raise ValueError("hub_radius must be >= 0 and < root radius")

    # Default lug width
    if lug_width is None:
        lug_width = 2.0 * module
    if lug_width <= 0 or lug_width >= 2.0 * (r_root - hub_radius):
        raise ValueError("lug_width must be > 0 and less than available inner arc width")

    eps = 1e-6

    # 1) Base ring
    ring = cq.Workplane("XY").circle(r_root).extrude(thickness)
    ring = ring.faces(">Z").workplane().circle(hub_radius).cutThruAll()

    # 2) Emboss gear teeth
    circular_pitch = math.pi * module
    half_tooth = circular_pitch / 4.0
    tooth_profile = [
        (r_root - eps, -half_tooth),
        (r_outer,      -half_tooth),
        (r_outer,       half_tooth),
        (r_root - eps,  half_tooth),
    ]
    single_tooth = (
        cq.Workplane("XY")
          .polyline(tooth_profile)
          .close()
          .extrude(thickness, combine=False)
    )
    for i in range(n_teeth):
        ring = ring.union(single_tooth.rotate((0, 0, 0), (0, 0, 1), 360.0 * i / n_teeth))

    # 3) Add radial rectangular lugs inside the hub
    half_lw = lug_width / 2.0
    lug_profile = [
        (hub_radius + eps, -half_lw),
        (r_root - eps,     -half_lw),
        (r_root - eps,      half_lw),
        (hub_radius + eps,  half_lw),
    ]
    single_lug = (
        cq.Workplane("XY")
          .polyline(lug_profile)
          .close()
          .extrude(thickness, combine=False)
    )
    for j in range(lug_count):
        ring = ring.union(single_lug.rotate((0, 0, 0), (0, 0, 1), 360.0 * j / lug_count))

    # 4) Combine and fillet
    try:
        ring = ring.combineSolids()
    except Exception:
        pass
    if fillet_radius > 0:
        ring = ring.edges("|Z").fillet(fillet_radius)

    return ring
