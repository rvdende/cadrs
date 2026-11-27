class GProp_GProps:
    def __init__(self, *a):
        self._mass = 0.0
        self._center = (0.0, 0.0, 0.0)

    def Mass(self):
        return self._mass

    def CentreOfMass(self):
        from .gp import gp_Pnt
        return gp_Pnt(*self._center)
