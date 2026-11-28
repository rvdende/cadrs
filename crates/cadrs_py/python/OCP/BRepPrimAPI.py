from ._shim import wrap


def _cq():
    import cadquery
    return cadquery


class _Made:
    def Build(self, *a):
        pass

    def __init__(self, s):
        self._s = s

    def Shape(self):
        return wrap(self._s)

    def Solid(self):
        return wrap(self._s)

    def IsDone(self):
        return True


class BRepPrimAPI_MakeTorus(_Made):
    def Build(self, *a):
        pass

    def __init__(self, *a):
        if hasattr(a[0], "p"):
            raise NotImplementedError("BRepPrimAPI_MakeTorus with an axis")
        super().__init__(_cq().Solid.makeTorus(a[0], a[1]))


class BRepPrimAPI_MakeBox(_Made):
    def Build(self, *a):
        pass

    def __init__(self, *a):
        if len(a) == 3:
            super().__init__(_cq().Solid.makeBox(*a))
        else:
            raise NotImplementedError("BRepPrimAPI_MakeBox with these arguments")


class BRepPrimAPI_MakeCylinder(_Made):
    def Build(self, *a):
        pass

    def __init__(self, *a):
        if len(a) == 2:
            super().__init__(_cq().Solid.makeCylinder(*a))
        else:
            raise NotImplementedError("BRepPrimAPI_MakeCylinder with these arguments")


class BRepPrimAPI_MakeSphere(_Made):
    def Build(self, *a):
        pass

    def __init__(self, *a):
        if len(a) == 1:
            super().__init__(_cq().Solid.makeSphere(a[0], angleDegrees1=-90, angleDegrees2=90))
        else:
            raise NotImplementedError("BRepPrimAPI_MakeSphere with these arguments")


class BRepPrimAPI_MakeCone(_Made):
    def Build(self, *a):
        pass

    def __init__(self, *a):
        if len(a) == 3:
            super().__init__(_cq().Solid.makeCone(*a))
        else:
            raise NotImplementedError("BRepPrimAPI_MakeCone with these arguments")


class BRepPrimAPI_MakePrism:
    def Build(self, *a):
        pass

    def __init__(self, *a):
        raise NotImplementedError("BRepPrimAPI_MakePrism")


class BRepPrimAPI_MakeRevol:
    def Build(self, *a):
        pass

    def __init__(self, *a):
        raise NotImplementedError("BRepPrimAPI_MakeRevol")
