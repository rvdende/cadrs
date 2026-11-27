"""`cadquery.exporters`: STEP and STL."""


def export(w, fname, exportType=None, tolerance=0.1, angularTolerance=0.1, **kw):
    from . import Workplane, Shape, Compound
    if isinstance(w, Workplane):
        w.export(fname)
    else:
        if str(fname).lower().endswith(".stl"):
            w.exportStl(fname)
        else:
            w.exportStep(fname)


class ExportTypes:
    STEP = "STEP"
    STL = "STL"
