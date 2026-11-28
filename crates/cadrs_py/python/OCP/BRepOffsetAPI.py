from ._shim import cq, wrap


class BRepOffsetAPI_ThruSections:
    def Build(self, *a):
        pass

    def __init__(self, isSolid=False, ruled=False, *a):
        self._ruled = ruled
        self._wires = []

    def AddWire(self, w):
        self._wires.append(cq(w))

    def AddVertex(self, v):
        self._wires.append(cq(v))

    def CheckCompatibility(self, *a):
        pass

    def Build(self):
        import cadquery
        self._res = cadquery.Solid.makeLoft(self._wires, self._ruled)

    def IsDone(self):
        return True

    def Shape(self):
        return wrap(self._res)


class BRepOffsetAPI_MakePipe:
    def Build(self, *a):
        pass

    def __init__(self, *a):
        raise NotImplementedError("BRepOffsetAPI_MakePipe")


class BRepOffsetAPI_MakeThickSolid:
    def Build(self, *a):
        pass

    def __init__(self, *a):
        raise NotImplementedError("BRepOffsetAPI_MakeThickSolid")
