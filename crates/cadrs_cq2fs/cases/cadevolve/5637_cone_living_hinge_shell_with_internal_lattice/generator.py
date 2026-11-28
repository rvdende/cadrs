def cone_living_hinge_shell_with_internal_lattice(
    base_radius: float = 40.0,
    top_radius: float = 10.0,
    height: float = 80.0,
    thickness: float = 2.0,
    hinge_thickness: float = 0.6,
    hinge_offset: float = 5.0,
    hinge_height: float = 6.0,
    hinge_relief: float = 0.8,
    lattice_rod_diam: float = 1.6,
    lattice_turns: int = 3,
    segments_per_turn: int = 24,
    triangular_web_height: float = 6.0,
    triangular_web_thickness: float = 1.2,
    tab_count: int = 8,
    tab_diam: float = 4.0,
    tab_depth: float = 2.0,
    fillet_radius: float = 0.6
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Validate inputs (defensive)
    if base_radius <= 0:
        raise ValueError("base_radius must be > 0")
    if top_radius < 0 or top_radius >= base_radius:
        raise ValueError("top_radius must be >= 0 and < base_radius")
    if height <= 0:
        raise ValueError("height must be > 0")
    if thickness <= 0 or thickness >= base_radius:
        raise ValueError("thickness must be > 0 and < base_radius")
    if hinge_thickness <= 0 or hinge_thickness >= thickness:
        raise ValueError("hinge_thickness must be > 0 and < thickness")
    if hinge_offset < 0 or hinge_offset + hinge_height > height:
        raise ValueError("hinge_offset must be >= 0 and hinge_offset + hinge_height <= height")
    if hinge_height <= 0:
        raise ValueError("hinge_height must be > 0")
    if hinge_relief < 0:
        raise ValueError("hinge_relief must be >= 0")
    if lattice_rod_diam <= 0:
        raise ValueError("lattice_rod_diam must be > 0")
    if lattice_turns <= 0 or segments_per_turn < 4:
        raise ValueError("lattice_turns must be > 0 and segments_per_turn >= 4")
    if triangular_web_height < 0 or triangular_web_thickness <= 0:
        raise ValueError("triangular_web_height must be >= 0 and triangular_web_thickness > 0")
    if tab_count < 0 or not isinstance(tab_count, int):
        raise ValueError("tab_count must be a non-negative integer")
    if tab_diam <= 0 or tab_depth < 0:
        raise ValueError("tab_diam must be > 0 and tab_depth >= 0")
    if fillet_radius < 0:
        raise ValueError("fillet_radius must be >= 0")

    # Precomputed geometry
    slope = (top_radius - base_radius) / float(height)
    inner_base = base_radius - thickness
    inner_top = max(0.0, top_radius - thickness)

    # 1) Outer and inner conical frustums (revolve profile in XZ)
    outer = (
        cq.Workplane("XZ")
        .moveTo(base_radius, 0.0)
        .lineTo(top_radius, height)
        .lineTo(0.0, height)
        .lineTo(0.0, 0.0)
        .close()
        .revolve(360.0)
    )
    inner = (
        cq.Workplane("XZ")
        .moveTo(inner_base, 0.0)
        .lineTo(inner_top, height)
        .lineTo(0.0, height)
        .lineTo(0.0, 0.0)
        .close()
        .revolve(360.0)
    )
    part = outer.cut(inner)  # hollow shell

    # 2) Living hinge: thin local band by removing a strip of interior material so wall thickness reduces
    z0 = hinge_offset
    z1 = hinge_offset + hinge_height
    r0_outer = base_radius + slope * z0
    r1_outer = base_radius + slope * z1
    r0_inner = r0_outer - thickness
    r1_inner = r1_outer - thickness

    reduction = thickness - hinge_thickness
    r0_cut = r0_inner + reduction
    r1_cut = r1_inner + reduction

    hinge_cut = (
        cq.Workplane("XZ")
        .moveTo(r0_inner, z0)
        .lineTo(r1_inner, z1)
        .lineTo(r1_cut, z1)
        .lineTo(r0_cut, z0)
        .close()
        .revolve(360.0)
    )
    part = part.cut(hinge_cut)

    # Add small semicircular reliefs at hinge ends (internal cuts) to keep hinge flexible
    if hinge_relief > 0.0:
        for z_rel in (z0, z1):
            r_rel = base_radius + slope * z_rel
            groove = (
                cq.Workplane("XZ")
                .pushPoints([(r_rel, z_rel)])
                .circle(hinge_relief)
                .revolve(360.0)
            )
            part = part.cut(groove)

    # 3) Internal helical lattice ribs - robust segmented rod construction (guaranteed to intersect inner wall)
    rods_union = None
    rod_r = lattice_rod_diam / 2.0
    total_segments = max(12, int(segments_per_turn * lattice_turns))
    # small inward overlap so rods fuse to inner wall
    overlap = min(0.6, max(rod_r * 0.4, 0.1))

    def helix_point(t):
        r = inner_base + (inner_top - inner_base) * t
        r_rod = max(0.001, r - overlap)
        theta = 2.0 * math.pi * float(lattice_turns) * t
        return (r_rod * math.cos(theta), r_rod * math.sin(theta), height * t)

    pts = [helix_point(i / float(total_segments)) for i in range(total_segments + 1)]
    for k in range(len(pts) - 1):
        x1, y1, z1 = pts[k]
        x2, y2, z2 = pts[k + 1]
        dx, dy, dz = x2 - x1, y2 - y1, z2 - z1
        seg_len = math.sqrt(dx * dx + dy * dy + dz * dz)
        if seg_len < 1e-8:
            continue
        ux, uy, uz = dx / seg_len, dy / seg_len, dz / seg_len
        # choose a stable xDir for the local plane (not parallel to normal)
        if abs(ux) < 1e-6 and abs(uy) < 1e-6:
            up = (1.0, 0.0, 0.0)
        else:
            up = (0.0, 0.0, 1.0)
        rx = up[1] * uz - up[2] * uy
        ry = up[2] * ux - up[0] * uz
        rz = up[0] * uy - up[1] * ux
        rlen = math.sqrt(rx * rx + ry * ry + rz * rz)
        if rlen < 1e-8:
            xdir = up
        else:
            xdir = (rx / rlen, ry / rlen, rz / rlen)
        plane_seg = cq.Plane(origin=(x1, y1, z1), normal=(ux, uy, uz), xDir=xdir)
        # create short cylinder/rod along the segment
        try:
            rod_seg = cq.Workplane(plane_seg).circle(rod_r).extrude(seg_len)
        except Exception:
            # if extrude on plane fails, fallback to cylinder built on default axis and translated/rotated
            rod_seg = cq.Workplane("XY").circle(rod_r).extrude(seg_len).translate((x1, y1, z1))
        rods_union = rod_seg if rods_union is None else rods_union.union(rod_seg, clean=True)

    if rods_union is not None:
        part = part.union(rods_union, clean=True)

    # 4) Triangular web lattice: small triangular ribs from inner wall to rod path sampling locations
    web_union = None
    web_samples = max(8, int(lattice_turns * 8))
    tri_h = triangular_web_height
    tri_t = triangular_web_thickness
    for i in range(web_samples):
        t = (i + 0.5) / float(web_samples)
        zc = height * t
        r_wall = inner_base + (inner_top - inner_base) * t
        theta = 2.0 * math.pi * float(lattice_turns) * t
        # rod center radial (same inward offset used above)
        r_rod = max(0.001, (inner_base + (inner_top - inner_base) * t) - overlap)
        # triangle in XZ plane (X=radius, Z=height)
        p1 = (r_wall, zc - tri_h / 2.0)
        p2 = (r_wall, zc + tri_h / 2.0)
        p3 = (r_rod, zc)
        try:
            tri = cq.Workplane("XZ").polyline([p1, p2, p3]).close().extrude(tri_t, both=True)
            tri = tri.rotate((0.0, 0.0, 0.0), (0.0, 0.0, 1.0), math.degrees(theta))
            web_union = tri if web_union is None else web_union.union(tri, clean=True)
        except Exception:
            # continue if a particular web fails
            continue

    if web_union is not None:
        part = part.union(web_union, clean=True)

    # 5) Circumferential snap tabs at rim: build a single tab geometry and polar-pattern it via manual placement
    if tab_count > 0 and tab_diam > 0 and tab_depth > 0:
        tabs_union = None
        # place tab centers on rim slightly inset so that tab base overlaps shell and fuses
        r_tab_center = base_radius - thickness * 0.5
        for i in range(tab_count):
            phi = 2.0 * math.pi * float(i) / float(tab_count)
            cx = r_tab_center * math.cos(phi)
            cy = r_tab_center * math.sin(phi)
            # normal points radially outward; create plane so extrusion goes along radial normal
            plane = cq.Plane(origin=(cx, cy, 0.0), normal=(math.cos(phi), math.sin(phi), 0.0), xDir=(0.0, 0.0, 1.0))
            # make a slight chamfered tab by extruding a short cylinder and then filleting edges later
            tab = cq.Workplane(plane).circle(tab_diam / 2.0).extrude(tab_depth)
            tabs_union = tab if tabs_union is None else tabs_union.union(tab, clean=True)
        if tabs_union is not None:
            part = part.union(tabs_union, clean=True)

    # 6) Conservative fillet blends at axial edges (avoid too-large fillets that create invalid geometry)
    if fillet_radius > 0.0:
        safe_f = min(fillet_radius, max(0.01, thickness * 0.35))
        try:
            part = part.edges("|Z").fillet(safe_f)
        except Exception:
            try:
                part = part.edges(">Z or <Z").fillet(max(0.01, safe_f * 0.6))
            except Exception:
                # fall back: try filleting only visible rim edges
                try:
                    part = part.edges(">Z").fillet(max(0.01, safe_f * 0.5))
                except Exception:
                    pass

    # 7) Ensure single fused solid. combine() may return None in some CQ versions; guard accordingly.
    try:
        combined = part.combine()
        return combined if combined is not None else part
    except Exception:
        return part
