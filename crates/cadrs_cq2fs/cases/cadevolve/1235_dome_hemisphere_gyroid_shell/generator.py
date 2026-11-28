def dome_hemisphere_gyroid_shell(
    radius: float = 12.0,
    shell_thickness: float = 3.0,
    cell_size: float = 10.0,
    beam_radius: float = 1.0,
    wave_steps: int = 36
):
    import math
    import cadquery as cq

    # Validate parameters
    if radius <= 0:
        raise ValueError("radius must be positive")
    if not (0 < shell_thickness < radius):
        raise ValueError("shell_thickness must be > 0 and < radius")
    if cell_size <= 0:
        raise ValueError("cell_size must be positive")
    if beam_radius <= 0:
        raise ValueError("beam_radius must be positive")
    if wave_steps < 3:
        raise ValueError("wave_steps must be >= 3")

    # Compute inner radius and maximum rod extent
    inner_r = radius - shell_thickness
    rod_max = inner_r - beam_radius

    # Build the outer hemisphere shell
    outer = (
        cq.Workplane("XY")
        .sphere(radius)
        .cut(
            cq.Workplane("XY")
            .rect(2 * radius, 2 * radius)
            .extrude(-radius)
        )
    )

    # Build the inner cavity hemisphere
    inner = (
        cq.Workplane("XY")
        .sphere(inner_r)
        .cut(
            cq.Workplane("XY")
            .rect(2 * inner_r, 2 * inner_r)
            .extrude(-inner_r)
        )
    )

    # The hollow shell
    shell = outer.cut(inner)

    rods = []
    n = int(math.floor(inner_r / cell_size))
    amp = cell_size / 4.0
    tol = 1e-6

    # helper to build and orient a cylinder segment from p0 to p1
    def make_segment(p0, p1):
        dx = p1[0] - p0[0]
        dy = p1[1] - p0[1]
        dz = p1[2] - p0[2]
        length = math.sqrt(dx * dx + dy * dy + dz * dz)
        if length <= tol:
            return None
        # build cylinder along +Z of given length
        c = cq.Workplane("XY").circle(beam_radius).extrude(length)
        # compute unit direction of segment
        ux, uy, uz = dx / length, dy / length, dz / length
        # axis to rotate around = cross(z_unit, segment_unit)
        axis_x = -uy
        axis_y = ux
        axis_z = 0.0
        axis_mag = math.hypot(axis_x, axis_y)
        if axis_mag < tol:
            # aligned with Z
            if uz < 0:
                # point downwards: rotate 180° around X
                c = c.rotate((0, 0, 0), (1, 0, 0), 180)
        else:
            # angle = acos(dot(z_unit, segment_unit))
            angle = math.degrees(math.acos(max(min(uz, 1.0), -1.0)))
            c = c.rotate((0, 0, 0), (axis_x, axis_y, axis_z), angle)
        # translate base of cylinder to p0
        c = c.translate((p0[0], p0[1], p0[2]))
        return c

    # X-direction sinusoidal beams
    for j in range(-n, n + 1):
        y0 = j * cell_size
        for k in range(-n, n + 1):
            z0 = k * cell_size
            pts = []
            for i in range(wave_steps + 1):
                t = -rod_max + 2 * rod_max * i / wave_steps
                x = t
                y = y0 + amp * math.sin(2 * math.pi * t / cell_size)
                z = z0 + amp * math.cos(2 * math.pi * t / cell_size)
                pts.append((x, y, z))
            segments = []
            for p0, p1 in zip(pts[:-1], pts[1:]):
                seg = make_segment(p0, p1)
                if seg:
                    segments.append(seg)
            if segments:
                rod = segments[0]
                for seg in segments[1:]:
                    rod = rod.union(seg)
                rods.append(rod)

    # Y-direction sinusoidal beams
    for i in range(-n, n + 1):
        x0 = i * cell_size
        for k in range(-n, n + 1):
            z0 = k * cell_size
            pts = []
            for j in range(wave_steps + 1):
                t = -rod_max + 2 * rod_max * j / wave_steps
                y = t
                x = x0 + amp * math.cos(2 * math.pi * t / cell_size)
                z = z0 + amp * math.sin(2 * math.pi * t / cell_size)
                pts.append((x, y, z))
            segments = []
            for p0, p1 in zip(pts[:-1], pts[1:]):
                seg = make_segment(p0, p1)
                if seg:
                    segments.append(seg)
            if segments:
                rod = segments[0]
                for seg in segments[1:]:
                    rod = rod.union(seg)
                rods.append(rod)

    # Z-direction sinusoidal beams
    for i in range(-n, n + 1):
        x0 = i * cell_size
        for j in range(-n, n + 1):
            y0 = j * cell_size
            pts = []
            for k in range(wave_steps + 1):
                t = -rod_max + 2 * rod_max * k / wave_steps
                z = t
                x = x0 + amp * math.sin(2 * math.pi * t / cell_size)
                y = y0 + amp * math.cos(2 * math.pi * t / cell_size)
                pts.append((x, y, z))
            segments = []
            for p0, p1 in zip(pts[:-1], pts[1:]):
                seg = make_segment(p0, p1)
                if seg:
                    segments.append(seg)
            if segments:
                rod = segments[0]
                for seg in segments[1:]:
                    rod = rod.union(seg)
                rods.append(rod)

    if not rods:
        return shell

    # Combine rods into a single lattice and trim to the inner cavity
    lattice = rods[0]
    for r in rods[1:]:
        lattice = lattice.union(r)
    lattice = lattice.intersect(inner)

    # Merge the lattice into the outer shell and return
    return shell.union(lattice)
