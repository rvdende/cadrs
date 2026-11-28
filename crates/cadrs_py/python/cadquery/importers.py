def importStep(fileName):
    """A STEP file's shape on a Workplane, as CadQuery's importers.importStep."""
    from . import _cadrs, Workplane

    return Workplane("XY").add(_cadrs.import_step(fileName))


def importStl(fileName):
    raise NotImplementedError("importers.importStl")
