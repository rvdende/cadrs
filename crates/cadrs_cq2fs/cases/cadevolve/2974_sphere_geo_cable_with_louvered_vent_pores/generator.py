import cadquery as cq
import math
from cadquery import Plane

def sphere_geo_cable_with_louvered_vent_pores(
    outer_radius: float = 20.0,
    thickness: float = 2.0,
    latitudes: int = 4,
    longitudes: int = 12,
    slot_width: float = None,
    slot_height: float = None,
    slot_depth: float = None,
    slot_angle_deg: float = 30.0,
    fillet_radius: float = 0.2
) -> cq.Workplane:
    """
    Build a spherical shell and cut louvered ventilation pores into it.
    The pores are rectangular slots angled by slot_angle_deg, arranged
    in a latitudes×longitudes grid on the mid-surface of the shell.

    Parameters:
        outer_radius: radius of the outer sphere
        thickness: shell thickness (must be < outer_radius)
        latitudes: number of rings of slots between pole and equator
        longitudes: slots per ring around the sphere
        slot_width: tangential width of each slot (defaults to 80% of segment)
        slot_height: meridional height of each slot (defaults to 80% of segment)
        slot_depth: depth of cut into shell (defaults to 120% of thickness)
        slot_angle_deg: tilt angle of slots from radial
        fillet_radius: fillet on cut edges for smooth airflow
    """
    # parameter checks
    if outer_radius <= 0:
        raise ValueError("outer_radius must be > 0")
    if thickness <= 0 or thickness >= outer_radius:
        raise ValueError("thickness must be > 0 and < outer_radius")
    if latitudes < 0 or longitudes < 0:
        raise ValueError("latitudes and longitudes must be ≥ 0")

    # build outer sphere by revolving a full-profile
    outer = (
        cq.Workplane("XZ")
          .moveTo(0.0, outer_radius)
          .threePointArc((outer_radius, 0.0), (0.0, -outer_radius))
          .threePointArc((-outer_radius, 0.0), (0.0, outer_radius))
          .close()
          .revolve(360.0)
    )
    # build inner sphere and subtract to make shell
    inner_r = outer_radius - thickness
    if inner_r > 0:
        inner = (
            cq.Workplane("XZ")
              .moveTo(0.0, inner_r)
              .threePointArc((inner_r, 0.0), (0.0, -inner_r))
              .threePointArc((-inner_r, 0.0), (0.0, inner_r))
              .close()
              .revolve(360.0)
        )
        shell = outer.cut(inner)
    else:
        shell = outer

    # compute mid-surface radius
    mid_r = outer_radius - thickness / 2.0

    # trigonometric tilt components
    cos_a = math.cos(math.radians(slot_angle_deg))
    sin_a = math.sin(math.radians(slot_angle_deg))

    # default slot cut depth
    depth = slot_depth if (slot_depth and slot_depth > 0) else thickness * 1.2

    # if no slots requested, return shell
    if latitudes < 1 or longitudes < 1:
        return shell

    # meridional spacing angle
    delta_theta = math.pi / float(latitudes + 1)

    # default meridional slot height (arc length)
    default_h = mid_r * delta_theta * 0.8
    # loop over each latitude ring
    for i in range(latitudes):
        theta = (i + 1) * delta_theta
        sin_t = math.sin(theta)
        cos_t = math.cos(theta)
        # center Z of this ring on mid-surface
        z0 = mid_r * cos_t
        # ring radius in XY
        ring_r = mid_r * sin_t

        # choose slot height
        h = slot_height if (slot_height and slot_height > 0) else default_h

        # loop around ring
        for j in range(longitudes):
            phi = 2.0 * math.pi * j / longitudes
            nx = math.cos(phi)
            ny = math.sin(phi)
            x0 = ring_r * nx
            y0 = ring_r * ny

            # default tangential width at this ring
            default_w = (2.0 * math.pi * ring_r / longitudes) * 0.8
            w = slot_width if (slot_width and slot_width > 0) else default_w

            # compute inward normal vector on mid-surface
            rx = sin_t * nx
            ry = sin_t * ny
            rz = cos_t
            normal = (-rx, -ry, -rz)

            # compute xDir for the tilted cutter plane
            circ = (-ny, nx, 0.0)
            xdir = (circ[0] * cos_a, circ[1] * cos_a, sin_a)

            # create and apply cutter
            plane = Plane(origin=(x0, y0, z0), normal=normal, xDir=xdir)
            cutter = cq.Workplane(plane).rect(w, h).extrude(depth)
            shell = shell.cut(cutter)

    # fillet the slot edges for smoother flow
    if fillet_radius and fillet_radius > 0:
        try:
            shell = shell.edges().fillet(fillet_radius)
        except Exception:
            pass

    return shell

# Example usage:
# model = sphere_geo_cable_with_louvered_vent_pores(
#     outer_radius=30, thickness=3,
#     latitudes=5, longitudes=16,
#     slot_angle_deg=25, fillet_radius=0.5
# )
# show_object(model)  # in a CadQuery viewer environment
