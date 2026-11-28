import cadquery as cq
import math

def ring_gear_with_vented_internal_spokes(
    n_teeth: int = 32,
    module: float = 2.0,
    thickness: float = 6.0,
    hub_radius: float = None,
    hub_thickness: float = None,
    spoke_count: int = None,
    spoke_width: float = None,
    vent_slot_width: float = None,
    vent_slot_length_ratio: float = 0.5
) -> cq.Workplane:
    """
    Create a ring gear with vented internal spokes.

    Parameters:
      n_teeth                – number of teeth on the gear
      module                 – gear module (controls tooth size)
      thickness              – axial thickness of the gear
      hub_radius             – radius of the central hub; defaults to 0.3 * pitch radius
      hub_thickness          – axial height of the hub; defaults to full gear thickness
      spoke_count            – number of radial spokes; defaults to number of teeth
      spoke_width            – width of each spoke at the hub; defaults to module
      vent_slot_width        – width of vent slot cut in each spoke; defaults to 60% of spoke width
      vent_slot_length_ratio – length of vent slot as fraction of span from hub to root
    """
    # 1) Calculate gear radii
    pitch_dia = module * n_teeth
    r_pitch = pitch_dia / 2.0
    addendum = module
    dedendum = 1.25 * module
    r_outer = r_pitch + addendum
    r_root = r_pitch - dedendum

    # 2) Defaults
    if hub_radius is None:
        hub_radius = 0.3 * r_pitch
    if hub_thickness is None:
        hub_thickness = thickness
    if spoke_count is None:
        spoke_count = n_teeth
    if spoke_width is None:
        spoke_width = module
    if vent_slot_width is None:
        vent_slot_width = spoke_width * 0.6

    span = r_root - hub_radius
    if span <= 0:
        raise ValueError("hub_radius must be less than root radius")
    vent_slot_length = vent_slot_length_ratio * span

    # 3) Make the blank at the root circle
    gear = cq.Workplane("XY").circle(r_root).extrude(thickness)

    # 4) Add rectangular (approximate) teeth
    circular_pitch = math.pi * module
    half_tooth = circular_pitch / 4.0
    tooth = (
        cq.Workplane("XY")
          .polyline([
              (r_root, -half_tooth),
              (r_outer, -half_tooth),
              (r_outer,  half_tooth),
              (r_root,   half_tooth),
          ]).close()
          .extrude(thickness)
    )
    for i in range(n_teeth):
        angle = 360.0 * i / n_teeth
        gear = gear.union(tooth.rotate((0, 0, 0), (0, 0, 1), angle))

    # 5) Remove the central region for hub mounting
    gear = gear.cut(cq.Workplane("XY").circle(hub_radius).extrude(thickness))

    # 6) Add radial spokes
    if spoke_count > 0 and spoke_width > 0:
        half_sw = spoke_width / 2.0
        profile = [
            (hub_radius,      -half_sw),
            (r_root,          -half_sw),
            (r_root,           half_sw),
            (hub_radius,       half_sw),
        ]
        spoke = cq.Workplane("XY").polyline(profile).close().extrude(thickness)
        for i in range(spoke_count):
            angle = 360.0 * i / spoke_count
            gear = gear.union(spoke.rotate((0,0,0),(0,0,1), angle))

    # 7) Add central hub boss
    hub = cq.Workplane("XY").circle(hub_radius).extrude(hub_thickness)
    gear = gear.union(hub)

    # 8) Cut vent slots in each spoke
    mid_rad = hub_radius + 0.5 * span
    angle_step = 360.0 / spoke_count
    for i in range(spoke_count):
        angle = i * angle_step
        gear = (
            gear
            .faces(">Z")
            .workplane()
            .transformed(rotate=(0, 0, angle))
            .center(mid_rad, 0)
            .rect(vent_slot_length, vent_slot_width)
            .cutThruAll()
        )

    return gear
