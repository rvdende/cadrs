def sphere_geo_cable_with_parametric_spike_array(
    sphere_radius: float = 10.0,
    cell_diameter: float = 1.0,
    spacing: float = 0.2,
    spike_base_radius: float = 0.1,
    spike_height: float = 0.5
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Parameter validation
    if sphere_radius <= 0:
        raise ValueError("sphere_radius must be positive")
    if cell_diameter <= 0:
        raise ValueError("cell_diameter must be positive")
    if spacing < 0:
        raise ValueError("spacing must be non-negative")
    if spike_base_radius < 0:
        raise ValueError("spike_base_radius must be non-negative")
    if spike_height < 0:
        raise ValueError("spike_height must be non-negative")

    # Create base sphere
    sphere_shape = cq.Workplane("XY").sphere(sphere_radius).val()

    # Hex grid spacing
    dx = cell_diameter + spacing
    dy = dx * math.sqrt(3.0) / 2.0
    r2 = sphere_radius * sphere_radius
    tol = 1e-6
    eps = 1e-4

    spikes = []
    row = 0
    y = -sphere_radius
    while y <= sphere_radius + tol:
        x_offset = (row % 2) * dx * 0.5
        x_limit = math.sqrt(max(r2 - y*y, 0.0))
        x = -x_limit + x_offset
        while x <= x_limit + tol:
            if x*x + y*y <= r2 + tol:
                z_cap = math.sqrt(max(r2 - x*x - y*y, 0.0))
                z_vals = (z_cap, -z_cap) if z_cap > tol else (0.0,)
                for z in z_vals:
                    nx = x / sphere_radius
                    ny = y / sphere_radius
                    nz = z / sphere_radius
                    length = spike_height * math.sqrt(max(0.0, 1.0 - nz*nz))
                    if length > tol and spike_base_radius > tol:
                        origin = (x - nx*eps, y - ny*eps, z - nz*eps)
                        xdir = (1.0, 0.0, 0.0) if abs(nz) > 1.0 - tol else (ny, -nx, 0.0)
                        pl = cq.Plane(origin=origin, normal=(nx, ny, nz), xDir=xdir)
                        # compute taper so radius shrinks to zero
                        taper_angle = -math.degrees(math.atan(spike_base_radius / length))
                        spike = (
                            cq.Workplane(pl)
                              .circle(spike_base_radius)
                              .extrude(length + eps, taper=taper_angle, combine=False)
                              .val()
                        )
                        spikes.append(spike)
            x += dx
        y += dy
        row += 1

    result_shape = sphere_shape
    if spikes:
        def fuse_list(lst):
            if len(lst) == 1:
                return lst[0]
            mid = len(lst) // 2
            return fuse_list(lst[:mid]).fuse(fuse_list(lst[mid:]))
        all_spikes = fuse_list(spikes)
        result_shape = sphere_shape.fuse(all_spikes)

    return cq.Workplane("XY").add(result_shape)
