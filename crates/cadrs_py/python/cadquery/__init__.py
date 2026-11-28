"""CadQuery on cadrs's kernel: the classes of `cadquery`, made natively (`_cadrs`)."""

from ._cadrs import (
    Vector,
    Plane,
    Location,
    BoundBox,
    Shape,
    Vertex,
    Edge,
    Wire,
    Face,
    Shell,
    Solid,
    CompSolid,
    Compound,
    Workplane,
    wiresToFaces,
    sortWiresByBuildOrder,
    edgesToWires,
    set_tolerance,
    tolerance,
)
from ._extra import Matrix, Assembly, Sketch, Color
from . import selectors, occ_impl, exporters, importers
from .selectors import (
    NearestToPointSelector,
    ParallelDirSelector,
    DirectionSelector,
    PerpendicularDirSelector,
    TypeSelector,
    DirectionMinMaxSelector,
    StringSyntaxSelector,
    Selector,
)

CQ = Workplane
__version__ = "2.8.0"


__all__ = [
    "Vector", "Plane", "Location", "BoundBox", "Shape", "Vertex", "Edge", "Wire", "Face", "Shell",
    "Solid", "CompSolid", "Compound", "Workplane", "CQ", "selectors", "Selector",
    "StringSyntaxSelector", "NearestToPointSelector", "ParallelDirSelector", "DirectionSelector",
    "PerpendicularDirSelector", "TypeSelector", "DirectionMinMaxSelector", "exporters", "importers",
]
