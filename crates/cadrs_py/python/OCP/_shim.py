"""Shapes and geometry as OCP's classes see them."""
import enum


class TopAbs_ShapeEnum(enum.IntEnum):
    TopAbs_COMPOUND = 0
    TopAbs_COMPSOLID = 1
    TopAbs_SOLID = 2
    TopAbs_SHELL = 3
    TopAbs_FACE = 4
    TopAbs_WIRE = 5
    TopAbs_EDGE = 6
    TopAbs_VERTEX = 7
    TopAbs_SHAPE = 8


_KIND = {
    "Compound": TopAbs_ShapeEnum.TopAbs_COMPOUND,
    "CompSolid": TopAbs_ShapeEnum.TopAbs_COMPSOLID,
    "Solid": TopAbs_ShapeEnum.TopAbs_SOLID,
    "Shell": TopAbs_ShapeEnum.TopAbs_SHELL,
    "Face": TopAbs_ShapeEnum.TopAbs_FACE,
    "Wire": TopAbs_ShapeEnum.TopAbs_WIRE,
    "Edge": TopAbs_ShapeEnum.TopAbs_EDGE,
    "Vertex": TopAbs_ShapeEnum.TopAbs_VERTEX,
}
_LISTS = {
    TopAbs_ShapeEnum.TopAbs_SOLID: "Solids",
    TopAbs_ShapeEnum.TopAbs_SHELL: "Shells",
    TopAbs_ShapeEnum.TopAbs_FACE: "Faces",
    TopAbs_ShapeEnum.TopAbs_WIRE: "Wires",
    TopAbs_ShapeEnum.TopAbs_EDGE: "Edges",
    TopAbs_ShapeEnum.TopAbs_VERTEX: "Vertices",
    TopAbs_ShapeEnum.TopAbs_COMPOUND: "Compounds",
}


class TopoDS_Shape:
    """OCCT's shape handle: a cadquery shape (`_cq_shape`)."""

    def __init__(self, s=None):
        self._cq_shape = s

    def IsNull(self):
        return self._cq_shape is None

    def ShapeType(self):
        return _KIND[self._cq_shape.ShapeType()]

    def IsSame(self, o):
        return self._cq_shape.isSame(o._cq_shape)

    def IsEqual(self, o):
        return self.IsSame(o)

    def __eq__(self, o):
        return isinstance(o, TopoDS_Shape) and self.IsSame(o)

    def __hash__(self):
        return hash(self._cq_shape)

    def Closed(self):
        return self._cq_shape.Closed()

    def HashCode(self, upper=2147483647):
        return hash(self._cq_shape) % upper


def wrap(s):
    return TopoDS_Shape(s)


def cq(o):
    """The cadquery shape of a stand-in or shape."""
    return getattr(o, "_cq_shape", o)


def children(s, kind):
    s = cq(s)
    name = _LISTS.get(kind)
    if name is None:
        return []
    return [wrap(x) for x in getattr(s, name)()]
