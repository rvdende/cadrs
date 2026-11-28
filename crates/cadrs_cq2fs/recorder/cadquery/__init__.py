"""A recording stand-in for CadQuery: no kernel, no geometry.

Running a CADEvolve generator against this package resolves its Python (loops, arithmetic,
literal coordinates) and records every CadQuery call on a *tape*: which object it was called
on, the method, and its arguments as plain values. Geometry is never computed here; the
tape is translated to FeatureScript (cq2fs) and evaluated by Parasolid, where a FeatureScript
port of CadQuery's own semantics (cq.py, selectors.py, geom.py) runs it.

Anything that needs geometry back in Python (a volume, a bounding box, the number of selected
objects, a callback over the stack) raises `Dynamic`: such a generator is not translatable
and is left out. Names CadQuery has that are not ported raise `Unsupported`.

The pure-math parts of CadQuery the generators use directly (Vector, Plane, Location) are
ported exactly, so their values go on the tape already resolved.
"""
import json as _json
import math
import os as _os
import sys

__version__ = "2.8.0-recorder"

# Method names real CadQuery 2.8 has (cq.py, occ_impl/shapes.py): a name not among them raises
# AttributeError exactly as CadQuery would, so a generator's `except` path is the one CadQuery takes.
_CQ_NAMES = {k: set(v) for k, v in _json.load(open(_os.path.join(_os.path.dirname(__file__), "_cq_names.json"))).items()}


class Dynamic(Exception):
    """The generator needs a geometric answer while it runs."""


class Unsupported(Exception):
    """The generator uses CadQuery API the recorder does not port."""


# ----------------------------------------------------------------------------------------------
# the tape

class _Tape:
    def __init__(self):
        self.entries = []
        self.nwp = 0
        self.nshape = 0
        self.try_lines = []  # [(first, last)] line ranges of try bodies in the generator
        self.filename = None

    def caller_line(self):
        f = sys._getframe(1)
        while f is not None:
            if f.f_code.co_filename == self.filename:
                return f.f_lineno
            f = f.f_back
        return None

    def add(self, entry):
        line = self.caller_line()
        if line is not None:
            entry["line"] = line
            if any(a <= line <= b for a, b in self.try_lines):
                entry["guarded"] = True
        self.entries.append(entry)
        return entry


TAPE = _Tape()


def _reset(filename=None, try_lines=()):
    global TAPE
    TAPE = _Tape()
    TAPE.filename = filename
    TAPE.try_lines = list(try_lines)
    return TAPE


def _enc(v, where="argument"):
    """A Python value as tape JSON."""
    if v is None or isinstance(v, (bool, str)):
        return v
    if isinstance(v, int):
        return v
    if isinstance(v, float):
        if math.isnan(v) or math.isinf(v):
            raise Dynamic(f"non-finite number as {where}")
        return v
    if hasattr(v, "__float__") and type(v).__module__.startswith("numpy"):
        return float(v)
    if isinstance(v, (list, tuple)):
        return [_enc(x, where) for x in v]
    if isinstance(v, Vector):
        return {"vec": [v.x, v.y, v.z]}
    if isinstance(v, Plane):
        return {"plane": v._enc()}
    if isinstance(v, Location):
        return {"loc": v._enc()}
    if isinstance(v, Workplane):
        return {"wp": v._id}
    if isinstance(v, Shape):
        return {"shape": v._id}
    if isinstance(v, _SelectorBase):
        return {"sel": v._enc()}
    if isinstance(v, dict):
        return {"dict": {str(k): _enc(x, where) for k, x in v.items()}}
    if callable(v):
        raise Dynamic(f"a callable as {where}")
    raise Unsupported(f"cannot record a {type(v).__name__} as {where}")


# ----------------------------------------------------------------------------------------------
# Vector (cadquery/occ_impl/geom.py), exact

