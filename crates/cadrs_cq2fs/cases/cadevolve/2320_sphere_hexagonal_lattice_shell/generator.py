import cadquery as cq
import math

def sphere_hexagonal_lattice_shell(
    outer_radius: float = 10.0,
    shell_thickness: float = 1.0,
    cell_radius: float = 1.0,
    rings: int = 8,
    columns: int = 16,
    fillet_radius: float = 0.2
) -> cq.Workplane:
    """
    Thin spherical shell with hexagonal lattice struts on the inside,
    followed by an optional fillet on all edges.

    Parameters:
        outer_radius    (float): Outer radius of the sphere (> 0)
        shell_thickness (float): Wall thickness (0 < shell_thickness < outer_radius)
        cell_radius     (float): Hex cell circumscribed radius (> 0)
        rings           (int):   Number of latitude rings of hex cells (>= 1)
        columns         (int):   Number of hex columns per ring (>= 1)
        fillet_radius   (float): Fillet radius to apply on all resulting edges (>= 0)
    """
    # Parameter validation
    if outer_radius <= 0:
        raise ValueError(f"outer_radius must be > 0; got {outer_radius}")
    if shell_thickness <= 0 or shell_thickness >= outer_radius:
        raise ValueError(f"shell_thickness must be > 0 and < outer_radius; got {shell_thickness}")
    if cell_radius <= 0:
        raise ValueError(f"cell_radius must be > 0; got {cell_radius}")
    rings = int(rings)
    columns = int(columns)
    if rings < 1:
        raise ValueError(f"rings must be an integer >= 1; got {rings}")
    if columns < 1:
        raise ValueError(f"columns must be an integer >= 1; got {columns}")
    if fillet_radius < 0:
        raise ValueError(f"fillet_radius must be >= 0; got {fillet_radius}")

    # Compute inner radius
    inner_radius = outer_radius - shell_thickness

    # Build solid outer sphere and subtract inner to make a thin shell
    outer_sphere = cq.Workplane("XY").sphere(outer_radius)
    inner_sphere = cq.Workplane("XY").sphere(inner_radius)
    result = outer_sphere.cut(inner_sphere)

    # Add hexagonal prism struts inward from the inner surface
    for i in range(rings):
        theta = math.pi * (i + 1) / (rings + 1)
        sin_t = math.sin(theta)
        cos_t = math.cos(theta)
        # stagger every other ring
        phi_offset = math.pi / columns if (i % 2 == 0) else 0.0
        for j in range(columns):
            phi = 2.0 * math.pi * j / columns + phi_offset
            x = inner_radius * sin_t * math.cos(phi)
            y = inner_radius * sin_t * math.sin(phi)
            z = inner_radius * cos_t
            # Workplane at the point, normal pointing outward
            plane = cq.Plane(origin=(x, y, z), normal=(x, y, z))
            hex_prism = (
                cq.Workplane(plane)
                  .polygon(6, 2.0 * cell_radius)
                  .extrude(-shell_thickness)
            )
            result = result.union(hex_prism)

    # Apply fillet to all edges if requested
    if fillet_radius > 0.0:
        try:
            result = result.edges().fillet(fillet_radius)
        except Exception:
            pass

    return result
