import cadquery as cq

def cone_with_chamfered_base(base_radius: float = 10.0,
                             height: float = 20.0,
                             bevel_size: float = 1.0) -> cq.Workplane:
    """
    Cone with chamfered larger end.

    Creates a right circular cone of given base_radius and height,
    then adds a bevel of size bevel_size at the base edge by
    incorporating it directly into the revolve profile.
    """
    profile = (
        cq.Workplane("XZ")
          .moveTo(0, 0)
          .lineTo(base_radius, 0)
          .lineTo(base_radius - bevel_size, bevel_size)
          .lineTo(0, height)
          .close()
    )
    return profile.revolve(360, (0, 0, 0), (0, 0, 1))
