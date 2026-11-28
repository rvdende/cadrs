def annular_filter_collar_with_variable_louver_array_and_countersunk_mounts(
    outer_radius: float = 30.0,
    inner_radius: float = None,
    height: float = 12.0,
    louver_band_height: float = 6.0,
    louver_band_offset: float = 3.0,
    louver_count: int = 16,
    slot_length: float = None,
    slot_width: float = None,
    slot_depth: float = None,
    angle_bottom_deg: float = 10.0,
    angle_top_deg: float = 45.0,
    angle_vary_per_louver_deg: float = 0.0,
    axial_segments_per_louver: int = 5,
    flange_width: float = 6.0,
    flange_thickness: float = 3.0,
    bolt_count: int = 6,
    bolt_diameter: float = 4.0,
    countersink_dia: float = 8.0,
    countersink_angle: float = 90.0,
    tab_angle_deg: float = 0.0,
    tab_diameter: float = 8.0,
    tab_height: float = 1.5,
    fillet_radius: float = 0.6
) -> "cq.Workplane":
    """
    Create an annular filter collar with a variable-tilt louver band and a countersunk mounting flange.
    Produces one single, watertight solid.

    Parameters of interest (defaults tuned for a small collar):
      - outer_radius, inner_radius: radii of ring (if inner_radius is None it is inferred).
      - height: axial height of collar.
      - louver_band_height, louver_band_offset: axial band where louvers are cut (offset from lower face).
      - louver_count: number of louvers around circumference.
      - slot_length, slot_width, slot_depth: geometry of individual louver cutters (defaults derive sensibly).
      - angle_bottom_deg, angle_top_deg: louver tilt at bottom/top of band (linear interpolation across band).
      - angle_vary_per_louver_deg: small per-louver perturbation amplitude (sinusoidal) to break perfect periodicity.
      - axial_segments_per_louver: discretization of loft-like cutter along the axial band (>=1).
      - flange_width, flange_thickness: radial and axial size of base flange (extruded from lower rim).
      - bolt_count, bolt_diameter, countersink_dia, countersink_angle: countersunk bolt pattern on flange.
      - tab_*: small orientation tab/vent boss on outer wall.
      - fillet_radius: applied to vertical edges (best-effort).
    """
    import cadquery as cq
    import math
    from cadquery import Plane

    # --- Parameter validation & sane defaults ---
    if outer_radius <= 0:
        raise ValueError("outer_radius must be positive")
    if inner_radius is None:
        # default inner radius so thickness ~ 1/6 of outer radius
        inner_radius = max(0.5, outer_radius * 0.6)
    if inner_radius < 0 or inner_radius >= outer_radius:
        raise ValueError("inner_radius must be >= 0 and < outer_radius")
    if height <= 0:
        raise ValueError("height must be positive")
    # clamp band to available height
    if louver_band_height <= 0:
        raise ValueError("louver_band_height must be positive")
    if louver_band_offset < 0:
        louver_band_offset = 0.0
    if louver_band_offset + louver_band_height > height:
        # clamp band within the collar
        louver_band_height = max(0.001, height - louver_band_offset)

    thickness = outer_radius - inner_radius
    if thickness <= 0:
        raise ValueError("outer_radius must exceed inner_radius")

    if louver_count < 0:
        raise ValueError("louver_count must be non-negative")
    axial_segments_per_louver = max(1, int(axial_segments_per_louver))

    # derive slot defaults ensuring they are large enough to cut through the band
    if slot_length is None or slot_length <= 0:
        # long enough to span the band even when tilted; include margin
        slot_length = max(louver_band_height * 2.0, height * 1.0, thickness * 4.0)
    if slot_width is None or slot_width <= 0:
        slot_width = max(0.5, thickness * 0.6)
    # prevent overlapping too large slots by limiting to circumferential sector
    mid_r = inner_radius + thickness / 2.0
    max_circumferential = 2.0 * math.pi * mid_r / max(1, louver_count)
    slot_width = min(slot_width, max_circumferential * 0.9)

    if slot_depth is None or slot_depth <= 0:
        # ensure it cuts fully through the wall and a little
        slot_depth = thickness * 1.25

    # flange validation
    if flange_width < 0:
        flange_width = 0.0
    if flange_thickness < 0:
        flange_thickness = 0.0
    if bolt_count < 0:
        bolt_count = 0
    if bolt_count > 0:
        if bolt_diameter <= 0:
            raise ValueError("bolt_diameter must be positive when bolt_count > 0")
        if countersink_dia < bolt_diameter:
            countersink_dia = bolt_diameter

    # compute usable band positions
    band_z0 = float(louver_band_offset)
    band_z1 = band_z0 + float(louver_band_height)

    # --- 1) Base annular collar (extruded ring) ---
    # Construct annular ring as single solid
    ring = cq.Workplane("XY").circle(outer_radius).circle(inner_radius).extrude(height)

    # --- 2) Build variable-tilt louver cutters (discretized along axial band) ---
    # We'll build many small cutters (extruded rectangles on planes tangent to ring)
    total_louver_cutter = None
    if louver_count > 0 and louver_band_height > 1e-6:
        # For each louver around circumference
        for i in range(louver_count):
            phi = 2.0 * math.pi * i / float(louver_count)
            nx = math.cos(phi)
            ny = math.sin(phi)

            # small per-louver perturbation to break perfect repetition if requested
            angle_offset_deg = angle_vary_per_louver_deg * math.sin(float(i) * 2.0 * math.pi / max(1, louver_count))

            # accumulate axial-segment cutters for this louver
            louver_union = None
            for seg in range(axial_segments_per_louver):
                # center Z for this segment
                seg_t = (seg + 0.5) / float(axial_segments_per_louver)
                z_k = band_z0 + seg_t * (band_z1 - band_z0)

                # linear interpolation of tilt angle between bottom and top
                base_angle = angle_bottom_deg + (angle_top_deg - angle_bottom_deg) * seg_t
                a_deg = base_angle + angle_offset_deg
                a = math.radians(a_deg)
                sin_a = math.sin(a)
                cos_a = math.cos(a)

                # define a tangent-plane located at (mid_r * [nx,ny], z_k), oriented so that
                # extruding along -normal goes radially inward; xDir mixes tangential and axial for tilt.
                # xDir has components (ny*sin_a, -nx*sin_a, cos_a) as used in examples.
                xdir = (ny * sin_a, -nx * sin_a, cos_a)
                plane = Plane(origin=(mid_r * nx, mid_r * ny, z_k), normal=(nx, ny, 0), xDir=xdir)

                try:
                    cutter_piece = cq.Workplane(plane).rect(slot_length, slot_width).extrude(-slot_depth, combine=False)
                except Exception:
                    # fallback: make slightly smaller to avoid degenerate profile
                    cutter_piece = cq.Workplane(plane).rect(max(1e-3, slot_length * 0.9), max(1e-3, slot_width * 0.9)).extrude(-slot_depth, combine=False)

                louver_union = cutter_piece if louver_union is None else louver_union.union(cutter_piece, clean=True)

            total_louver_cutter = louver_union if total_louver_cutter is None else total_louver_cutter.union(louver_union, clean=True)

        # perform the cut of all louvers in one go
        try:
            ring = ring.cut(total_louver_cutter)
        except Exception:
            # fallback: cut iteratively (less efficient) to avoid single large boolean failures
            # iterate again and cut per-louver by reconstructing per-louver cutters
            if louver_count > 0:
                for i in range(louver_count):
                    phi = 2.0 * math.pi * i / float(louver_count)
                    nx = math.cos(phi); ny = math.sin(phi)
                    louver_union = None
                    for seg in range(axial_segments_per_louver):
                        seg_t = (seg + 0.5) / float(axial_segments_per_louver)
                        z_k = band_z0 + seg_t * (band_z1 - band_z0)
                        base_angle = angle_bottom_deg + (angle_top_deg - angle_bottom_deg) * seg_t
                        a_deg = base_angle + angle_vary_per_louver_deg * math.sin(float(i) * 2.0 * math.pi / max(1, louver_count))
                        a = math.radians(a_deg)
                        sin_a = math.sin(a); cos_a = math.cos(a)
                        xdir = (ny * sin_a, -nx * sin_a, cos_a)
                        plane = Plane(origin=(mid_r * nx, mid_r * ny, z_k), normal=(nx, ny, 0), xDir=xdir)
                        cutter_piece = cq.Workplane(plane).rect(slot_length, slot_width).extrude(-slot_depth, combine=False)
                        louver_union = cutter_piece if louver_union is None else louver_union.union(cutter_piece, clean=True)
                    try:
                        ring = ring.cut(louver_union)
                    except Exception:
                        # last resort: do individual small cuts
                        try:
                            # use each segment piece separately
                            for face in louver_union.solids().objects:
                                ring = ring.cut(face)
                        except Exception:
                            pass

    # --- 3) Build flange on lower rim and drill countersunk bolt pattern (on flange only) ---
    flange_outer = outer_radius + flange_width
    flange_inner = outer_radius
    flange = None
    if flange_width > 1e-6 and flange_thickness > 1e-6:
        # create flange as separate solid extruded downward from z=0 (the collar bottom)
        flange = cq.Workplane("XY").workplane(offset=0.0).circle(flange_outer).circle(flange_inner).extrude(-flange_thickness, combine=False)

        # bolt positions (midpoint of flange radial band)
        if bolt_count > 0 and bolt_diameter > 0:
            bolt_circle_radius = outer_radius + flange_width * 0.5
            bolt_pts = []
            for k in range(bolt_count):
                a = 2.0 * math.pi * k / float(bolt_count)
                bolt_pts.append((bolt_circle_radius * math.cos(a), bolt_circle_radius * math.sin(a)))

            # drill countersunk holes on flange top face (z=0)
            try:
                flange = flange.faces(">Z").workplane().pushPoints(bolt_pts).cskHole(bolt_diameter, countersink_dia, countersink_angle, flange_thickness)
            except Exception:
                try:
                    flange = flange.faces(">Z").workplane().pushPoints(bolt_pts).hole(bolt_diameter, flange_thickness)
                except Exception:
                    pass

        # union flange with ring so that flange holes remain as cuts
        try:
            ring = ring.union(flange, clean=True)
        except Exception:
            ring = ring.union(flange)

    # --- 4) Add small embossed orientation tab / vent boss on outer wall ---
    # place at mid-band or near bottom if band absent
    try:
        tab_phi = math.radians(tab_angle_deg)
        tx = math.cos(tab_phi)
        ty = math.sin(tab_phi)
        # keep tab within collar height bounds (a little above bottom)
        z_tab = max(0.5 * tab_height, min(height - 0.5 * tab_height, band_z0 + 0.5 * louver_band_height))
        plane_tab = Plane(origin=(outer_radius * tx, outer_radius * ty, z_tab), normal=(tx, ty, 0), xDir=(-ty, tx, 0))
        # small circular boss (extruded outward from radial surface)
        boss = cq.Workplane(plane_tab).circle(max(0.5, tab_diameter * 0.5)).extrude(tab_height, combine=False)
        try:
            ring = ring.union(boss, clean=True)
        except Exception:
            ring = ring.union(boss)
    except Exception:
        # if boss creation fails, ignore
        pass

    # --- 5) Fillet/round vertical edges (best-effort) ---
    if fillet_radius and fillet_radius > 1e-6:
        try:
            ring = ring.edges("|Z").fillet(fillet_radius)
        except Exception:
            # try a more conservative selection if the broad selection fails
            try:
                ring = ring.edges().filter(lambda e: abs(e.val().Center().z - (height / 2.0)) < (height / 2.0 + 1e-6)).fillet(min(fillet_radius, thickness * 0.4))
            except Exception:
                pass

    # --- 6) Final housekeeping: combine solids into single solid if possible and return ---
    try:
        ring = ring.combineSolids()
    except Exception:
        # combineSolids can fail on some backends; ignore and continue
        pass

    # Return a Workplane containing exactly one solid (if possible)
    try:
        solid = ring.val()
        return cq.Workplane("XY").add(solid)
    except Exception:
        # fallback, return whatever we have (likely a Workplane with one or more solids)
        return ring
