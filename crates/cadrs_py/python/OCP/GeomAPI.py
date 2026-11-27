class _Curve:
    """A B-spline as OCP hands it on: what makes it (cadquery builds its edge)."""

    def __init__(self, kind, points, periodic=False):
        self.kind, self.points, self.periodic = kind, points, periodic


class GeomAPI_Interpolate:
    def __init__(self, pts, *args):
        self._pts = pts.points()
        self._periodic = bool(args[0]) if args and isinstance(args[0], bool) else False
        if len(args) >= 2 and isinstance(args[1], bool):
            self._periodic = args[1]
        self._done = False

    def Load(self, *a):
        pass

    def Perform(self):
        self._done = True

    def IsDone(self):
        return True

    def Curve(self):
        return _Curve("interpolate", self._pts, self._periodic)


class GeomAPI_PointsToBSpline:
    def __init__(self, pts, *a, **k):
        self._pts = pts.points()

    def IsDone(self):
        return True

    def Curve(self):
        return _Curve("approx", self._pts)