class Vector:
    __slots__ = ("x", "y", "z")

    def __init__(self, *args):
        if len(args) == 3:
            x, y, z = args
        elif len(args) == 2:
            x, y = args
            z = 0.0
        elif len(args) == 1:
            a = args[0]
            if isinstance(a, Vector):
                x, y, z = a.x, a.y, a.z
            elif isinstance(a, (tuple, list)):
                if len(a) == 3:
                    x, y, z = a
                elif len(a) == 2:
                    x, y = a
                    z = 0.0
                else:
                    raise TypeError("Expected three floats, OCC gp_, or 3-tuple")
            else:
                raise TypeError("Expected three floats, OCC gp_, or 3-tuple")
        elif len(args) == 0:
            x = y = z = 0.0
        else:
            raise TypeError("Expected three floats, OCC gp_, or 3-tuple")
        for c in (x, y, z):
            if type(c).__name__ in ("Workplane", "Shape"):
                raise Dynamic("Vector built from a shape")
        self.x, self.y, self.z = float(x), float(y), float(z)

    # properties CadQuery has
    @property
    def Length(self):
        return math.sqrt(self.x * self.x + self.y * self.y + self.z * self.z)

    @property
    def wrapped(self):
        raise Unsupported("Vector.wrapped (OCP)")

    def toTuple(self):
        return (self.x, self.y, self.z)

    def cross(self, v):
        return Vector(self.y * v.z - self.z * v.y, self.z * v.x - self.x * v.z, self.x * v.y - self.y * v.x)

    def dot(self, v):
        return self.x * v.x + self.y * v.y + self.z * v.z

    def sub(self, v):
        v = Vector(v) if not isinstance(v, Vector) else v
        return Vector(self.x - v.x, self.y - v.y, self.z - v.z)

    def add(self, v):
        v = Vector(v) if not isinstance(v, Vector) else v
        return Vector(self.x + v.x, self.y + v.y, self.z + v.z)

    def multiply(self, s):
        return Vector(self.x * s, self.y * s, self.z * s)

    def normalized(self):
        n = self.Length
        if n <= 1e-300:
            raise ValueError("cannot normalize a null vector")  # gp_Vec raises too
        return Vector(self.x / n, self.y / n, self.z / n)

    def Center(self):
        return self

    def getAngle(self, v):
        # gp_Vec::Angle
        a = self.Length * v.Length
        if a <= 1e-300:
            raise ValueError("angle with a null vector")
        c = max(-1.0, min(1.0, self.dot(v) / a))
        cr = self.cross(v).Length / a
        if c > -0.70710678118655 and c < 0.70710678118655:
            return math.acos(c)
        return math.asin(cr) if c >= 0 else math.pi - math.asin(cr)

    def getSignedAngle(self, v):
        raise Unsupported("Vector.getSignedAngle")

    def distanceToLine(self, *a):
        raise Unsupported("Vector.distanceToLine")

    def projectToPlane(self, plane):
        base = plane.origin
        normal = plane.zDir
        return self - normal * (((self - base).dot(normal)) / normal.Length ** 2)

    def projectToLine(self, line):
        lineLength = line.Length
        return line * (self.dot(line) / (lineLength * lineLength))

    def transform(self, T):
        if isinstance(T, Location):
            return T._apply_point(self)
        raise Unsupported("Vector.transform with a Matrix")

    def __add__(self, v):
        return self.add(v)

    def __sub__(self, v):
        return self.sub(v)

    def __mul__(self, s):
        if isinstance(s, Vector):
            raise TypeError("Vector * Vector")
        return self.multiply(s)

    __rmul__ = __mul__

    def __truediv__(self, s):
        return self.multiply(1.0 / s)

    def __neg__(self):
        return self * -1.0

    def __abs__(self):
        return self.Length

    def __eq__(self, other):
        if not isinstance(other, Vector):
            return NotImplemented
        return (self - other).Length < 1e-7 * max(1.0, self.Length)  # gp_Vec::IsEqual(o, 1e-7, 1e-7)

    def __hash__(self):
        return hash((round(self.x, 9), round(self.y, 9), round(self.z, 9)))

    def __iter__(self):
        return iter((self.x, self.y, self.z))

    def __getitem__(self, i):
        return (self.x, self.y, self.z)[i]

    def __repr__(self):
        return f"Vector: ({self.x}, {self.y}, {self.z})"

    def toPnt(self):
        raise Unsupported("Vector.toPnt (OCP)")

    def toDir(self):
        raise Unsupported("Vector.toDir (OCP)")


