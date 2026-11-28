"""CadQuery classes cadrs does not give (yet)."""


class Matrix:
    """Not supported on cadrs."""

    def __init__(self, *a, **k):
        raise NotImplementedError("cadquery.Matrix")


class Assembly:
    """Not supported on cadrs."""

    def __init__(self, *a, **k):
        raise NotImplementedError("cadquery.Assembly")


class Sketch:
    """Not supported on cadrs."""

    def __init__(self, *a, **k):
        raise NotImplementedError("cadquery.Sketch")


class Color:
    def __init__(self, *a, **k):
        self.args = a


