import math


class gp_XYZ:
    def __init__(self, x=0.0, y=0.0, z=0.0):
        self._v = (float(x), float(y), float(z))

    def X(self):
        return self._v[0]

    def Y(self):
        return self._v[1]

    def Z(self):
        return self._v[2]

    def Coord(self, i=None):
        return self._v if i is None else self._v[i - 1]

    def toTuple(self):
        return self._v

    def XYZ(self):
        return gp_XYZ(*self._v)


class gp_Pnt(gp_XYZ):
    def Distance(self, o):
        return math.dist(self._v, o._v)


class gp_Vec(gp_XYZ):
    def Magnitude(self):
        return math.hypot(*self._v)


class gp_Dir(gp_XYZ):
    def __init__(self, x=0.0, y=0.0, z=1.0):
        n = math.hypot(x, y, z) or 1.0
        super().__init__(x / n, y / n, z / n)


class gp_Pnt2d:
    def __init__(self, x=0.0, y=0.0):
        self._v = (float(x), float(y))

    def X(self):
        return self._v[0]

    def Y(self):
        return self._v[1]


class gp_Ax1:
    def __init__(self, p=None, d=None):
        self.p, self.d = p or gp_Pnt(), d or gp_Dir()


class gp_Ax2(gp_Ax1):
    def __init__(self, p=None, d=None, x=None):
        super().__init__(p, d)
        self.x = x


gp_Ax3 = gp_Ax2


class gp_Trsf:
    pass


class gp_Circ:
    def __init__(self, ax=None, r=1.0):
        self.ax, self.r = ax, r


class gp_Pln:
    def __init__(self, *a):
        self.args = a