def _vec(v):
    return v if isinstance(v, Vector) else Vector(v)


# 3x3 rotation helpers (row-major lists)
def _mat_mul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]


def _mat_vec(m, v):
    return Vector(*(m[i][0] * v.x + m[i][1] * v.y + m[i][2] * v.z for i in range(3)))


def _rot_axis(axis, ang):
    """Rotation by `ang` radians about unit `axis` (gp_Trsf::SetRotation, right-handed)."""
    a = _vec(axis).normalized()
    c, s = math.cos(ang), math.sin(ang)
    t = 1.0 - c
    x, y, z = a.x, a.y, a.z
    return [[t * x * x + c, t * x * y - s * z, t * x * z + s * y],
            [t * x * y + s * z, t * y * y + c, t * y * z - s * x],
            [t * x * z - s * y, t * y * z + s * x, t * z * z + c]]


def _default_xdir(n):
    """gp_Ax2(P, V) / gp_Ax3(P, V): OCCT's choice of X direction for a normal alone."""
    A, B, C = n.x, n.y, n.z
    Aa, Ba, Ca = abs(A), abs(B), abs(C)
    if Ba <= Aa and Ba <= Ca:
        d = Vector(-C, 0.0, A) if Aa > Ca else Vector(C, 0.0, -A)
    elif Aa <= Ba and Aa <= Ca:
        d = Vector(0.0, -C, B) if Ba > Ca else Vector(0.0, C, -B)
    else:
        d = Vector(-B, A, 0.0) if Aa > Ba else Vector(B, -A, 0.0)
    return d.normalized()


# ----------------------------------------------------------------------------------------------
# Plane (geom.py), exact for everything the generators can observe

