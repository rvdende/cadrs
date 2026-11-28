from ._shim import cq, wrap


class _Bool:
    def Build(self, *a):
        pass

    op = "fuse"

    def __init__(self, a=None, b=None):
        self._a, self._b = a, b

    def Shape(self):
        return wrap(getattr(cq(self._a), self.op)(cq(self._b)))

    def IsDone(self):
        return True


class BRepAlgoAPI_Fuse(_Bool):
    def Build(self, *a):
        pass

    op = "fuse"


class BRepAlgoAPI_Cut(_Bool):
    def Build(self, *a):
        pass

    op = "cut"


class BRepAlgoAPI_Common(_Bool):
    def Build(self, *a):
        pass

    op = "intersect"
