from ._shim import children


class TopExp_Explorer:
    def __init__(self, shape=None, kind=None, avoid=None):
        self._items = children(shape, kind) if shape is not None else []
        self._i = 0

    def Init(self, shape, kind, avoid=None):
        self.__init__(shape, kind)

    def More(self):
        return self._i < len(self._items)

    def Current(self):
        return self._items[self._i]

    def Value(self):
        return self.Current()

    def Next(self):
        self._i += 1


class TopExp:
    @staticmethod
    def MapShapes_s(shape, kind, out):
        for c in children(shape, kind):
            out.Add(c)