class Plane:
    @classmethod
    def named(cls, stdName, origin=(0, 0, 0)):
        namedPlanes = {
            "XY": ((1, 0, 0), (0, 0, 1)),
            "YZ": ((0, 1, 0), (1, 0, 0)),
            "ZX": ((0, 0, 1), (0, 1, 0)),
            "XZ": ((1, 0, 0), (0, -1, 0)),
            "YX": ((0, 1, 0), (0, 0, -1)),
            "ZY": ((0, 0, 1), (-1, 0, 0)),
            "front": ((1, 0, 0), (0, 0, 1)),
            "back": ((-1, 0, 0), (0, 0, -1)),
            "left": ((0, 0, 1), (-1, 0, 0)),
            "right": ((0, 0, -1), (1, 0, 0)),
            "top": ((1, 0, 0), (0, 1, 0)),
            "bottom": ((1, 0, 0), (0, -1, 0)),
        }
        try:
            x, n = namedPlanes[stdName]
        except KeyError:
            raise ValueError("Supported names are {}".format(list(namedPlanes.keys())))
        return Plane(origin, x, n)

    @classmethod
    def XY(cls, origin=(0, 0, 0), xDir=Vector(1, 0, 0)):
        p = Plane.named("XY", origin)
        p._setPlaneDir(xDir)
        return p

    @classmethod
    def YZ(cls, origin=(0, 0, 0), xDir=Vector(0, 1, 0)):
        p = Plane.named("YZ", origin)
        p._setPlaneDir(xDir)
        return p

    @classmethod
    def ZX(cls, origin=(0, 0, 0), xDir=Vector(0, 0, 1)):
        p = Plane.named("ZX", origin)
        p._setPlaneDir(xDir)
        return p

    @classmethod
    def XZ(cls, origin=(0, 0, 0), xDir=Vector(1, 0, 0)):
        p = Plane.named("XZ", origin)
        p._setPlaneDir(xDir)
        return p

    @classmethod
    def YX(cls, origin=(0, 0, 0), xDir=Vector(0, 1, 0)):
        p = Plane.named("YX", origin)
        p._setPlaneDir(xDir)
        return p

    @classmethod
    def ZY(cls, origin=(0, 0, 0), xDir=Vector(0, 0, 1)):
        p = Plane.named("ZY", origin)
        p._setPlaneDir(xDir)
        return p

    def __init__(self, origin=(0, 0, 0), xDir=None, normal=(0, 0, 1)):
        if isinstance(origin, Location):
            raise Unsupported("Plane(Location)")
        zDir = _vec(normal)
        if zDir.Length == 0.0:
            raise ValueError("normal should be non null")
        self.zDir = zDir.normalized()
        if xDir is None:
            xDir = _default_xdir(_vec(normal).normalized())
        else:
            xDir = _vec(xDir)
            if xDir.Length == 0.0:
                raise ValueError("xDir should be non null")
        self._setPlaneDir(xDir)
        self.origin = _vec(origin)

    def _setPlaneDir(self, xDir):
        xDir = _vec(xDir)
        self.xDir = xDir.normalized()
        self.yDir = self.zDir.cross(self.xDir).normalized()

    # the frame OCCT's gp_Ax3(origin, zDir, xDir) builds: X made perpendicular to Z
    def _frame(self):
        z = self.zDir
        x = z.cross(self.xDir.cross(z))
        if x.Length < 1e-12:
            raise ValueError("xDir parallel to the normal")
        x = x.normalized()
        y = z.cross(x)
        return x, y, z

    def toWorldCoords(self, p):
        v = p if isinstance(p, Vector) else (Vector(p[0], p[1], 0) if len(p) == 2 else Vector(p))
        x, y, z = self._frame()
        o = self.origin
        return Vector(o.x + v.x * x.x + v.y * y.x + v.z * z.x,
                      o.y + v.x * x.y + v.y * y.y + v.z * z.y,
                      o.z + v.x * x.z + v.y * y.z + v.z * z.z)

    def toLocalCoords(self, obj):
        if not isinstance(obj, Vector):
            raise Dynamic("Plane.toLocalCoords of a shape")
        x, y, z = self._frame()
        d = obj - self.origin
        return Vector(d.dot(x), d.dot(y), d.dot(z))

    def setOrigin2d(self, x, y):
        self.origin = self.toWorldCoords((x, y))

    def rotated(self, rotate=(0, 0, 0)):
        r = _vec(rotate).multiply(math.pi / 180.0)
        T1 = _rot_axis(self.xDir, r.x)
        T2 = _rot_axis(self.yDir, r.y)
        T3 = _rot_axis(self.zDir, r.z)
        T = _mat_mul(_mat_mul(T1, T2), T3)
        return Plane(self.origin, _mat_vec(T, self.xDir), _mat_vec(T, self.zDir))

    @property
    def location(self):
        return Location(self)

    def toPln(self):
        raise Unsupported("Plane.toPln (OCP)")

    def mirrorInPlane(self, *a, **k):
        raise Unsupported("Plane.mirrorInPlane")

    def _enc(self):
        x, y, z = self._frame()
        return {"origin": list(self.origin.toTuple()), "xDir": list(x.toTuple()), "zDir": list(z.toTuple())}

    def __repr__(self):
        return f"Plane(origin={self.origin.toTuple()}, xDir={self.xDir.toTuple()}, normal={self.zDir.toTuple()})"


# ----------------------------------------------------------------------------------------------
# Location (geom.py): a rigid transform, kept as a 3x3 rotation and a translation

