"""`cadquery.selectors`, native."""
from ._cadrs import (
    Selector,
    StringSyntaxSelector,
    DirectionSelector,
    ParallelDirSelector,
    PerpendicularDirSelector,
    TypeSelector,
    DirectionMinMaxSelector,
    DirectionNthSelector,
    CenterNthSelector,
    RadiusNthSelector,
    LengthNthSelector,
    AreaNthSelector,
    NearestToPointSelector,
    BoxSelector,
    AndSelector,
    SumSelector,
    SubtractSelector,
    InverseSelector,
)

BaseDirSelector = Selector
BinarySelector = Selector
NearestToPoint = NearestToPointSelector
Parallel = ParallelDirSelector
Perpendicular = PerpendicularDirSelector
