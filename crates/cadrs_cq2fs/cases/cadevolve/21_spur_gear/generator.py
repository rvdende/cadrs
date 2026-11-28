def spur_gear(n_teeth=16, module=2.0, thickness=6.0):
    pitch_diameter = module * n_teeth
    r_pitch = pitch_diameter / 2.0
    addendum = module
    dedendum = 1.25 * module
    r_root = r_pitch - dedendum
    r_outer = r_pitch + addendum
    circular_pitch = math.pi * module
    half_tooth = circular_pitch / 4.0
    gear_blank = cq.Workplane('XY').circle(r_pitch).extrude(thickness)
    tooth = (
        cq.Workplane('XY')
          .polyline([
              (r_root, -half_tooth),
              (r_outer, -half_tooth),
              (r_outer,  half_tooth),
              (r_root,  half_tooth),
          ])
          .close()
          .extrude(thickness)
    )
    gear = gear_blank
    for i in range(n_teeth):
        angle = 360.0 * i / n_teeth
        rotated_tooth = tooth.rotate((0, 0, 0), (0, 0, 1), angle)
        gear = gear.union(rotated_tooth)
    return gear