class Location:
    def __init__(self, *args):
        R = [[1.0, 0, 0], [0, 1.0, 0], [0, 0, 1.0]]
        t = Vector()
        if len(args) == 0:
            pass
        elif len(args) == 1 and isinstance(args[0], Plane):
            x, y, z = args[0]._frame()
            R = [[x.x, y.x, z.x], [x.y, y.y, z.y], [x.z, y.z, z.z]]
            t = args[0].origin
        elif len(args) == 2 and isinstance(args[0], Plane):
            x, y, z = args[0]._frame()
            R = [[x.x, y.x, z.x], [x.y, y.y, z.y], [x.z, y.z, z.z]]
            t = _vec(args[1])
        elif len(args) == 1 and isinstance(args[0], Location):
            R = [row[:] for row in args[0]._R]
            t = args[0]._t
        elif len(args) == 1 and isinstance(args[0], (Vector, tuple, list)):
            t = _vec(args[0])
        elif len(args) == 3 and isinstance(args[1], (Vector, tuple, list)) and not isinstance(args[0], (int, float)):
            t = _vec(args[0])
            R = _rot_axis(_vec(args[1]), math.radians(args[2]))
        elif len(args) == 2 and isinstance(args[0], (Vector, tuple, list)) and isinstance(args[1], (tuple, list)):
            t = _vec(args[0])
            R = _euler_xyz(*args[1])
        elif all(isinstance(a, (int, float)) for a in args) and len(args) <= 6:
            a = list(args) + [0] * (6 - len(args))
            t = Vector(a[0], a[1], a[2])
            R = _euler_xyz(a[3], a[4], a[5])
        else:
            raise Unsupported(f"Location{tuple(type(a).__name__ for a in args)}")
        self._R = R
        self._t = t

    def _apply_point(self, v):
        r = _mat_vec(self._R, v)
        return r + self._t

    def __mul__(self, other):
        if not isinstance(other, Location):
            raise TypeError("Location * non-Location")
        out = Location()
        out._R = _mat_mul(self._R, other._R)
        out._t = _mat_vec(self._R, other._t) + self._t
        return out

    @property
    def inverse(self):
        out = Location()
        Rt = [[self._R[j][i] for j in range(3)] for i in range(3)]
        out._R = Rt
        out._t = _mat_vec(Rt, self._t) * -1.0
        return out

    def toTuple(self):
        raise Dynamic("Location.toTuple")

    def _enc(self):
        return {"R": self._R, "t": list(self._t.toTuple())}


def _euler_xyz(rx, ry, rz):
    """gp_Quaternion::SetEulerAngles(gp_Extrinsic_XYZ, rx, ry, rz): R = Rz * Ry * Rx."""
    Rx = _rot_axis(Vector(1, 0, 0), math.radians(rx))
    Ry = _rot_axis(Vector(0, 1, 0), math.radians(ry))
    Rz = _rot_axis(Vector(0, 0, 1), math.radians(rz))
    return _mat_mul(Rz, _mat_mul(Ry, Rx))


# ----------------------------------------------------------------------------------------------
# selectors (recorded, evaluated by the FeatureScript port)

class _SelectorBase:
    kind = None

    def __init__(self, *args, **kwargs):
        self.args = [_enc(a, "selector argument") for a in args]
        self.kwargs = {k: _enc(v, "selector argument") for k, v in kwargs.items()}

    def _enc(self):
        return {"kind": self.kind, "args": self.args, "kwargs": self.kwargs}

    def __and__(self, other):
        return _Combo("and", self, other)

    def __add__(self, other):
        return _Combo("or", self, other)

    def __sub__(self, other):
        return _Combo("exc", self, other)

    def __neg__(self):
        return _Combo("not", self)

    def filter(self, objs):
        raise Dynamic("Selector.filter called from Python")


class _Combo(_SelectorBase):
    kind = "combo"

    def __init__(self, op, *parts):
        self.op = op
        self.parts = parts

    def _enc(self):
        return {"kind": "combo", "op": self.op, "parts": [p._enc() for p in self.parts]}


class _SelectorsModule:
    pass


selectors = _SelectorsModule()
for _name in ("StringSyntaxSelector", "DirectionMinMaxSelector", "ParallelDirSelector", "DirectionSelector",
              "PerpendicularDirSelector", "TypeSelector", "DirectionNthSelector", "RadiusNthSelector",
              "CenterNthSelector", "LengthNthSelector", "AreaNthSelector", "NearestToPointSelector",
              "BoxSelector"):
    setattr(selectors, _name, type(_name, (_SelectorBase,), {"kind": _name}))
selectors.Selector = _SelectorBase
selectors.AndSelector = lambda a, b: _Combo("and", a, b)
selectors.SumSelector = lambda a, b: _Combo("or", a, b)
selectors.SubtractSelector = lambda a, b: _Combo("exc", a, b)
selectors.InverseSelector = lambda a: _Combo("not", a)
sys.modules[__name__ + ".selectors"] = selectors
for _name in dir(selectors):
    if _name.endswith("Selector"):
        globals()[_name] = getattr(selectors, _name)


