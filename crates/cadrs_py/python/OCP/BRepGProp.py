from ._shim import cq


def _set(props, mass, center):
    props._mass = mass
    props._center = (center.x, center.y, center.z)


class BRepGProp:
    @staticmethod
    def VolumeProperties_s(shape, props, *a):
        s = cq(shape)
        _set(props, s.Volume(), s.Center())

    @staticmethod
    def SurfaceProperties_s(shape, props, *a):
        s = cq(shape)
        _set(props, s.Area(), s.Center())

    @staticmethod
    def LinearProperties_s(shape, props, *a):
        s = cq(shape)
        _set(props, sum(e.Length() for e in s.Edges()), s.Center())
