def plate_perforated_wave_pattern(
    length: float = 100.0,
    width: float = 60.0,
    thickness: float = 5.0,
    hole_diameter: float = 5.0,
    hole_count: int = 20,
    amplitude: float = 10.0,
    wavelength: float = 40.0,
    guslet_width: float = 5.0,
    guslet_height: float = 3.0
) -> "cq.Workplane":
    """
    Plate with perforated wave hole pattern – Derived from
    perforated_plate_with_wave_hole_pattern and
    plate_with_wave_trim_and_integral_gussets, cuts wave-shaped holes
    and adds triangular guslets at each hole location on both long edges,
    all boolean‐merged.

    Parameters:
        length        -- plate X dimension
        width         -- plate Y dimension
        thickness     -- plate thickness in Z
        hole_diameter -- diameter of each circular hole
        hole_count    -- number of holes sampled along the sine wave
        amplitude     -- amplitude of the sine wave (peak deviation in Y)
        wavelength    -- wavelength of the sine wave (in X)
        guslet_width  -- how far the guslet extends outward from the plate edge
        guslet_height -- height of the guslet above/below the plate surface
    """
    import cadquery as cq
    import math

    # Convert & validate parameters
    length = float(length)
    width = float(width)
    thickness = float(thickness)
    hole_diameter = float(hole_diameter)
    hole_count = int(hole_count)
    amplitude = float(amplitude)
    wavelength = float(wavelength)
    guslet_width = float(guslet_width)
    guslet_height = float(guslet_height)

    if length <= 0 or width <= 0 or thickness <= 0:
        raise ValueError("length, width, and thickness must be positive")
    if hole_diameter <= 0:
        raise ValueError("hole_diameter must be positive")
    if hole_count < 1:
        raise ValueError("hole_count must be at least 1")
    if amplitude < 0:
        raise ValueError("amplitude must be non-negative")
    if wavelength <= 0:
        raise ValueError("wavelength must be positive")
    if guslet_width < 0 or guslet_width > width:
        raise ValueError("guslet_width must be >= 0 and <= plate width")
    if guslet_height < 0:
        raise ValueError("guslet_height must be non-negative")

    half_x = length / 2.0
    half_y = width / 2.0
    half_z = thickness / 2.0
    r = hole_diameter / 2.0

    # 1) Create base plate
    plate = cq.Workplane("XY").box(length, width, thickness)

    # 2) Compute hole centers along a sine wave
    if hole_count > 1:
        x_start = -half_x + r
        x_end = half_x - r
        dx = (x_end - x_start) / (hole_count - 1)
    else:
        x_start = 0.0
        dx = 0.0

    hole_centers = []
    for i in range(hole_count):
        x = x_start + i * dx
        y = amplitude * math.sin(2 * math.pi * x / wavelength)
        # only keep holes fully inside plate
        if -half_y + r <= y <= half_y - r:
            hole_centers.append((x, y))

    # 3) Drill holes through the thickness
    if hole_centers:
        plate = (
            plate
            .faces(">Z")
            .workplane()
            .pushPoints(hole_centers)
            .hole(hole_diameter)
        )

    # 4) Add triangular guslets at each hole, on both long edges
    for x, y in hole_centers:
        for side in (-1, 1):
            # direction to extrude: into the plate interior
            disp = guslet_width if side < 0 else -guslet_width
            # Y positions: plate edge and extended guslet tip
            Y0 = side * half_y
            Y1 = side * (half_y + guslet_width)
            # bottom guslet (under plate)
            bottom = (
                cq.Workplane("YZ", origin=(x, 0, -half_z))
                  .polyline([(Y0, 0), (Y1, 0), (Y0, -guslet_height)])
                  .close()
                  .extrude(disp)
            )
            plate = plate.union(bottom)
            # top guslet (above plate)
            top = (
                cq.Workplane("YZ", origin=(x, 0, half_z))
                  .polyline([(Y0, 0), (Y1, 0), (Y0, guslet_height)])
                  .close()
                  .extrude(disp)
            )
            plate = plate.union(top)

    return plate
