def cylinder_shell_with_internal_spiral_lattice(
    outer_radius: float = 20.0,
    wall_thickness: float = 2.0,
    height: float = 30.0,
    lattice_rib_thickness: float = 1.0,
    helix_turns: int = 3,
    helix_steps: int = 60
) -> "cq.Workplane":
    """
    Cylindrical shell reinforced by an internal continuous spiral lattice of ribs.
    A hollow cylinder of uniform wall thickness. On the interior, two interleaved
    helical ribs wrap from one end to the other at opposing pitch, forming a spiral
    lattice network. The ribs intersect and fuse at crossings, ensuring a single
    watertight solid.
    """
    import cadquery as cq
    import math

    # Parameter validation
    if outer_radius <= 0:
        raise ValueError("outer_radius must be positive")
    if wall_thickness <= 0 or wall_thickness >= outer_radius:
        raise ValueError("wall_thickness must be > 0 and < outer_radius")
    if height <= 0:
        raise ValueError("height must be positive")
    inner_radius = outer_radius - wall_thickness
    if lattice_rib_thickness <= 0 or lattice_rib_thickness > inner_radius:
        raise ValueError("lattice_rib_thickness must be > 0 and <= inner radius")
    if helix_turns <= 0:
        raise ValueError("helix_turns must be positive")
    if helix_steps < 2:
        raise ValueError("helix_steps must be >= 2")

    # Path radius for center of ribs
    r_path = inner_radius - lattice_rib_thickness / 2.0

    # 1) Build the hollow cylinder shell (open ends)
    shell = (
        cq.Workplane("XY")
          .circle(outer_radius)
          .extrude(height)
          .faces("|Z")            # select top and bottom
          .shell(-wall_thickness) # hollow inward
    )

    # 2) Build two interleaved helices of rods with opposing pitch
    rods = None
    for sign, start_angle in [(1.0, 0.0), (-1.0, math.pi)]:
        # sample points along helix
        pts = []
        for j in range(helix_steps + 1):
            t = j / helix_steps
            theta = sign * 2.0 * math.pi * helix_turns * t + start_angle
            x = r_path * math.cos(theta)
            y = r_path * math.sin(theta)
            z = height * t
            pts.append((x, y, z))
        # approximate each small segment with a cylinder
        for k in range(len(pts) - 1):
            x1, y1, z1 = pts[k]
            x2, y2, z2 = pts[k + 1]
            dx, dy, dz = x2 - x1, y2 - y1, z2 - z1
            seg_len = math.sqrt(dx*dx + dy*dy + dz*dz)
            if seg_len < 1e-6:
                continue
            ux, uy, uz = dx/seg_len, dy/seg_len, dz/seg_len
            # pick an up vector not parallel to helix direction
            if abs(ux) < 1e-6 and abs(uy) < 1e-6:
                up = (1.0, 0.0, 0.0)
            else:
                up = (0.0, 0.0, 1.0)
            # compute perpendicular xDir = up × helix_dir
            rx = up[1]*uz - up[2]*uy
            ry = up[2]*ux - up[0]*uz
            rz = up[0]*uy - up[1]*ux
            rlen = math.sqrt(rx*rx + ry*ry + rz*rz)
            if rlen > 1e-6:
                xdir = (rx/rlen, ry/rlen, rz/rlen)
            else:
                xdir = up
            helix_plane = cq.Plane(origin=(x1, y1, z1), normal=(ux, uy, uz), xDir=xdir)
            rod = (
                cq.Workplane(helix_plane)
                  .circle(lattice_rib_thickness/2.0)
                  .extrude(seg_len)
            )
            rods = rod if rods is None else rods.union(rod, clean=True)

    # 3) Union shell with lattice rods
    result = shell.union(rods, clean=True) if rods is not None else shell
    return result
