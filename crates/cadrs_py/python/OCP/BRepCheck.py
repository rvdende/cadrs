from ._shim import cq


class BRepCheck_Analyzer:
    def __init__(self, s, *a):
        self._s = cq(s)

    def IsValid(self):
        return self._s.isValid()
