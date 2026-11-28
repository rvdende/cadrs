import cadquery as cq
import math

def annular_ring_with_scalloped_thorn_fin_and_counterbore_lip(
    outer_radius: float = 20.0,
    ring_thickness: float = 4.0,
    height: float = 5.0,
    scallop_count: int = 16,
    scallop_depth: float = 1.0,
    thorn_count: int = 16,
    thorn_length: float = 3.0,
    thorn_base_radius: float = 1.0,
    thorn_tip_radius: float = 0.2,
    counterbore_diameter: float = 36.0,
    counterbore_depth: float = 1.5
) -> "cq.Workplane":
    import cadquery as cq
    import math

    # Parameter validation
    if outer_radius <= 0:
        raise ValueError("outer_radius must be positive")
    if ring_thickness <= 0 or ring_thickness >= outer_radius:
        raise ValueError("ring_thickness must be > 0 and < outer_radius")
    if height <= 0:
        raise ValueError("height must be positive")
    scallop_count = int(scallop_count)
    if scallop_count < 0:
        raise ValueError("scallop_count must be non-negative")
    if scallop_depth < 0 or scallop_depth > ring_thickness:
        raise ValueError("scallop_depth must be >= 0 and <= ring_thickness")
    thorn_count = int(thorn_count)
    if thorn_count < 0:
        raise ValueError("thorn_count must be non-negative")
    if thorn_length <= 0:
        raise ValueError("thorn_length must be positive")
    if thorn_base_radius <= 0:
        raise ValueError("thorn_base_radius must be positive")
    if thorn_tip_radius < 0:
        raise ValueError("thorn_tip_radius must be non-negative")
    if counterbore_diameter <= 0:
        raise ValueError("counterbore_diameter must be positive")
    if counterbore_depth < 0 or counterbore_depth > height:
        raise ValueError("counterbore_depth must be >= 0 and <= height")

    # compute inner radius and check counterbore
    inner_radius = outer_radius - ring_thickness
    hole_diameter = inner_radius * 2.0
    if counterbore_diameter <= hole_diameter:
        raise ValueError("counterbore_diameter must be greater than the inner hole diameter")

    # 1) Base annular ring
    result = (
        cq.Workplane("XY")
          .circle(outer_radius)
          .circle(inner_radius)
          .extrude(height)
    )

    # 2) Scallop cuts between thorn fins
    if scallop_count > 0 and scallop_depth > 1e-6:
        step = 2.0 * math.pi / scallop_count
        half_step = step / 2.0
        for i in range(scallop_count):
            theta = step * i + half_step
            x = (outer_radius - scallop_depth) * math.cos(theta)
            y = (outer_radius - scallop_depth) * math.sin(theta)
            pocket = (
                cq.Workplane("XY")
                  .transformed(offset=(x, y, 0))
                  .circle(scallop_depth)
                  .extrude(height)
            )
            result = result.cut(pocket)

    # 3) Radial thorn fins as triangular prisms
    if thorn_count > 0 and thorn_length > 1e-6:
        step = 2.0 * math.pi / thorn_count
        half_ang = step / 2.0
        for i in range(thorn_count):
            theta = step * i
            p1 = (
                outer_radius * math.cos(theta - half_ang),
                outer_radius * math.sin(theta - half_ang),
            )
            p2 = (
                outer_radius * math.cos(theta + half_ang),
                outer_radius * math.sin(theta + half_ang),
            )
            tip = (
                (outer_radius + thorn_length) * math.cos(theta),
                (outer_radius + thorn_length) * math.sin(theta),
            )
            fin = (
                cq.Workplane("XY")
                  .polyline([p1, tip, p2])
                  .close()
                  .extrude(height)
            )
            result = result.union(fin)

    # 4) Underside counterbore (recess)
    if counterbore_depth > 1e-6:
        cb = (
            cq.Workplane("XY")
              .circle(counterbore_diameter / 2.0)
              .extrude(counterbore_depth)
        )
        result = result.cut(cb)

    # 5) Return the combined solid
    return result.combine()
