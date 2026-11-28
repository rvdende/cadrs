class TColgp_Array1OfPnt:
    def __init__(self, lo=1, hi=0):
        self._lo = lo
        self._v = [None] * (hi - lo + 1)

    def SetValue(self, i, p):
        self._v[i - self._lo] = p

    def Value(self, i):
        return self._v[i - self._lo]

    def Length(self):
        return len(self._v)

    def Lower(self):
        return self._lo

    def Upper(self):
        return self._lo + len(self._v) - 1

    def points(self):
        return [(p.X(), p.Y(), p.Z()) for p in self._v]


class TColgp_HArray1OfPnt(TColgp_Array1OfPnt):
    def __init__(self, a=1, hi=None):
        if isinstance(a, TColgp_Array1OfPnt):
            self._lo, self._v = a._lo, list(a._v)
        else:
            super().__init__(a, hi)


class TColgp_Array1OfVec(TColgp_Array1OfPnt):
    pass


class TColgp_Array2OfPnt:
    def __init__(self, *a):
        raise NotImplementedError("TColgp_Array2OfPnt")
