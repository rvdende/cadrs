import cadquery as cq
import math

def sphere_with_hexagonal_lattice_shell(
    outer_radius: float = 10.0,
    shell_thickness: float = 1.0,
    cell_radius: float = 1.0,
    rings: int = 8,
    columns: int = 16
) -> cq.Workplane:
    """
    Thin spherical shell with hexagonal lattice infill.

    Abstract:
        Starting from a solid sphere, subtract an inner sphere to leave a
        thin shell of uniform thickness, then extrude hexagonal prisms
        inward from the inner surface to form a lattice.

    Parameters:
        outer_radius    (float): Outer radius of the sphere (> 0)
        shell_thickness (float): Wall thickness (0 < shell_thickness < outer_radius)
        cell_radius     (float): Radius of the circumscribed circle of each hex cell (> 0)
        rings            (int): Number of latitude rings of hex cells (>= 1)
        columns          (int): Number of hex columns per ring (>= 1)

    Returns:
        cq.Workplane: The resulting shell with internal hexagonal lattice.
    """
    # Parameter validation
    if outer_radius <= 0:
        raise ValueError("outer_radius must be positive")
    if shell_thickness <= 0 or shell_thickness >= outer_radius:
        raise ValueError("shell_thickness must be > 0 and < outer_radius")
    if cell_radius <= 0:
        raise ValueError("cell_radius must be positive")
    if not isinstance(rings, int) or rings < 1:
        raise ValueError("rings must be an integer >= 1")
    if not isinstance(columns, int) or columns < 1:
        raise ValueError("columns must be an integer >= 1")

    # Compute inner radius
    inner_radius = outer_radius - shell_thickness

    # Build the outer and inner spheres and subtract to form the shell
    outer_sphere = cq.Workplane("XY").sphere(outer_radius)
    inner_sphere = cq.Workplane("XY").sphere(inner_radius)
    shell = outer_sphere.cut(inner_sphere)

    result = shell

    # Create hexagonal lattice on the inner surface
    # We distribute 'rings' latitude rings of cells between the poles (excluding)
    for i in range(rings):
        theta = math.pi * (i + 1) / (rings + 1)
        sin_t = math.sin(theta)
        cos_t = math.cos(theta)
        # Alternate a half‐step offset every other ring for hex staggering
        offset = math.pi / columns if (i % 2) == 0 else 0.0
        for j in range(columns):
            # Azimuthal position
            phi = 2.0 * math.pi * j / columns + offset
            # Point on the inner spherical surface
            x = inner_radius * sin_t * math.cos(phi)
            y = inner_radius * sin_t * math.sin(phi)
            z = inner_radius * cos_t
            # Define a workplane at that point, normal pointing outward
            plane = cq.Plane(origin=(x, y, z), normal=(x, y, z))
            # Draw a regular hexagon (diameter = 2 * cell_radius) and extrude inward
            hex_prism = (
                cq.Workplane(plane)
                  .polygon(6, 2.0 * cell_radius)
                  .extrude(-shell_thickness)
            )
            result = result.union(hex_prism)

    return result
