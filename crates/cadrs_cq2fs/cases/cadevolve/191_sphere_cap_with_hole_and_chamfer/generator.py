def sphere_cap_with_hole_and_chamfer(R=12.0, h=4.0, hole_diameter=2.0, chamfer_size=0.5):
    """
    Cap with penetrating hole and bevel.

    Creates a spherical cap of radius R and height h, drills
    a through‐hole of diameter hole_diameter in the flat face,
    then chamfers the hole perimeter and outer rim on that face
    by chamfer_size.
    """
    import cadquery as cq

    # Create full sphere and slice it to form a cap
    sphere = cq.Workplane("XY").sphere(R)
    # A big box/cutter at z = R-h cutting away the bottom
    cutter = (
        cq.Workplane("XY")
        .workplane(offset=R - h)
        .rect(2 * R, 2 * R)
        .extrude(h)
    )
    cap = sphere.intersect(cutter)

    # Drill hole through the flat face
    cap = cap.faces("<Z").workplane().hole(hole_diameter)

    # Chamfer all edges on the flat face (outer rim + hole rim)
    cap = cap.faces("<Z").edges().chamfer(chamfer_size)

    return cap
