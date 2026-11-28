import cadquery as cq
import math

def star_prism_with_boss(R=15.0, H=10.0, boss_d=5.0, boss_h=3.0):
    """
    Abstract: Star prism with a central boss.
    Detailed: Creates a five-pointed star prism of outer radius R and height H,
    then extrudes a centered cylindrical boss of diameter boss_d and height boss_h
    from the top face.
    Parameters:
        R       -- outer radius of the star points
        H       -- height of the star prism
        boss_d  -- diameter of the central boss
        boss_h  -- height of the central boss
    """
    # Build star profile
    pts = []
    for i in range(10):
        ang = math.pi/5 * i
        r = R if i % 2 == 0 else R * 0.382
        pts.append((r * math.cos(ang), r * math.sin(ang)))
    # Extrude star prism
    result = cq.Workplane("XY").polyline(pts).close().extrude(H)
    # Add boss on top face
    result = result.faces(">Z").workplane().circle(boss_d/2.0).extrude(boss_h)
    return result

# Example usage:
# model = star_prism_with_boss(R=20, H=8, boss_d=6, boss_h=4)
# show_object(model)  # in a CadQuery viewer environment
