from ._shim import TopoDS_Shape, cq, wrap


def _cq():
    import cadquery
    return cadquery


class BRepBuilderAPI_MakeEdge:
    def Build(self, *a):
        pass

    def __init__(self, *a):
        c = _cq()
        if len(a) == 2 and hasattr(a[0], "X"):
            self._e = c.Edge.makeLine(c.Vector(a[0].X(), a[0].Y(), a[0].Z()), c.Vector(a[1].X(), a[1].Y(), a[1].Z()))
        elif len(a) >= 1 and hasattr(a[0], "kind"):
            curve = a[0]
            pts = [c.Vector(*p) for p in curve.points]
            if curve.kind == "approx":
                self._e = c.Edge.makeSplineApprox(pts)
            else:
                self._e = c.Edge.makeSpline(pts, periodic=curve.periodic)
        else:
            raise NotImplementedError("BRepBuilderAPI_MakeEdge with these arguments")

    def IsDone(self):
        return True

    def Edge(self):
        return wrap(self._e)

    def Shape(self):
        return self.Edge()


class BRepBuilderAPI_MakeWire:
    def Build(self, *a):
        pass

    def __init__(self, *edges):
        self._edges = []
        for e in edges:
            self.Add(e)

    def Add(self, e):
        s = cq(e)
        self._edges.extend(s.Edges() if s.ShapeType() == "Wire" else [s])

    def IsDone(self):
        return bool(self._edges)

    def Wire(self):
        return wrap(_cq().Wire.assembleEdges(self._edges))

    def Shape(self):
        return self.Wire()


class BRepBuilderAPI_Sewing:
    def Build(self, *a):
        pass

    def __init__(self, *a):
        self._faces = []

    def Add(self, s):
        self._faces.extend(cq(s).Faces())

    def Perform(self):
        self._res = _cq().Shell.makeShell(self._faces)

    def SewedShape(self):
        return wrap(self._res)


class BRepBuilderAPI_MakeSolid:
    def Build(self, *a):
        pass

    def __init__(self, *shells):
        self._shells = [cq(s) for s in shells]

    def Add(self, s):
        self._shells.append(cq(s))

    def IsDone(self):
        return True

    def Solid(self):
        return wrap(_cq().Solid.makeSolid(self._shells[0]))

    def Shape(self):
        return self.Solid()


class BRepBuilderAPI_MakeFace:
    def Build(self, *a):
        pass

    def __init__(self, *a):
        raise NotImplementedError("BRepBuilderAPI_MakeFace")


class BRepBuilderAPI_Transform:
    def Build(self, *a):
        pass

    def __init__(self, *a):
        raise NotImplementedError("BRepBuilderAPI_Transform")


class BRepBuilderAPI_MakePolygon:
    def Build(self, *a):
        pass

    def __init__(self, *a):
        raise NotImplementedError("BRepBuilderAPI_MakePolygon")
