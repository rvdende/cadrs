def cylinder_with_annular_gradient_fin_clusters(
    inner_radius: float = 5.0,
    wall_thickness: float = 2.0,
    height: float = 20.0,
    ring_count: int = 5,
    fins_per_ring: int = 12,
    fin_thickness: float = 1.0,
    fin_height_max: float = 3.0,
    fin_height_min: float = 1.0,
    radial_gap: float = 0.1
) -> cq.Workplane:
    """
    Cylinder featuring annular clusters of radial fin arrays with gradient size.
    Around the cylinder circumference are ringed groups of radial fins whose
    heights decrease progressively along the cylinder’s axis.
    """
    # Parameter validation
    if inner_radius < 0 or wall_thickness <= 0 or height <= 0:
        raise ValueError("inner_radius must be >=0; wall_thickness and height must be >0")
    if ring_count < 0 or fins_per_ring < 0 or fin_thickness < 0 \
       or fin_height_max < 0 or fin_height_min < 0 or radial_gap < 0:
        raise ValueError("ring_count, fins_per_ring, fin_thickness, "
                         "fin_height_max, fin_height_min, radial_gap must be >= 0")

    # Compute outer radius of cylinder body
    outer_radius = inner_radius + wall_thickness

    # 1) Build the base (solid) cylinder
    result = cq.Workplane("XY").circle(outer_radius).extrude(height)

    # 2) Subtract inner bore if requested
    if inner_radius > 0:
        result = result.faces(">Z").workplane().circle(inner_radius).cutThruAll()

    # 3) Add annular rings of radial fins with gradient heights
    if ring_count > 0 and fins_per_ring > 0 and fin_thickness > 0:
        total_fin_thickness = ring_count * fin_thickness
        gap = max((height - total_fin_thickness) / (ring_count + 1), 0.0)
        for i in range(ring_count):
            z0 = gap * (i + 1) + fin_thickness * i
            # largest fins at bottom (i=0), decreasing to smallest at top
            if ring_count > 1:
                t = i / (ring_count - 1)
                fin_height = fin_height_max - (fin_height_max - fin_height_min) * t
            else:
                fin_height = fin_height_max
            for j in range(fins_per_ring):
                angle = 360.0 * j / fins_per_ring
                fin = (
                    cq.Workplane("YZ")
                      .transformed(offset=(
                          0,
                          z0 + fin_thickness / 2.0,
                          outer_radius + radial_gap
                      ))
                      .rect(fin_thickness, fin_thickness)
                      .extrude(fin_height)
                      .rotate((0, 0, 0), (0, 0, 1), angle)
                )
                result = result.union(fin)

    return result
