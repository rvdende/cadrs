import cadquery as cq
import math

def hex_prism_with_countersunk_mounting_holes_and_gussets(
    outer_radius: float = 20.0,
    prism_height: float = 10.0,
    flange_radius: float = 28.0,
    flange_thickness: float = 3.0,
    gusset_width: float = 2.0,
    gusset_count: int = 6,
    hole_count: int = 6,
    hole_circle_radius: float = 24.0,
    hole_diameter: float = 5.0,
    countersink_diameter: float = 10.0,
    countersink_angle: float = 82.0,
    fillet_radius: float = 0.5
) -> cq.Workplane:
    """
    Hexagonal prism with a larger hexagonal flange on top, reinforced by triangular gussets,
    and equally spaced countersunk mounting holes through the flange and prism.
    """
    # Parameter validation
    outer_radius = float(outer_radius)
    prism_height = float(prism_height)
    flange_radius = float(flange_radius)
    flange_thickness = float(flange_thickness)
    gusset_width = float(gusset_width)
    gusset_count = int(gusset_count)
    hole_count = int(hole_count)
    hole_circle_radius = float(hole_circle_radius)
    hole_diameter = float(hole_diameter)
    countersink_diameter = float(countersink_diameter)
    countersink_angle = float(countersink_angle)
    fillet_radius = float(fillet_radius)

    if outer_radius <= 0 or prism_height <= 0 or flange_radius <= 0 or flange_thickness <= 0:
        raise ValueError("All radii and heights must be positive")
    if flange_radius < outer_radius:
        raise ValueError("flange_radius must be >= outer_radius")
    if gusset_width <= 0:
        raise ValueError("gusset_width must be positive")
    if gusset_count < 1 or hole_count < 1:
        raise ValueError("gusset_count and hole_count must be at least 1")
    if hole_diameter <= 0 or countersink_diameter < hole_diameter:
        raise ValueError("hole_diameter must be positive and countersink_diameter >= hole_diameter")
    if not (0.0 < countersink_angle < 180.0):
        raise ValueError("countersink_angle must be between 0 and 180 degrees")
    # 1) Main hexagonal prism body
    result = cq.Workplane("XY").polygon(6, 2.0 * outer_radius).extrude(prism_height)
    # 2) Hexagonal flange on top
    result = result.faces(">Z").workplane().polygon(6, 2.0 * flange_radius).extrude(flange_thickness)
    # 3) Triangular gussets under the flange
    tiny = 1e-6
    angle_step = 360.0 / gusset_count
    for i in range(gusset_count):
        angle = angle_step * i
        gusset = (
            cq.Workplane("XZ", origin=(0, -gusset_width / 2.0, prism_height - tiny))
              .polyline([
                  (outer_radius - tiny, 0.0),
                  (flange_radius + tiny, 0.0),
                  (flange_radius + tiny, flange_thickness + tiny)
              ])
              .close()
              .extrude(gusset_width)
              .rotate((0, 0, 0), (0, 0, 1), angle)
        )
        if fillet_radius > 0:
            gusset = gusset.edges("|Z").fillet(fillet_radius)
        result = result.union(gusset)
    # 4) Countersunk holes through flange and body
    points = []
    hole_step = 360.0 / hole_count
    for i in range(hole_count):
        ang = math.radians(hole_step * i)
        x = hole_circle_radius * math.cos(ang)
        y = hole_circle_radius * math.sin(ang)
        points.append((x, y))
    result = (
        result.faces(">Z")
              .workplane()
              .pushPoints(points)
              .cskHole(hole_diameter, countersink_diameter, countersink_angle, depth=prism_height + flange_thickness)
    )
    return result
