import cadquery as cq
import math

def spur_gear_with_diagonal_cutouts_with_chamfered_ribs(
    n_teeth: int = 16,
    module: float = 2.0,
    thickness: float = 6.0,
    slot_width: float = 0.5,
    slot_depth: float = None,
    relief_angle: float = 30.0,
    fillet_radius: float = None,
    rib_count: int = None,
    rib_width: float = None,
    rib_height: float = None,
    rib_chamfer: float = 0.2
) -> cq.Workplane:
    """
    Spur gear with diagonal relief slots and chamfered reinforcing ribs on the back face.

    Parameters:
        n_teeth (int): Number of teeth
        module (float): Gear module
        thickness (float): Axial thickness of the gear
        slot_width (float): Tangential width of each diagonal relief slot
        slot_depth (float): Radial depth of each slot (defaults to half the dedendum)
        relief_angle (float): Angle offset of the slot from the tooth mid‐line
        fillet_radius (float): Fillet radius on slot edges (defaults to half slot width)
        rib_count (int): Number of ribs on the back face (defaults to n_teeth)
        rib_width (float): Width of each rib (defaults to module)
        rib_height (float): Height of rib protrusion on back face (defaults to half thickness)
        rib_chamfer (float): Chamfer size on the vertical edges of each rib
    """
    # 1) Basic gear geometry
    pitch_dia = module * n_teeth
    r_pitch = pitch_dia / 2.0
    addendum = module
    dedendum = 1.25 * module
    r_outer = r_pitch + addendum
    r_root = r_pitch - dedendum

    # 2) Defaults for slots and fillet
    if slot_depth is None:
        slot_depth = dedendum * 0.5
    if fillet_radius is None:
        fillet_radius = slot_width * 0.5
    max_rad = min(slot_width, slot_depth) * 0.45
    rad = min(fillet_radius, max_rad) if fillet_radius and fillet_radius > 0 else 0.0

    # 3) Defaults for ribs
    if rib_count is None:
        rib_count = n_teeth
    if rib_width is None:
        rib_width = module
    if rib_height is None:
        rib_height = thickness * 0.5

    # 4) Create the blank at root diameter
    gear = cq.Workplane("XY").circle(r_root).extrude(thickness)

    # 5) Add simple trapezoidal teeth
    half_ang = math.pi / (2.0 * n_teeth)
    for i in range(n_teeth):
        theta = 2.0 * math.pi * i / n_teeth
        a1 = theta - half_ang
        a2 = theta + half_ang
        pts = [
            (r_root * math.cos(a1), r_root * math.sin(a1)),
            (r_outer * math.cos(a1), r_outer * math.sin(a1)),
            (r_outer * math.cos(a2), r_outer * math.sin(a2)),
            (r_root * math.cos(a2), r_root * math.sin(a2)),
        ]
        tooth = cq.Workplane("XY").polyline(pts).close().extrude(thickness)
        gear = gear.union(tooth)

    # 6) Subtract diagonal relief slots
    for i in range(n_teeth):
        mid_ang = 360.0 * (i + 0.5) / n_teeth
        total_ang = mid_ang + relief_angle
        ang_rad = math.radians(total_ang)
        cx = (r_root - slot_depth / 2.0) * math.cos(ang_rad)
        cy = (r_root - slot_depth / 2.0) * math.sin(ang_rad)
        cutter = (
            cq.Workplane("XY")
              .box(slot_depth, slot_width, thickness, centered=(True, True, False))
        )
        if rad > 1e-6:
            cutter = cutter.edges("|Z").fillet(rad)
        cutter = cutter.rotate((0, 0, 0), (0, 0, 1), total_ang).translate((cx, cy, 0))
        gear = gear.cut(cutter)

    # 7) Add chamfered ribs on the back (flat) face
    ribs = None
    for j in range(rib_count):
        ang = 360.0 * j / rib_count
        rib = (
            cq.Workplane("XY")
              .box(r_root, rib_width, rib_height, centered=(False, True, False))
              .translate((0, 0, -rib_height))
              .rotate((0, 0, 0), (0, 0, 1), ang)
        )
        if rib_chamfer > 0:
            rib = rib.edges("|Z").chamfer(rib_chamfer)
        ribs = rib if ribs is None else ribs.union(rib)

    if ribs is not None:
        gear = gear.union(ribs)

    return gear