# ----------------------------------------------------------------------------------------------
# Shapes: handles to values the FeatureScript side computes

class Shape:
    """A shape value: from `Workplane.val()`, a `cq.Solid.make*` constructor, or a shape method."""

    def __init__(self, entry):
        TAPE.nshape += 1
        self._id = TAPE.nshape
        entry["out"] = {"shape": self._id}
        TAPE.add(entry)

    def _op(self, op, *args, **kwargs):
        return Shape({"op": "shape." + op, "self": {"shape": self._id},
                      "args": [_enc(a) for a in args], "kwargs": {k: _enc(v) for k, v in kwargs.items()}})

    def __getattr__(self, name):
        if name.startswith("_"):
            raise AttributeError(name)
        if name not in _CQ_NAMES["Shape"]:
            raise AttributeError(f"'Shape' object has no attribute '{name}'")
        if name in _SHAPE_OPS:
            return lambda *a, **k: self._op(name, *a, **k)
        raise Dynamic(f"Shape.{name} (geometry read back in Python)")

    def __bool__(self):
        return True

    def __iter__(self):
        raise Dynamic("iterating a shape")

    def __len__(self):
        raise Dynamic("len() of a shape")


_SHAPE_OPS = {"translate", "rotate", "mirror", "moved", "located", "move", "locate", "fuse", "cut", "intersect",
              "clean", "fix", "transformShape", "scale", "copy", "fillet", "chamfer", "shell", "split"}


class _ShapeClass:
    """cq.Solid, cq.Edge, cq.Wire, ... : static constructors recorded, everything else Unsupported."""

    def __init__(self, name):
        self._name = name

    def __getattr__(self, attr):
        if self._name in _CQ_NAMES and attr not in _CQ_NAMES[self._name]:
            raise AttributeError(f"type object '{self._name}' has no attribute '{attr}'")
        if attr.startswith("make") or attr in ("extrudeLinear", "revolve", "sweep", "loft", "assembleEdges",
                                               "combine"):
            def ctor(*args, **kwargs):
                return Shape({"op": f"{self._name}.{attr}", "args": [_enc(a) for a in args],
                              "kwargs": {k: _enc(v) for k, v in kwargs.items()}})
            return ctor
        raise Unsupported(f"cq.{self._name}.{attr}")

    def __call__(self, *a, **k):
        raise Unsupported(f"cq.{self._name}(...) constructed from OCP objects")

    def __instancecheck__(self, inst):
        raise Dynamic(f"isinstance(..., cq.{self._name})")


Solid = _ShapeClass("Solid")
Edge = _ShapeClass("Edge")
Wire = _ShapeClass("Wire")
Face = _ShapeClass("Face")
Shell = _ShapeClass("Shell")
Compound = _ShapeClass("Compound")
Vertex = _ShapeClass("Vertex")
Sketch = _ShapeClass("Sketch")


class _Unported:
    def __init__(self, name):
        self._name = name

    def __getattr__(self, attr):
        raise Unsupported(f"cq.{self._name}.{attr}")

    def __call__(self, *a, **k):
        raise Unsupported(f"cq.{self._name}")


Assembly = _Unported("Assembly")
Color = _Unported("Color")
Matrix = _Unported("Matrix")
exporters = _Unported("exporters")
importers = _Unported("importers")
occ_impl = _Unported("occ_impl")


# ----------------------------------------------------------------------------------------------
# Workplane

class _ObjectsList:
    """`wp.objects` / `wp.vals()`: only what needs no geometry is allowed."""

    def __init__(self, wp):
        self._wp = wp

    def __len__(self):
        raise Dynamic("number of objects on the stack")

    def __iter__(self):
        raise Dynamic("iterating the objects on the stack")

    def __getitem__(self, i):
        raise Dynamic("indexing the objects on the stack")

    def __bool__(self):
        raise Dynamic("truth of the stack")


