class Bnd_Box:
    def __init__(self):
        self._b = None

    def Get(self):
        b = self._b
        return (b.xmin, b.ymin, b.zmin, b.xmax, b.ymax, b.zmax)

    def IsVoid(self):
        return self._b is None
