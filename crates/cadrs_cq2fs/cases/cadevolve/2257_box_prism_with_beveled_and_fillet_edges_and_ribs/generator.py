def box_prism_with_beveled_and_fillet_edges_and_ribs(
    length: float = 100.0,
    width: float = 60.0,
    height: float = 40.0,
    chamfer: float = 2.0,
    rib_height: float = 5.0,
    rib_thickness: float = 2.0,
    rib_spacing: float = 15.0,
    fillet_radius: float = 1.0
) -> cq.Workplane:
    # 1) Base box with optional chamfer on all edges
    result = cq.Workplane("XY").box(length, width, height)
    if chamfer > 0:
        result = result.edges().chamfer(chamfer)

    # 2) Add stiffening ribs on all faces, filleting each rib before union
    if rib_height > 0 and rib_thickness > 0 and rib_spacing > 0:
        # clamp effective fillet radius for ribs to avoid too-large radius errors
        tol = 1e-6
        max_r = min(rib_height, rib_thickness) / 2 - tol
        eff_fr = fillet_radius if fillet_radius > 0 else 0.0
        if eff_fr > max_r:
            eff_fr = max_r if max_r > 0 else 0.0

        # ribs on +X and -X faces
        n_y = max(1, int(width // rib_spacing))
        sp_y = width / (n_y + 1)
        for i in range(n_y):
            y_i = -width / 2 + (i + 1) * sp_y
            for sign in (1, -1):
                rib = cq.Workplane("XY").box(rib_height, rib_thickness, height)
                rib = rib.translate((sign * (length / 2 + rib_height / 2), y_i, 0))
                if eff_fr > 0:
                    try:
                        rib = rib.edges("|Z").fillet(eff_fr)
                    except Exception:
                        pass
                result = result.union(rib)

        # ribs on +Y and -Y faces
        n_x = max(1, int(length // rib_spacing))
        sp_x = length / (n_x + 1)
        for j in range(n_x):
            x_j = -length / 2 + (j + 1) * sp_x
            for sign in (1, -1):
                rib = cq.Workplane("XY").box(rib_thickness, rib_height, height)
                rib = rib.translate((x_j, sign * (width / 2 + rib_height / 2), 0))
                if eff_fr > 0:
                    try:
                        rib = rib.edges("|Z").fillet(eff_fr)
                    except Exception:
                        pass
                result = result.union(rib)

    return result
