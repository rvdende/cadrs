import cadquery as cq

def hollow_cylinder_shell(inner_radius: float = 10.0,
                          wall_thickness: float = 2.0,
                          height: float = 20.0) -> cq.Workplane:
    """
    Abstract: Thin cylindrical shell.
    Detailed: A pipe‐like shell with uniform wall thickness produced by extruding an annular profile.

    Parameters:
        inner_radius   -- inner radius of the shell
        wall_thickness -- thickness of the shell wall
        height         -- height (length) of the shell along the Z axis
    """
    outer_radius = inner_radius + wall_thickness
    return (
        cq.Workplane("XY")
          .circle(outer_radius)   # outer diameter of shell
          .circle(inner_radius)   # inner diameter of shell (creates the hollow)
          .extrude(height)        # extrude to form the cylindrical shell
    )
