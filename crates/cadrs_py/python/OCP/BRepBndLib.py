from ._shim import cq


class BRepBndLib:
    @staticmethod
    def Add_s(shape, box, *a):
        b = cq(shape).BoundingBox()
        box._b = b if box._b is None else box._b.add(b)

    AddOptimal_s = Add_s