class Workplane:
    """Each call makes a new recorded object, except those CadQuery itself returns `self` from."""

    def __init__(self, inPlane="XY", origin=(0, 0, 0), obj=None):
        if isinstance(inPlane, str):
            plane = Plane.named(inPlane, origin)
        elif isinstance(inPlane, Plane):
            plane = inPlane  # CadQuery copies it; the tape holds its value now
        elif isinstance(inPlane, Location):
            raise Unsupported("Workplane(Location)")
        else:
            raise ValueError("Provided value {} is not a valid work plane".format(inPlane))
        TAPE.nwp += 1
        self._id = TAPE.nwp
        TAPE.add({"op": "Workplane", "out": {"wp": self._id}, "plane": plane._enc(),
                  "obj": _enc(obj) if obj is not None else None})

    @classmethod
    def _make(cls, entry):
        self = cls.__new__(cls)
        TAPE.nwp += 1
        self._id = TAPE.nwp
        entry["out"] = {"wp": self._id}
        TAPE.add(entry)
        return self

    def _call(self, op, args, kwargs):
        e = {"op": op, "self": {"wp": self._id}, "args": [_enc(a, f"argument of {op}") for a in args],
             "kwargs": {k: _enc(v, f"argument {k} of {op}") for k, v in kwargs.items()}}
        if op in _RETURNS_SELF:
            e["out"] = {"wp": self._id}
            TAPE.add(e)
            return self
        return Workplane._make(e)

    def __getattr__(self, name):
        if name.startswith("_"):
            raise AttributeError(name)
        if name in ("objects",):
            return _ObjectsList(self)
        if name in ("plane", "ctx", "parent"):
            raise Dynamic(f"Workplane.{name} read in Python")
        if name not in _CQ_NAMES["Workplane"]:
            raise AttributeError(f"'Workplane' object has no attribute '{name}'")
        if name in _WP_OPS:
            return lambda *a, **k: self._call(name, a, k)
        raise Unsupported(f"Workplane.{name}")

    def val(self):
        return Shape({"op": "val", "self": {"wp": self._id}})

    def vals(self):
        return _ObjectsList(self)

    def first(self):
        return self._call("first", (), {})

    def last(self):
        return self._call("last", (), {})

    def item(self, i):
        return self._call("item", (i,), {})

    def size(self):
        raise Dynamic("Workplane.size()")

    def __len__(self):
        raise Dynamic("len(Workplane)")

    def __iter__(self):
        raise Dynamic("iterating a Workplane")

    def __bool__(self):
        return True

    # operators CadQuery defines
    def __add__(self, other):
        return self._call("union", (other,), {})

    def __sub__(self, other):
        return self._call("cut", (other,), {})

    def __mul__(self, other):
        return self._call("intersect", (other,), {})

    def __or__(self, other):
        return self._call("union", (other,), {})

    def __and__(self, other):
        return self._call("intersect", (other,), {})


_RETURNS_SELF = {"add", "tag", "toPending"}

_WP_OPS = {
    # stack / plane
    "workplane", "transformed", "center", "moveTo", "move", "pushPoints", "rarray", "polarArray", "end", "tag",
    "workplaneFromTagged", "copyWorkplane", "add", "newObject", "toPending", "first", "last", "item", "clean",
    # 2D
    "rect", "circle", "polygon", "polyline", "lineTo", "line", "hLine", "vLine", "hLineTo", "vLineTo",
    "polarLine", "polarLineTo", "threePointArc", "sagittaArc", "radiusArc", "tangentArcPoint", "ellipse",
    "ellipseArc", "spline", "splineApprox", "parametricCurve", "slot2D", "close", "wire", "consolidateWires",
    "mirrorX", "mirrorY", "offset2D", "text",
    # 3D
    "extrude", "cutBlind", "cutThruAll", "hole", "cboreHole", "cskHole", "box", "sphere", "cylinder", "wedge",
    "revolve", "loft", "sweep", "twistExtrude", "union", "cut", "intersect", "combine", "fillet", "chamfer",
    "shell", "rotate", "rotateAboutCenter", "translate", "mirror", "split", "combineSolids",
    # selection
    "faces", "edges", "vertices", "solids", "wires", "shells", "compounds",
    # needs callables (recorded only to fail with a clear reason)
    "each", "eachpoint", "cutEach", "filter", "map", "apply", "invoke", "sort",
}
