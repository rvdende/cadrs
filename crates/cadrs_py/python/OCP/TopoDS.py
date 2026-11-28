from ._shim import TopoDS_Shape, cq, wrap


class TopoDS_Solid(TopoDS_Shape):
    pass


class TopoDS_Shell(TopoDS_Shape):
    pass


class TopoDS_Face(TopoDS_Shape):
    pass


class TopoDS_Edge(TopoDS_Shape):
    pass


class TopoDS_Wire(TopoDS_Shape):
    pass


class TopoDS_Vertex(TopoDS_Shape):
    pass


class TopoDS_Compound(TopoDS_Shape):
    pass


class TopoDS:
    @staticmethod
    def _same(s):
        return s

    Solid_s = Shell_s = Face_s = Edge_s = Wire_s = Vertex_s = Compound_s = _same
    Solid = Shell = Face = Edge = Wire = Vertex = Compound = _same


class TopoDS_Iterator:
    def __init__(self, shape):
        s = cq(shape)
        try:
            self._items = [wrap(x) for x in s]
        except TypeError:
            self._items = []
        self._i = 0

    def More(self):
        return self._i < len(self._items)

    def Value(self):
        return self._items[self._i]

    def Next(self):
        self._i += 1


class TopoDS_Builder:
    def MakeCompound(self, c):
        c._items = []

    def Add(self, c, s):
        c._items.append(s)
        import cadquery
        c._cq_shape = cadquery.Compound.makeCompound([cq(x) for x in c._items])
