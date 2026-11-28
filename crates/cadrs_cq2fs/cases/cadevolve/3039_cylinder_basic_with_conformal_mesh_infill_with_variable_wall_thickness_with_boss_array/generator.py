import cadquery as cq
import math

def cylinder_basic_with_conformal_mesh_infill_with_variable_wall_thickness_with_boss_array(
    outer_radius: float = 10.0,
    min_wall_thickness: float = 1.0,
    max_wall_thickness: float = 2.0,
    variation_waves: int = 4,
    wedge_count: int = 60,
    height: float = 20.0,
    mesh_size: float = 5.0,
    mesh_depth: float = None,
    boss_count: int = 6,
    boss_radius: float = 0.5,
    boss_length: float = 5.0
) -> cq.Workplane:
    """
    Create a cylinder with sinusoidally varying wall thickness and a conformal triangular mesh infill,
    then add a ring of bosses on the outer surface for modular mounting.

    1) Build a solid cylinder (outer_radius, height).
    2) Subtract wedge‐shaped inner cutters in wedge_count slices to achieve a wall thickness
       T(phi) = min + (max-min)/2*(1+sin(variation_waves*phi)).
    3) Generate a triangular conformal mesh of beams on a constant-inner-radius surface
       defined by the maximum wall thickness, extruded inward by mesh_depth.
    4) On the outer curved surface at mid-height, place boss_count boss cylinders,
       each of radius boss_radius and length boss_length, radially outward.

    Parameters:
        outer_radius        – outer radius of the cylinder (>0)
        min_wall_thickness  – minimum wall thickness (>0, < outer_radius)
        max_wall_thickness  – maximum wall thickness (>= min, < outer_radius)
        variation_waves     – number of sinusoidal waves around circumference (>=1)
        wedge_count         – number of angular wedges to approximate the variation (>=1)
        height              – axial height of the cylinder (>0)
        mesh_size           – target edge length of triangular mesh (>0)
        mesh_depth          – extrusion depth of mesh beams; None => full minimal thickness
        boss_count          – number of bosses around circumference (>=0)
        boss_radius         – radius of each boss (>0)
        boss_length         – extrusion length of each boss (>0)

    Returns:
        cq.Workplane: resulting solid with variable wall thickness, conformal mesh infill, and boss array.
    """
    # Parameter validation
    if outer_radius <= 0:
        raise ValueError("outer_radius must be > 0")
    if height <= 0:
        raise ValueError("height must be > 0")
    if min_wall_thickness <= 0 or min_wall_thickness >= outer_radius:
        raise ValueError("min_wall_thickness must be > 0 and < outer_radius")
    if max_wall_thickness < min_wall_thickness or max_wall_thickness >= outer_radius:
        raise ValueError("max_wall_thickness must be >= min_wall_thickness and < outer_radius")
    variation_waves = int(variation_waves)
    if variation_waves < 1:
        raise ValueError("variation_waves must be integer >= 1")
    wedge_count = int(wedge_count)
    if wedge_count < 1:
        raise ValueError("wedge_count must be integer >= 1")
    if mesh_size <= 0:
        raise ValueError("mesh_size must be > 0")
    boss_count = int(boss_count)
    if boss_count < 0:
        raise ValueError("boss_count must be >= 0")
    if boss_radius <= 0:
        raise ValueError("boss_radius must be > 0")
    if boss_length <= 0:
        raise ValueError("boss_length must be > 0")

    # 1) Base solid cylinder
    result = cq.Workplane("XY").circle(outer_radius).extrude(height)

    # 2) Subtract wedge‐shaped inner cutters for variable wall thickness
    for i in range(wedge_count):
        # angular span of this wedge
        a0 = 2.0 * math.pi * i / wedge_count
        a1 = 2.0 * math.pi * (i + 1) / wedge_count
        # local wall thickness at wedge boundaries
        t0 = min_wall_thickness + 0.5 * (max_wall_thickness - min_wall_thickness) * (
            1.0 + math.sin(variation_waves * a0)
        )
        t1 = min_wall_thickness + 0.5 * (max_wall_thickness - min_wall_thickness) * (
            1.0 + math.sin(variation_waves * a1)
        )
        # corresponding inner radii
        r0 = outer_radius - t0
        r1 = outer_radius - t1
        x0, y0 = r0 * math.cos(a0), r0 * math.sin(a0)
        x1, y1 = r1 * math.cos(a1), r1 * math.sin(a1)
        # wedge cutter from center to the inner boundary of this wedge
        wedge = (
            cq.Workplane("XY")
              .polyline([(0.0, 0.0), (x0, y0), (x1, y1)])
              .close()
              .extrude(height)
        )
        result = result.cut(wedge)

    # 3) Conformal triangular mesh infill (use a safe inner radius so beams
    #    never poke through the shell at any angle)
    inner_radius_mesh = outer_radius - max_wall_thickness
    if mesh_depth is None or mesh_depth <= 0 or mesh_depth > inner_radius_mesh:
        mesh_depth = inner_radius_mesh
    total_arc = 2.0 * math.pi * inner_radius_mesh
    half_w = total_arc / 2.0
    cell_h = mesh_size * math.sqrt(3.0) / 2.0
    n_cols = max(int(math.floor(total_arc / mesh_size)), 1)
    n_rows = max(int(math.ceil(height / cell_h)), 1)

    for j in range(n_rows):
        v0 = j * cell_h
        for i in range(n_cols):
            u0 = -half_w + i * mesh_size
            # pick triangle orientation
            if (i + j) % 2 == 0:
                pts = [
                    (u0, v0),
                    (u0 + mesh_size, v0),
                    (u0 + mesh_size / 2.0, v0 + cell_h),
                ]
            else:
                pts = [
                    (u0, v0 + cell_h),
                    (u0 + mesh_size, v0 + cell_h),
                    (u0 + mesh_size / 2.0, v0),
                ]
            # skip if outside mesh domain
            if any(u < -half_w or u > half_w or v < 0 or v > height for u, v in pts):
                continue
            # centroid in unwrapped coords
            uc = sum(u for u, _ in pts) / 3.0
            vc = sum(v for _, v in pts) / 3.0
            # angular position of centroid
            phi = math.pi / 2.0 - uc / inner_radius_mesh
            nx = math.cos(phi)
            ny = math.sin(phi)
            # local workplane on inner cylindrical surface
            plane = cq.Plane(
                origin=(inner_radius_mesh * nx, inner_radius_mesh * ny, vc),
                normal=(-nx, -ny, 0.0),
                xDir=(-ny, nx, 0.0),
            )
            # 2D coords of triangle relative to centroid
            pts2d = [(u - uc, v - vc) for u, v in pts]
            beam = cq.Workplane(plane).polyline(pts2d).close().extrude(mesh_depth, combine=True)
            result = result.union(beam, clean=True)

    # 4) Boss array on outer surface
    if boss_count > 0:
        mid_z = height / 2.0
        step_ang = 2.0 * math.pi / boss_count
        for i in range(boss_count):
            ang = i * step_ang
            nx = math.cos(ang)
            ny = math.sin(ang)
            ox, oy = nx * outer_radius, ny * outer_radius
            plane = cq.Plane(
                origin=(ox, oy, mid_z),
                normal=(nx, ny, 0.0),
                xDir=(0.0, 0.0, 1.0),
            )
            boss = cq.Workplane(plane).circle(boss_radius).extrude(boss_length)
            result = result.union(boss, clean=True)

    return result

# Example usage:
# part = cylinder_basic_with_conformal_mesh_infill_with_variable_wall_thickness_with_boss_array()
# show_object(part)  # if using a display environment like CQ-Editor or Jupyter.
