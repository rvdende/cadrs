import cadquery as cq

def cylinder_with_annular_gradient_fin_mesh(
    inner_radius: float = 5.0,
    wall_thickness: float = 2.0,
    height: float = 20.0,
    ring_count: int = 5,
    fin_thickness: float = 0.5,
    fin_height: float = 3.0,
    fins_per_ring_min: int = 8,
    fins_per_ring_max: int = 32,
    radial_gap: float = 0.1
) -> cq.Workplane:
    """
    Cylinder with annular gradient fin mesh.
    Creates a hollow cylinder of given inner_radius, wall_thickness and height.
    Then adds 'ring_count' small axial-band clusters of radial fins. Each cluster
    is a ring of thin fins extruded radially. The number of fins per ring varies
    linearly from 'fins_per_ring_max' at the bottom to 'fins_per_ring_min' at the top.
    fin_thickness controls the axial width of each ring, and fin_height the radial extension.
    radial_gap is the radial clearance between the shell and the base of each fin.
    """
    # Parameter validation
    if inner_radius < 0 or wall_thickness <= 0 or height <= 0:
        raise ValueError("inner_radius must be >= 0; wall_thickness and height must be > 0")
    if ring_count < 0 or fin_thickness < 0 or fin_height < 0 or radial_gap < 0:
        raise ValueError("ring_count, fin_thickness, fin_height, radial_gap must be >= 0")
    if fins_per_ring_min < 0 or fins_per_ring_max < 0:
        raise ValueError("fins_per_ring_min and fins_per_ring_max must be >= 0")

    # Compute outer radius of the shell
    outer_radius = inner_radius + wall_thickness

    # 1) Build the base cylinder
    result = cq.Workplane("XY").circle(outer_radius).extrude(height)

    # 2) Subtract inner bore if requested
    if inner_radius > 0:
        result = result.faces(">Z").workplane().circle(inner_radius).cutThruAll()

    # 3) Add annular rings of radial fins with a density gradient
    if ring_count > 0 and fin_thickness > 0 and fin_height > 0 and fins_per_ring_max > 0:
        # Compute constant offset-gap so all fins and gaps fill the height
        total_fin_height = ring_count * fin_thickness
        # We'll use equal total gap height, but distribute it into ring_count+1 gaps
        base_gap = max((height - total_fin_height) / (ring_count + 1), 0.0)
        for i in range(ring_count):
            # Z location of the bottom of this fin-ring
            z0 = base_gap * (i + 1) + fin_thickness * i
            # Determine number of fins in this ring (linear gradient)
            if ring_count > 1:
                t = i / (ring_count - 1)
                count = int(round(fins_per_ring_max - (fins_per_ring_max - fins_per_ring_min) * t))
            else:
                count = fins_per_ring_max
            count = max(count, 0)
            # Place each fin in the ring
            for j in range(count):
                angle = 360.0 * j / count if count > 0 else 0.0
                fin = (
                    cq.Workplane("YZ")
                      # move the workplane to the ring location and just outside the shell
                      .transformed(offset=(0,
                                           z0 + fin_thickness / 2.0,
                                           outer_radius + radial_gap))
                      # draw the fin cross-section (axial × circumferential)
                      .rect(fin_thickness, fin_thickness)
                      # extrude radially outward
                      .extrude(fin_height)
                      # rotate around the cylinder axis to correct angular position
                      .rotate((0, 0, 0), (0, 0, 1), angle)
                )
                result = result.union(fin)

    return result
