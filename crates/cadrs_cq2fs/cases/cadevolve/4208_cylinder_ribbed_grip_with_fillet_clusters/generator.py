def cylinder_ribbed_grip_with_fillet_clusters(
    radius: float = 10.0,
    height: float = 20.0,
    cluster_cols: int = 8,
    cluster_rows: int = 4,
    ribs_per_cluster: int = 3,
    cluster_angle: float = 15.0,
    rib_axial_length: float = 5.0,
    rib_circumferential_width: float = 1.0,
    rib_radial_thickness: float = 0.5,
    fillet_radius: float = 0.2
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Validate parameters
    if radius <= 0 or height <= 0:
        raise ValueError("radius and height must be positive")
    if cluster_cols < 1 or cluster_rows < 1 or ribs_per_cluster < 1:
        raise ValueError("cluster_cols, cluster_rows, ribs_per_cluster must be >= 1")
    if cluster_angle < 0 or cluster_angle > 360:
        raise ValueError("cluster_angle must be between 0 and 360")
    if rib_axial_length <= 0 or rib_axial_length > height:
        raise ValueError("rib_axial_length must be >0 and <= height")
    if rib_circumferential_width < 0 or rib_radial_thickness < 0:
        raise ValueError("rib_circumferential_width and rib_radial_thickness must be >= 0")
    if fillet_radius < 0:
        raise ValueError("fillet_radius must be >= 0")

    # 1) Base cylinder
    base = (
        cq.Workplane("XY")
          .circle(radius)
          .extrude(height)
          .translate((0, 0, -height/2.0))
    )

    # 2) Axial (Z) positions for cluster rows
    if cluster_rows == 1 or height == rib_axial_length:
        z_positions = [0.0]
    else:
        span_z = height - rib_axial_length
        step_z = span_z / (cluster_rows - 1)
        start_z = -height/2.0 + rib_axial_length/2.0
        z_positions = [start_z + i * step_z for i in range(cluster_rows)]

    # 3) Build each rib as a separate solid
    rib_shapes = []
    for z in z_positions:
        for j in range(cluster_cols):
            center_angle = j * (360.0 / cluster_cols)
            if ribs_per_cluster == 1:
                offsets = [0.0]
            else:
                step_a = cluster_angle / (ribs_per_cluster - 1)
                offsets = [-cluster_angle/2.0 + k * step_a for k in range(ribs_per_cluster)]
            for off in offsets:
                ang = center_angle + off
                rad_ang = math.radians(ang)
                px = radius * math.cos(rad_ang)
                py = radius * math.sin(rad_ang)
                plane = cq.Plane(
                    origin=(px, py, z),
                    normal=(math.cos(rad_ang), math.sin(rad_ang), 0),
                    xDir=(0, 0, 1)
                )
                rib = (
                    cq.Workplane(plane)
                      .rect(rib_axial_length, rib_circumferential_width)
                      .extrude(rib_radial_thickness)
                )
                rib_shapes.append(rib.val())

    # 4) Union all ribs with the base sequentially
    model = base
    for rib in rib_shapes:
        model = model.union(rib)

    # 5) Fillet all vertical rib edges
    if fillet_radius > 0:
        model = model.edges("|Z").fillet(fillet_radius)

    return model
