import cadquery as cq
import math

def sphere_with_annular_trough(
    sphere_radius: float = 10.0,
    groove_radius: float = 1.0,
    latitude: float = 0.0
) -> cq.Workplane:
    """
    Abstract: Sphere with circumferential groove.
    Detailed: Create a sphere of given radius, then subtract a toroidal cutter
    to carve a ring‐shaped trough around its surface. The cutter is a torus
    whose cross‐section is a circle of radius groove_radius, centered on the
    sphere surface at the specified latitude (degrees from equator, positive
    towards +Z).

    Parameters:
        sphere_radius -- radius of the sphere (> 0)
        groove_radius -- radius of the groove cross‐section (> 0, < sphere_radius)
        latitude      -- latitude angle in degrees from equator (0 at equator,
                         positive towards +Z)
    """
    if sphere_radius <= 0:
        raise ValueError("sphere_radius must be positive")
    if groove_radius <= 0 or groove_radius >= sphere_radius:
        raise ValueError("groove_radius must be > 0 and < sphere_radius")

    # Base sphere
    sphere = cq.Workplane("XY").sphere(sphere_radius)

    # Compute 2D center of the torus cross‐section in XZ‐plane
    lat_rad = math.radians(latitude)
    x_c = sphere_radius * math.cos(lat_rad)
    z_c = sphere_radius * math.sin(lat_rad)

    # Build torus cutter by revolving a circle in the XZ‐plane about the Z‐axis
    cutter = (
        cq.Workplane("XZ")
          .moveTo(x_c, z_c)
          .circle(groove_radius)
          .revolve(360, axisStart=(0, 0, 0), axisEnd=(0, 0, 1))
    )

    # Subtract the torus from the sphere to make the groove
    return sphere.cut(cutter)
