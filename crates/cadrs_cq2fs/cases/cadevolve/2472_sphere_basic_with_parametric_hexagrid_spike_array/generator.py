def sphere_basic_with_parametric_hexagrid_spike_array(
    sphere_radius=10.0,
    cell_diameter=1.0,
    spacing=0.2,
    spike_base_radius=0.1,
    spike_height=0.5
):
    import cadquery as cq
    import math

    # Parameter validation
    if sphere_radius <= 0:
        raise ValueError("sphere_radius must be > 0")
    if cell_diameter <= 0:
        raise ValueError("cell_diameter must be > 0")
    if spacing < 0:
        raise ValueError("spacing must be >= 0")
    if spike_base_radius < 0:
        raise ValueError("spike_base_radius must be >= 0")
    if spike_height < 0:
        raise ValueError("spike_height must be >= 0")

    # Create base sphere
    sphere_wp = cq.Workplane("XY").sphere(sphere_radius)
    sphere_shape = sphere_wp.val()

    # Hex grid spacing
    dx = cell_diameter + spacing
    dy = dx * math.sqrt(3.0) / 2.0
    r2 = sphere_radius * sphere_radius
    tol = 1e-6
    eps = 1e-4  # small inward offset to avoid gaps

    # Collect spike shapes
    spike_shapes = []
    row = 0
    y = -sphere_radius
    while y <= sphere_radius + tol:
        x_max = math.sqrt(max(r2 - y*y, 0.0))
        x_offset = (row % 2) * dx / 2.0
        x = -x_max + x_offset
        while x <= x_max + tol:
            d2 = x*x + y*y
            if d2 <= r2 + tol:
                z_cap = math.sqrt(max(r2 - d2, 0.0))
                z_values = (z_cap, -z_cap) if z_cap > tol else (0.0,)
                for z in z_values:
                    nx = x / sphere_radius
                    ny = y / sphere_radius
                    nz = z / sphere_radius
                    length = spike_height * math.sqrt(max(0.0, 1.0 - nz*nz))
                    if length > tol and spike_base_radius > tol:
                        # define local workplane for the spike
                        xdir = (ny, -nx, 0.0)
                        if abs(nz) > 1.0 - tol:
                            xdir = (1.0, 0.0, 0.0)
                        origin = (x - nx*eps, y - ny*eps, z - nz*eps)
                        pl = cq.Plane(origin=origin, normal=(nx, ny, nz), xDir=xdir)
                        spike_wp = cq.Workplane(pl).circle(spike_base_radius).extrude(length + eps, combine=False)
                        spike_shapes.append(spike_wp.val())
            x += dx
        row += 1
        y += dy

    # Fuse spikes recursively to reduce boolean operations
    final_shape = sphere_shape
    if spike_shapes:
        def fuse_list(shapes):
            if len(shapes) == 1:
                return shapes[0]
            mid = len(shapes) // 2
            return fuse_list(shapes[:mid]).fuse(fuse_list(shapes[mid:]))
        all_spikes = fuse_list(spike_shapes)
        final_shape = sphere_shape.fuse(all_spikes)

    # Return the combined result as a Workplane
    return cq.Workplane("XY").add(final_shape)
