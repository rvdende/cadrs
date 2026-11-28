from ._shim import cq


class BRep_Tool:
    @staticmethod
    def Pnt_s(v):
        from .gp import gp_Pnt
        c = cq(v).Center()
        return gp_Pnt(c.x, c.y, c.z)
