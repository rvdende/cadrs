from ._shim import cq, wrap


class ShapeFix_Shape:
    def __init__(self, s=None):
        self._s = s

    def Init(self, s):
        self._s = s

    def Perform(self, *a):
        pass

    def Shape(self):
        return self._s


class ShapeFix_Solid(ShapeFix_Shape):
    def SolidFromShell(self, shell):
        import cadquery
        return wrap(cadquery.Solid.makeSolid(cq(shell)))

    def Solid(self):
        return self._s
