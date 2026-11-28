def sphere_spiral_fin_external(
    base_radius: float = 10.0,
    height: float = 25.0,
    fin_count: int = 3,
    fin_thickness: float = 1.0,
    fin_height: float = 2.0,
    turn_count: float = 1.0,
    segments_per_turn: int = 36,
    fillet_radius: float = 0.1
):
    """
    external spiral fin variant – Extrude; emboss helical fins; fillet crest.

    Parameters:
        base_radius       – radius of the cylindrical body (> 0)
        height            – axial length of the cylinder (> 0)
        fin_count         – number of parallel spiral fins (>= 1)
        fin_thickness     – thickness of each fin along the axis (> 0)
        fin_height        – radial extension of each fin (> 0)
        turn_count        – number of spiral turns (> 0)
        segments_per_turn – number of linear segments per turn (>= 1)
        fillet_radius     – fillet radius at fin edges (>= 0)

    Returns:
        A CadQuery Workplane object of a cylinder with external spiral fins.
    """
    import cadquery as cq

    # Validate parameters
    if base_radius <= 0 or height <= 0:
        raise ValueError("base_radius and height must be positive")
    if fin_count < 1 or fin_thickness <= 0 or fin_height <= 0 or turn_count <= 0 or segments_per_turn < 1:
        # no valid fins: just return a plain cylinder
        return cq.Workplane("XY").circle(base_radius).extrude(height)

    # Build the base cylinder
    base = cq.Workplane("XY").circle(base_radius).extrude(height)

    total_segments = int(turn_count * segments_per_turn)
    result = base

    # Create each spiral fin
    for i in range(fin_count):
        phase = 360.0 * i / fin_count
        fin_union = None
        for s in range(total_segments):
            t0 = s / total_segments
            t1 = (s + 1) / total_segments
            tm = (s + 0.5) / total_segments
            z0 = height * t0
            z1 = height * t1
            # cylinder has constant radius
            r0 = base_radius
            r1 = base_radius
            # define quad in XZ plane
            pts = [
                (r0,      z0),
                (r0 + fin_height, z0),
                (r1 + fin_height, z1),
                (r1,      z1),
            ]
            seg = (
                cq.Workplane("XZ")
                  .polyline(pts)
                  .close()
                  .extrude(fin_thickness)
                  .rotate((0, 0, 0), (0, 0, 1), phase + 360.0 * turn_count * tm)
            )
            fin_union = seg if fin_union is None else fin_union.union(seg)
        result = result.union(fin_union)

    # Fillet the vertical crest edges of the fins
    if fillet_radius > 0:
        try:
            result = result.edges("|Z").fillet(fillet_radius)
        except Exception:
            pass

    return result
