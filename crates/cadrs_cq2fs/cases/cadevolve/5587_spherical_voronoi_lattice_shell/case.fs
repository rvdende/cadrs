function(context is Context, queries) {
// cqrt: CadQuery 2.8's Workplane semantics, ported to FeatureScript so Parasolid can run CADEvolve
// generators. Each function names the cq.py / selectors.py method it follows; read them side by side.
//
// This file is the body of a lambda (Onshape's evalFeatureScript takes one function), so every
// helper is a const function value; they take the context `c` and the state box `st` explicitly.
//
// Conventions
// - Lengths are plain numbers in millimetres; units are attached only when calling Onshape (`* MM`).
// - Bodies are never modified in place: an operation copies its inputs (opPattern) and works on the
//   copies, so every shape value (a query) and every selected face or edge stays valid, as CadQuery's
//   values are immutable.
// - Stack objects: {"t": "vec", "p"} a point; {"t": "loc", "T"} a Location (local, as CadQuery keeps
//   it); {"t": "edge", ...} / {"t": "wire", ...} edges and wires as data (world coordinates);
//   {"t": "solid", "q"} bodies; {"t": "face" | "edgeq" | "vertex", "q", "body"} topology of a body.
// - Workplanes live in st[].wps by their tape id: {"plane", "objs", "parent", "ctx"}; a context
//   (CQContext) in st[].ctx by index: {"pw" pending wires, "pe" pending edges, "fp" firstPoint, "tags"}.

const MM = millimeter;
const CQTOL = 0.0001;   // CQContext.tolerance and the selectors' tolerance
const TOL6 = 1e-6;      // cq.py's TOL

const cqErr = function(msg) { throw regenError("cq: " ~ msg); };
const cqUnsup = function(msg) { throw regenError("cq-unsupported: " ~ msg); };

const nid = function(st, prefix) {
    var s = st[];
    s.n = s.n + 1;
    st[] = s;
    return s.base + (prefix ~ s.n);   // under the case's own id (a feature's ops must be under its id)
};

const v3 = function(a) {
    if (a == undefined) cqErr("missing vector");
    if (a is map && a.vec != undefined) a = a.vec;
    if (size(a) == 2) return vector(a[0], a[1], 0);
    if (size(a) == 3) return vector(a[0], a[1], a[2]);
    cqErr("not a vector: " ~ toString(a));
};
const isNum = function(x) { return x is number; };
const unitOf = function(v) { return v / norm(v); };

// ---- points and directions through Onshape transforms (unitless mm <-> lengths)
const tP = function(T, p) { return (T * (p * MM)) / MM; };
const tD = function(T, d) { return T.linear * d; };
const lenV = function(p) { return p * MM; };
const unlen = function(p) { return vector(p[0] / MM, p[1] / MM, p[2] / MM); };

// ---- Plane (geom.py). Frame: x orthogonalised as gp_Ax3(origin, zDir, xDir) does.
const plMake = function(o, x, z) {
    var zz = normalize(z);
    var xp = x - zz * dot(x, zz);
    if (norm(xp) < 1e-12) cqErr("xDir parallel to the normal");
    return {"o": o, "x": normalize(xp), "z": zz};
};
const plY = function(p) { return cross(p.z, p.x); };
const plW = function(p, l) {            // Plane.toWorldCoords
    var v = v3(l);
    return p.o + p.x * v[0] + plY(p) * v[1] + p.z * v[2];
};
const plL = function(p, w) {            // Plane.toLocalCoords (points)
    var d = w - p.o;
    return vector(dot(d, p.x), dot(d, plY(p)), dot(d, p.z));
};
const plT = function(p) { return toWorld(coordSystem(p.o * MM, p.x, p.z)); };   // Location(plane): local -> world
const plFromTape = function(m) { return plMake(v3(m.origin), v3(m.xDir), v3(m.zDir)); };
const plRotated = function(p, r) {      // Plane.rotated: T = T1 * T2 * T3 about the plane's x, y, z
    var rv = v3(r);
    var T = rotationMatrix3d(p.x, rv[0] * degree) * rotationMatrix3d(plY(p), rv[1] * degree) * rotationMatrix3d(p.z, rv[2] * degree);
    return plMake(p.o, T * p.x, T * p.z);
};
// gp_Ax2(P, V): OCCT's X direction for a normal alone (Plane(normal=...) with xDir None)
const defaultXDir = function(n) {
    const A = n[0]; const B = n[1]; const C = n[2];
    const Aa = abs(A); const Ba = abs(B); const Ca = abs(C);
    var d;
    if (Ba <= Aa && Ba <= Ca) d = (Aa > Ca) ? vector(-C, 0, A) : vector(C, 0, -A);
    else if (Aa <= Ba && Aa <= Ca) d = (Ba > Ca) ? vector(0, -C, B) : vector(0, C, -B);
    else d = (Aa > Ba) ? vector(-B, A, 0) : vector(B, -A, 0);
    return normalize(d);
};
const locFromTape = function(m) {       // {"R": 3x3 rows, "t": [x, y, z]} -> Transform (local)
    return transform(matrix(m.R), v3(m.t) * MM);
};

// ---- state
const wpGet = function(st, i) {
    var w = st[].wps[i];
    if (w == undefined) cqErr("no workplane " ~ i);
    return w;
};
const wpPut = function(st, i, w) { var s = st[]; s.wps[i] = w; st[] = s; };
const ctxGet = function(st, w) { return st[].ctx[w.ctx]; };
const ctxPut = function(st, w, cx) { var s = st[]; s.ctx[w.ctx] = cx; st[] = s; };
const newObject = function(st, out, selfI, objs) {      // Workplane.newObject
    var w = wpGet(st, selfI);
    wpPut(st, out, {"plane": w.plane, "objs": objs, "parent": selfI, "ctx": w.ctx});
};
const shapeGet = function(st, i) {
    var s = st[].shapes[i];
    if (s == undefined) cqErr("no shape " ~ i);
    return s;
};
const logStep = function(c, st, id, op, msg) {
    var s = st[];
    s.log = append(s.log, {"id": id, "op": op, "msg": msg});
    st[] = s;
};
const warn = function(st, msg) {
    var s = st[];
    if (!isIn(msg, s.warn)) s.warn = append(s.warn, msg);
    st[] = s;
};

// ---- data edges / wires (world coordinates, mm)
const eStart = function(e) { return (e.k == "circle" || e.k == "ellipse") ? e.s : (e.k == "spline" ? e.pts[0] : e.a); };
const eEnd = function(e) { return (e.k == "circle" || e.k == "ellipse") ? e.s : (e.k == "spline" ? e.pts[size(e.pts) - 1] : e.b); };
const eLine = function(a, b) { return {"t": "edge", "k": "line", "a": a, "b": b}; };
const eArc = function(a, m, b) { return {"t": "edge", "k": "arc", "a": a, "m": m, "b": b}; };
const eXform = function(T, e) {
    if (e.k == "line") return eLine(tP(T, e.a), tP(T, e.b));
    if (e.k == "arc") return eArc(tP(T, e.a), tP(T, e.m), tP(T, e.b));
    if (e.k == "circle") return {"t": "edge", "k": "circle", "c": tP(T, e.c), "n": tD(T, e.n), "r": e.r, "s": tP(T, e.s)};
    if (e.k == "ellipse") return {"t": "edge", "k": "ellipse", "c": tP(T, e.c), "n": tD(T, e.n), "xa": tD(T, e.xa), "r1": e.r1, "r2": e.r2, "s": tP(T, e.s)};
    if (e.k == "spline") {
        var ps = [];
        for (var p in e.pts) ps = append(ps, tP(T, p));
        var ts = undefined;
        if (e.tans != undefined) { ts = []; for (var d in e.tans) ts = append(ts, d == undefined ? undefined : tD(T, d)); }
        return {"t": "edge", "k": "spline", "pts": ps, "tans": ts, "periodic": e.periodic};
    }
    cqErr("edge kind " ~ e.k);
};
const wXform = function(T, w) {
    var es = [];
    for (var e in w.edges) es = append(es, eXform(T, e));
    return {"t": "wire", "edges": es, "closed": w.closed, "constr": w.constr};
};
// circumcentre of three points, for arcs
const circum = function(a, m, b) {
    const u = m - a; const v = b - a;
    const n = cross(u, v);
    const nn = dot(n, n);
    if (nn < 1e-24) cqErr("arc through collinear points");
    return a + (cross(n, u) * dot(v, v) + cross(v, n) * dot(u, u)) / (2 * nn);
};
// length and centroid of a data edge (GProp linear properties)
const eLenCen = function(e) {
    if (e.k == "line") return {"L": norm(e.b - e.a), "C": (e.a + e.b) / 2};
    if (e.k == "circle") return {"L": 2 * PI * e.r, "C": e.c};
    if (e.k == "ellipse") {
        const h = ((e.r1 - e.r2) / (e.r1 + e.r2)) ^ 2;
        return {"L": PI * (e.r1 + e.r2) * (1 + 3 * h / (10 + sqrt(4 - 3 * h))), "C": e.c};
    }
    if (e.k == "arc") {
        const cc = circum(e.a, e.m, e.b);
        const r = norm(e.a - cc);
        const ua = unitOf(e.a - cc); const ub = unitOf(e.b - cc); const um = unitOf(e.m - cc);
        const n = unitOf(cross(e.a - cc, e.m - cc) + cross(e.m - cc, e.b - cc));
        var ang = atan2(dot(cross(ua, ub), n), dot(ua, ub)) / radian;
        if (ang <= 0) ang += 2 * PI;
        const bis = unitOf(rotationMatrix3d(n, ang / 2 * radian) * ua);
        const half = ang / 2;
        return {"L": r * ang, "C": cc + bis * (r * sin(half * radian) / half)};
    }
    if (e.k == "spline") {   // by its control polygon: only used to centre a wire made of it
        var L = 0; var acc = vector(0, 0, 0);
        for (var j = 0; j + 1 < size(e.pts); j += 1) { const l = norm(e.pts[j + 1] - e.pts[j]); L += l; acc += (e.pts[j] + e.pts[j + 1]) / 2 * l; }
        return {"L": L, "C": acc / L};
    }
    cqErr("edge kind " ~ e.k);
};
const wCenter = function(w) {
    var L = 0; var acc = vector(0, 0, 0);
    for (var e in w.edges) { const lc = eLenCen(e); L += lc.L; acc += lc.C * lc.L; }
    if (L <= 0) cqErr("empty wire");
    return acc / L;
};
// wire plane: circle/ellipse normal, else Newell's normal of the polygon through the points
const wPoints = function(w) {
    var ps = [];
    for (var e in w.edges) {
        if (e.k == "line") ps = concatenateArrays([ps, [e.a, e.b]]);
        else if (e.k == "arc") ps = concatenateArrays([ps, [e.a, e.m, e.b]]);
        else if (e.k == "circle" || e.k == "ellipse") ps = append(ps, e.c);
        else if (e.k == "spline") ps = concatenateArrays([ps, e.pts]);
    }
    return ps;
};
const wPlane = function(w) {
    for (var e in w.edges) if (e.k == "circle" || e.k == "ellipse") return plMake(e.c, e.k == "ellipse" ? e.xa : defaultXDir(unitOf(e.n)), e.n);
    const ps = wPoints(w);
    var n = vector(0, 0, 0);
    for (var i = 0; i < size(ps); i += 1) {
        const p = ps[i]; const q = ps[(i + 1) % size(ps)];
        n += vector((p[1] - q[1]) * (p[2] + q[2]), (p[2] - q[2]) * (p[0] + q[0]), (p[0] - q[0]) * (p[1] + q[1]));
    }
    if (norm(n) < 1e-12) cqErr("Cannot build face(s): wires not planar");
    var cen = vector(0, 0, 0);
    for (var p in ps) cen += p;
    cen = cen / size(ps);
    return plMake(cen, defaultXDir(unitOf(n)), n);
};

// ---- topology helpers (Parasolid answers)
const ents = function(c, q) { return evaluateQuery(c, q); };
const entObj = function(t, q, body) { return {"t": t, "q": q, "body": body}; };
const centroid = function(c, q) { return unlen(evApproximateCentroid(c, {"entities": q})); };
const objCenter = function(c, o) {      // Shape.Center() / Vector.Center()
    if (o.t == "vec") return o.p;
    if (o.t == "vertex") return unlen(evVertexPoint(c, {"vertex": o.q}));
    if (o.t == "face" || o.t == "edgeq" || o.t == "solid") return centroid(c, o.q);
    if (o.t == "wire") return wCenter(o);
    if (o.t == "edge") return eLenCen(o).C;
    cqErr("Center() of " ~ o.t);
};
const faceGeom = function(c, q) {       // Face.geomType()
    const s = evSurfaceDefinition(c, {"face": q});
    if (s is Plane) return "PLANE";
    if (s is Cylinder) return "CYLINDER";
    if (s is Cone) return "CONE";
    if (s is Sphere) return "SPHERE";
    if (s is Torus) return "TORUS";
    if (s is BSplineSurface) return "BSPLINE";
    return "OTHER";
};
const edgeGeom = function(c, q) {       // Edge.geomType()
    const d = evCurveDefinition(c, {"edge": q});
    if (d is Line) return "LINE";
    if (d is Circle) return "CIRCLE";
    if (d is Ellipse) return "ELLIPSE";
    if (d is BSplineCurve) return "BSPLINE";
    return "OTHER";
};
const geomType = function(c, o) {
    if (o.t == "face") return faceGeom(c, o.q);
    if (o.t == "edgeq") return edgeGeom(c, o.q);
    if (o.t == "edge") return o.k == "line" ? "LINE" : (o.k == "ellipse" ? "ELLIPSE" : "CIRCLE");
    return "OTHER";
};
const faceNormalAt = function(c, q, p) {   // Face.normalAt(point): outward for a solid's faces
    if (faceGeom(c, q) == "PLANE") return evPlane(c, {"face": q}).normal;
    const d = evDistance(c, {"side0": p * MM, "side1": q});
    return evFaceTangentPlane(c, {"face": q, "parameter": d.sides[1].parameter}).normal;
};
const faceNormal0 = function(c, q) {    // Face.normalAt() with no point: at the middle of its UV bounds
    if (faceGeom(c, q) == "PLANE") return evPlane(c, {"face": q}).normal;
    return evFaceTangentPlane(c, {"face": q, "parameter": vector(0.5, 0.5)}).normal;
};
const edgeTangent = function(c, q) {    // Edge.tangentAt() (locationParam 0.5)
    return evEdgeTangentLine(c, {"edge": q, "parameter": 0.5, "arcLengthParameterization": true}).direction;
};
const bodiesOf = function(c, o) {       // owning bodies of a shape object
    if (o.t == "solid") return o.q;
    if (o.body != undefined) return o.body;
    cqErr("no body for " ~ o.t);
};

// _collectProperty: the unique sub-shapes of every object on the stack, in stack order
const collect = function(c, objs, kind) {
    var cand = [];    // [type, query, body] in stack order, duplicates removed below
    for (var o in objs) {
        if (o.t == "solid") {
            for (var b in ents(c, o.q)) {
                if (kind == "Solids") cand = append(cand, ["solid", b, b]);
                if (kind == "Faces") for (var f in ents(c, qOwnedByBody(b, EntityType.FACE))) cand = append(cand, ["face", f, b]);
                if (kind == "Edges") for (var e in ents(c, qOwnedByBody(b, EntityType.EDGE))) cand = append(cand, ["edgeq", e, b]);
                if (kind == "Vertices") for (var v in ents(c, qOwnedByBody(b, EntityType.VERTEX))) cand = append(cand, ["vertex", v, b]);
            }
        } else if (o.t == "face") {
            if (kind == "Faces") cand = append(cand, ["face", o.q, o.body]);
            if (kind == "Edges") for (var e in ents(c, qAdjacent(o.q, AdjacencyType.EDGE, EntityType.EDGE))) cand = append(cand, ["edgeq", e, o.body]);
            if (kind == "Vertices") for (var v in ents(c, qAdjacent(o.q, AdjacencyType.VERTEX, EntityType.VERTEX))) cand = append(cand, ["vertex", v, o.body]);
        } else if (o.t == "edgeq") {
            if (kind == "Edges") cand = append(cand, ["edgeq", o.q, o.body]);
            if (kind == "Vertices") for (var v in ents(c, qAdjacent(o.q, AdjacencyType.VERTEX, EntityType.VERTEX))) cand = append(cand, ["vertex", v, o.body]);
        } else if (o.t == "vertex") {
            if (kind == "Vertices") cand = append(cand, ["vertex", o.q, o.body]);
        } else if (o.t == "wire" || o.t == "edge") {
            if (kind == "Edges" || kind == "Vertices" || kind == "Wires") cqUnsup("selecting " ~ kind ~ " of pending wires");
        }
    }
    var out = [];
    var seen = {};
    for (var x in cand) {
        const k = toString(x[1]);
        if (seen[k] != true) {
            seen[k] = true;
            out = append(out, entObj(x[0], x[1], x[2]));
        }
    }
    return out;
};

// ---- selectors (selectors.py). `sel` is the compiled form cq2fs emits.
const objKey = function(o) { return toString(o.q); };
const selFilter = function(self, c, sel, objs) {
    const k = sel.k;
    if (k == "and" || k == "or" || k == "exc") {
        const a = self(self, c, sel.a, objs);
        const b = self(self, c, sel.b, objs);
        var inB = {};
        for (var o in b) inB[objKey(o)] = true;
        var inA = {};
        for (var o in a) inA[objKey(o)] = true;
        var res = [];
        if (k == "and") { for (var o in a) if (inB[objKey(o)] == true) res = append(res, o); }
        else if (k == "exc") { for (var o in a) if (inB[objKey(o)] != true) res = append(res, o); }
        else {
            res = a;
            for (var o in b) if (inA[objKey(o)] != true) { res = append(res, o); inA[objKey(o)] = true; }
        }
        return res;
    }
    if (k == "not") {
        const na = self(self, c, sel.a, objs);
        var inN = {};
        for (var o in na) inN[objKey(o)] = true;
        var rn = [];
        for (var o in objs) if (inN[objKey(o)] != true) rn = append(rn, o);
        return rn;
    }
    if (k == "type") {
        var r = [];
        for (var o in objs) if (geomType(c, o) == sel.ty) r = append(r, o);
        return r;
    }
    if (k == "dir" || k == "par" || k == "perp") {     // BaseDirSelector subclasses
        const d = v3(sel.v);
        var r = [];
        for (var o in objs) {
            var tv;
            if (o.t == "face" && faceGeom(c, o.q) == "PLANE") tv = faceNormal0(c, o.q);
            else if (o.t == "edgeq" && edgeGeom(c, o.q) == "LINE") tv = edgeTangent(c, o.q);
            else continue;
            var ok;
            if (k == "par") ok = norm(cross(d, tv)) < CQTOL;
            else {
                const ang = acos(max(-1, min(1, dot(d, tv) / (norm(d) * norm(tv))))) / radian;
                ok = (k == "dir") ? ang < CQTOL : abs(ang - PI / 2) < CQTOL;
            }
            if (ok) r = append(r, o);
        }
        return r;
    }
    if (k == "nth") {     // _NthSelector.filter / cluster; key "center" (CenterNth), "radius", "length", "area"
        var pool = objs;
        if (sel.parallel == true) pool = self(self, c, {"k": "par", "v": sel.v}, objs);
        if (size(pool) == 0) cqErr("Can not return the Nth element of an empty list");
        var keyed = [];
        for (var o in pool) {
            var key;
            if (sel.key == "center") key = dot(objCenter(c, o), v3(sel.v));
            else if (sel.key == "radius") {
                if (o.t != "edgeq") continue;
                const cd = evCurveDefinition(c, {"edge": o.q});
                if (!(cd is Circle)) continue;
                key = cd.radius / MM;
            } else if (sel.key == "length") {
                if (o.t != "edgeq") cqErr("LengthNthSelector supports only Edges and Wires");
                key = evLength(c, {"entities": o.q}) / MM;
            } else if (sel.key == "area") {
                if (o.t != "face" && o.t != "solid") cqErr("AreaNthSelector: unsupported object");
                key = evArea(c, {"entities": o.t == "face" ? o.q : qOwnedByBody(o.q, EntityType.FACE)}) / (MM * MM);
            }
            keyed = append(keyed, {"key": key, "o": o, "i": size(keyed)});
        }
        if (size(keyed) == 0) cqErr("Can not return the Nth element of an empty list");
        keyed = sort(keyed, function(a, b) { return (a.key != b.key) ? a.key - b.key : a.i - b.i; });
        var clusters = [[keyed[0].o]];
        var start = keyed[0].key;
        for (var j = 1; j < size(keyed); j += 1) {
            if (abs(keyed[j].key - start) <= CQTOL) clusters[size(clusters) - 1] = append(clusters[size(clusters) - 1], keyed[j].o);
            else { clusters = append(clusters, [keyed[j].o]); start = keyed[j].key; }
        }
        if (sel.max != true) {
            var rev = [];
            for (var j = size(clusters) - 1; j >= 0; j -= 1) rev = append(rev, clusters[j]);
            clusters = rev;
        }
        var n = sel.n;
        if (n < 0) n += size(clusters);
        if (n < 0 || n >= size(clusters)) cqErr("Attempted to access index " ~ sel.n ~ " of a list with length " ~ size(clusters));
        return clusters[n];
    }
    cqUnsup("selector " ~ k);
};
const select = function(c, st, w, kind, sel, tagW) {    // Workplane._selectObjects
    const src = (tagW == undefined) ? w : tagW;
    const objs = collect(c, src.objs, kind);
    if (sel == undefined) return objs;
    return selFilter(selFilter, c, sel, objs);
};

// ---- _findType((Solid,), searchStack, searchParents): the solids as one query, or undefined
const findSolidQ = function(st, i, searchStack, searchParents) {
    var cur = i;
    var first = true;
    while (cur != undefined) {
        const w = wpGet(st, cur);
        if (searchStack || !first) {
            var qs = [];
            for (var o in w.objs) if (o.t == "solid") qs = append(qs, o.q);
            if (size(qs) > 0) return qUnion(qs);
        }
        first = false;
        if (!searchParents) break;
        cur = w.parent;
    }
    return undefined;
};
const findSolid = function(st, i) {
    const q = findSolidQ(st, i, true, true);
    if (q == undefined) cqErr("Cannot find a solid on the stack or in the parent chain");
    return q;
};

// ---- body operations, always on copies
const copyBodies = function(c, st, q, T) {
    if (size(ents(c, q)) == 0) cqErr("no bodies to copy");
    const id = nid(st, "cp");
    opPattern(c, id, {"entities": q, "transforms": [T == undefined ? identityTransform() : T], "instanceNames": ["i"]});
    return qCreatedBy(id, EntityType.BODY);
};
// copy `base` with its selected entities tagged, so the copies' entities can be found
const copyWithEntities = function(c, st, base, entsQ) {
    const tagv = "s" ~ st[].n;
    var all = [];
    for (var e in entsQ) all = append(all, e.q);
    if (size(all) > 0) setAttribute(c, {"entities": qUnion(all), "name": "cqsel", "attribute": tagv});
    const b = copyBodies(c, st, base, undefined);
    return {"base": b, "tag": tagv};
};
const taggedIn = function(body, kind, tagv) {
    return qHasAttributeWithValue(qOwnedByBody(body, kind), "cqsel", tagv);
};
const boolOp = function(c, st, kind, baseQ, toolQ) {
    const b = copyBodies(c, st, baseQ, undefined);
    if (toolQ == undefined || size(ents(c, toolQ)) == 0) {
        if (kind == "union") return b;
        if (kind == "cut") return b;
        cqErr("intersect with nothing");
    }
    const t = copyBodies(c, st, toolQ, undefined);
    if (kind == "union") {
        opBoolean(c, nid(st, "bo"), {"tools": qUnion([b, t]), "operationType": BooleanOperationType.UNION});
        return qUnion([b, t]);
    }
    if (kind == "cut") {
        opBoolean(c, nid(st, "bo"), {"targets": b, "tools": t, "operationType": BooleanOperationType.SUBTRACTION});
        return b;
    }
    if (kind == "intersect") {
        // the tools are consumed, as SUBTRACTION's are
        opBoolean(c, nid(st, "bo"), {"targets": b, "tools": t, "operationType": BooleanOperationType.SUBTRACT_COMPLEMENT});
        return b;
    }
    cqErr("boolean " ~ kind);
};
// Compound.fuse() of a compound with itself: merge its overlapping solids
const selfFuse = function(c, st, q) {
    if (size(ents(c, q)) < 2) return q;
    const b = copyBodies(c, st, q, undefined);
    opBoolean(c, nid(st, "bo"), {"tools": b, "operationType": BooleanOperationType.UNION});
    return b;
};
const solidObj = function(q) { return {"t": "solid", "q": q}; };
const isShapeObj = function(o) { return o.t == "solid" || o.t == "face" || o.t == "edgeq" || o.t == "vertex" || o.t == "wire" || o.t == "edge"; };

// ---- faces from pending wires (wiresToFaces) as sketch regions; or selected faces
// Edge.makeSpline through the points (GeomAPI_Interpolate in CadQuery): Onshape's fit spline
// through the same points, with the end tangents where given. Both interpolate, but OCCT's
// parametrization may not be Onshape's: the result carries a warning.
const cqSkSpline = function(sk, nm, e, to2, P, st) {
    warn(st, "spline: Onshape's interpolation through the points, not OCCT's GeomAPI_Interpolate");
    var pts = [];
    for (var p in e.pts) pts = append(pts, to2(p));
    var def = {"points": pts};
    if (e.periodic == true) def.closed = true;
    if (e.tans != undefined && size(e.tans) >= 2) {
        const d2 = function(d) { const l = plL(P, P.o + d); return vector(l[0], l[1]) * MM; };
        if (e.tans[0] != undefined) def.startDerivative = d2(e.tans[0]);
        if (e.tans[size(e.tans) - 1] != undefined) def.endDerivative = d2(e.tans[size(e.tans) - 1]);
        if (size(e.tans) > 2) warn(st, "spline: tangents at inner points not given to Onshape");
    }
    skFitSpline(sk, nm, def);
};

const sketchRegions = function(c, st, wires) {
    const P = wPlane(wires[0]);
    const scale = 1;
    for (var w in wires) {
        if (!w.closed) cqErr("Cannot build face(s): wire is not closed");
        for (var p in wPoints(w)) if (abs(dot(p - P.o, P.z)) > 1e-6 * max(1, norm(p))) cqErr("Cannot build face(s): wires not planar");
        for (var e in w.edges) if (e.k == "circle" || e.k == "ellipse") if (norm(cross(unitOf(e.n), P.z)) > 1e-9) cqErr("Cannot build face(s): wires not planar");
    }
    const id = nid(st, "sk");
    var sk = newSketchOnPlane(c, id, {"sketchPlane": plane(P.o * MM, P.z, P.x)});
    const to2 = function(p) { const l = plL(P, p); return vector(l[0], l[1]) * MM; };
    var j = 0;
    for (var w in wires) {
        for (var e in w.edges) {
            j += 1;
            const nm = "e" ~ j;
            if (e.k == "line") {
                if (norm(e.b - e.a) > 1e-9) skLineSegment(sk, nm, {"start": to2(e.a), "end": to2(e.b)});
            } else if (e.k == "arc") skArc(sk, nm, {"start": to2(e.a), "mid": to2(e.m), "end": to2(e.b)});
            else if (e.k == "circle") skCircle(sk, nm, {"center": to2(e.c), "radius": e.r * MM});
            else if (e.k == "ellipse") {
                const xa = plL(P, P.o + e.xa);
                var major = vector(xa[0], xa[1]);
                var r1 = e.r1; var r2 = e.r2;
                if (r2 > r1) { major = vector(-major[1], major[0]); r1 = e.r2; r2 = e.r1; }
                skEllipse(sk, nm, {"center": to2(e.c), "majorRadius": r1 * MM, "minorRadius": r2 * MM, "majorAxis": normalize(major)});
            } else if (e.k == "spline") cqSkSpline(sk, nm, e, to2, P, st);
        }
    }
    skSolve(sk);
    const regions = qSketchRegion(id, true);
    const n = size(ents(c, regions));
    if (n == 0) cqErr("Cannot build face(s): no closed region");
    // a wire inside another: its region a hole in the other's face (an inner wire)
    return {"q": regions, "inner": size(ents(c, qSketchRegion(id, false))) > n};
};
const getFaces = function(c, st, selfI) {     // Workplane._getFaces
    const w = wpGet(st, selfI);
    var fq = [];
    for (var o in w.objs) if (o.t == "face") fq = append(fq, o.q);
    if (size(fq) > 0) {
        warn(st, "faces on the stack extruded: inner wires not checked for a taper");
        return {"q": qUnion(fq), "inner": false};
    }
    var cx = ctxGet(st, w);
    if (size(cx.pw) == 0) cqErr("No pending wires present");
    const wires = cx.pw;
    cx.pw = [];
    ctxPut(st, w, cx);
    return sketchRegions(c, st, wires);
};

// ---- eachpoint's placements (useLocalCoordinates=True): one world Transform per stack object
const placements = function(c, w) {
    const L = plT(w.plane);
    if (size(w.objs) == 0) return [L];
    var out = [];
    for (var o in w.objs) {
        if (o.t == "loc") out = append(out, L * o.T);
        else out = append(out, plT(plMake(objCenter(c, o), w.plane.x, w.plane.z)));
    }
    return out;
};
const addPendingWires = function(st, w, wires) {
    var cx = ctxGet(st, w);
    for (var x in wires) if (x.constr != true) cx.pw = append(cx.pw, x);
    ctxPut(st, w, cx);
};
const addPendingEdge = function(st, w, e) {    // Workplane._addPendingEdge
    var cx = ctxGet(st, w);
    cx.pe = append(cx.pe, e);
    if (cx.fp == undefined) cx.fp = plL(w.plane, eStart(e));
    ctxPut(st, w, cx);
};
const fromPoint = function(c, w, useLocal) {    // Workplane._findFromPoint
    var p;
    if (size(w.objs) == 0) p = w.plane.o;
    else {
        const o = w.objs[size(w.objs) - 1];
        if (o.t == "edge") p = eEnd(o);
        else if (o.t == "vec") p = o.p;
        else if (o.t == "vertex") p = objCenter(c, o);
        else cqErr("Cannot convert object type " ~ o.t ~ " to vector.");
    }
    return useLocal ? plL(w.plane, p) : p;
};

// ---- _combineWithBase(obj, mode, clean)
const combineWithBase = function(c, st, out, selfI, q, mode) {
    if (mode == true || mode == "a") {          // _fuseWithBase
        const base = findSolidQ(st, selfI, true, true);
        if (base != undefined) newObject(st, out, selfI, [solidObj(boolOp(c, st, "union", base, q))]);
        else newObject(st, out, selfI, [solidObj(selfFuse(c, st, q))]);
    } else if (mode == "cut" || mode == "s") {  // _cutFromBase
        const base = findSolidQ(st, selfI, true, true);
        if (base != undefined) newObject(st, out, selfI, [solidObj(boolOp(c, st, "cut", base, q))]);
        else newObject(st, out, selfI, [solidObj(q)]);
    } else if (mode == false) newObject(st, out, selfI, [solidObj(q)]);
    else cqErr("combine mode " ~ toString(mode));
};

// a Workplane argument's shapes (_selectShapes(wp.vals())) or a shape value
const argShapes = function(c, st, a) {
    if (a is map && a.wp != undefined) return wpGet(st, a.wp).objs;
    if (a is map && a.shape != undefined) return [shapeGet(st, a.shape)];
    cqErr("not a shape argument: " ~ toString(a));
};
const solidsQ = function(c, objs) {
    var qs = [];
    for (var o in objs) if (o.t == "solid") qs = append(qs, o.q);
    return size(qs) == 0 ? undefined : qUnion(qs);
};

// ================================================================================================
// Workplane methods. Each: (c, st, out, selfI, a) with a = the bound arguments.

const cqWorkplaneNew = function(c, st, out, a) {      // Workplane.__init__
    var s = st[];
    const ci = size(s.ctx);
    s.ctx = append(s.ctx, {"pw": [], "pe": [], "fp": undefined, "tags": {}});
    st[] = s;
    var objs = [];
    if (a.obj != undefined) objs = argShapes(c, st, a.obj);
    wpPut(st, out, {"plane": plFromTape(a.plane), "objs": objs, "parent": undefined, "ctx": ci});
};

const cqWorkplane = function(c, st, out, selfI, a) {   // Workplane.workplane
    const w = wpGet(st, selfI);
    const computeXdir = function(normal) {
        var xd = cross(vector(0, 0, 1), normal);
        if (norm(xd) < CQTOL) xd = vector(1, 0, 0);
        return xd;
    };
    var center; var normal; var xDir;
    if (size(w.objs) > 1) {
        var fqs = [];
        for (var o in w.objs) {
            if (o.t != "face" || (faceGeom(c, o.q) != "PLANE")) cqErr("If multiple objects selected, they all must be planar faces.");
            fqs = append(fqs, o.q);
        }
        const n0 = faceNormal0(c, fqs[0]);
        const p0 = centroid(c, fqs[0]);
        for (var j = 1; j < size(fqs); j += 1) {
            const n1 = faceNormal0(c, fqs[j]);
            const p1 = centroid(c, fqs[j]);
            const dn = n0 - n1;
            if (!(abs(dn[0]) < CQTOL || abs(dn[1]) < CQTOL || abs(dn[2]) < CQTOL)) cqErr("Selected faces must be co-planar.");
            if (abs(dot(n0, p0 - p1)) >= CQTOL) cqErr("Selected faces must be co-planar.");
        }
        if (a.centerOption == "CenterOfBoundBox") {
            const bb = evBox3d(c, {"topology": qUnion(fqs), "tight": true});
            center = unlen((bb.minCorner + bb.maxCorner) / 2);
            warn(st, "CenterOfBoundBox: Onshape's tight box, CadQuery's is OCCT's (enlarged)");
        } else center = centroid(c, qUnion(fqs));
        normal = n0;
        xDir = computeXdir(normal);
    } else {
        const obj = (size(w.objs) == 0) ? {"t": "vec", "p": w.plane.o} : w.objs[0];
        if (obj.t == "face") {
            if (a.centerOption == "CenterOfBoundBox") {
                const bb = evBox3d(c, {"topology": obj.q, "tight": true});
                center = unlen((bb.minCorner + bb.maxCorner) / 2);
                warn(st, "CenterOfBoundBox: Onshape's tight box, CadQuery's is OCCT's (enlarged)");
            } else center = objCenter(c, obj);
            normal = faceNormalAt(c, obj.q, center);
            xDir = computeXdir(normal);
        } else if (obj.t == "vec" || isShapeObj(obj)) {
            if (a.centerOption == "CenterOfBoundBox" && obj.t != "vec") {
                const bb = evBox3d(c, {"topology": bodiesOf(c, obj), "tight": true});
                center = unlen((bb.minCorner + bb.maxCorner) / 2);
                warn(st, "CenterOfBoundBox: Onshape's tight box, CadQuery's is OCCT's (enlarged)");
            } else center = objCenter(c, obj);
            var pv = undefined;
            if (w.parent != undefined) {
                const pw = wpGet(st, w.parent);
                if (size(pw.objs) > 0) pv = pw.objs[0];
            }
            if (pv != undefined && pv.t == "face") {
                normal = faceNormalAt(c, pv.q, center);
                xDir = computeXdir(normal);
            } else {
                normal = w.plane.z;
                xDir = w.plane.x;
            }
        } else cqErr("Needs a face or a vertex or point on a work plane");
    }
    if (a.centerOption == "ProjectedOrigin") {
        const orig = (a.origin == undefined) ? w.plane.o : v3(a.origin);
        center = orig - normal * (dot(orig - center, normal) / dot(normal, normal));
    } else if (a.centerOption != "CenterOfMass" && a.centerOption != "CenterOfBoundBox") cqErr("Undefined centerOption value provided.");
    if (a.invert == true) normal = -normal;
    const offsetCenter = center + normalize(normal) * a.offset;
    wpPut(st, out, {"plane": plMake(offsetCenter, xDir, normal), "objs": [], "parent": selfI, "ctx": w.ctx});
};

const cqTransformed = function(c, st, out, selfI, a) {   // Workplane.transformed
    const w = wpGet(st, selfI);
    var p = plRotated(w.plane, a.rotate);
    p.o = plW(w.plane, a.offset);
    wpPut(st, out, {"plane": p, "objs": [{"t": "vec", "p": p.o}], "parent": selfI, "ctx": w.ctx});
};

const cqCenter = function(c, st, out, selfI, a) {   // Workplane.center
    const w = wpGet(st, selfI);
    const o = plW(w.plane, [a.x, a.y]);
    var p = w.plane;
    p.o = o;
    wpPut(st, out, {"plane": p, "objs": [{"t": "vec", "p": o}], "parent": selfI, "ctx": w.ctx});
};

const cqMoveTo = function(c, st, out, selfI, a) {   // Workplane.moveTo
    const w = wpGet(st, selfI);
    newObject(st, out, selfI, [{"t": "vec", "p": plW(w.plane, [a.x, a.y, 0])}]);
};

const cqMove = function(c, st, out, selfI, a) {     // Workplane.move
    const w = wpGet(st, selfI);
    const p = fromPoint(c, w, true);
    newObject(st, out, selfI, [{"t": "vec", "p": plW(w.plane, p + vector(a.xDist, a.yDist, 0))}]);
};

const pushPts = function(c, st, out, selfI, pts) {   // Workplane.pushPoints
    const w = wpGet(st, selfI);
    var objs = [];
    for (var p in pts) {
        if (p is map && p.loc != undefined) objs = append(objs, {"t": "loc", "T": locFromTape(p.loc)});
        else objs = append(objs, {"t": "vec", "p": plW(w.plane, p)});
    }
    newObject(st, out, selfI, objs);
};
const cqPushPoints = function(c, st, out, selfI, a) { pushPts(c, st, out, selfI, a.pntList); };

const cqRarray = function(c, st, out, selfI, a) {   // Workplane.rarray
    if ((a.xSpacing <= 0 && a.ySpacing <= 0) || a.xCount < 1 || a.yCount < 1) cqErr("Spacing and count must be > 0 in at least one direction");
    const cen = (a.center is boolean) ? [a.center, a.center] : a.center;
    var pts = [];
    for (var x = 0; x < a.xCount; x += 1)
        for (var y = 0; y < a.yCount; y += 1)
            pts = append(pts, [a.xSpacing * x - (cen[0] ? a.xSpacing * (a.xCount - 1) * 0.5 : 0),
                               a.ySpacing * y - (cen[1] ? a.ySpacing * (a.yCount - 1) * 0.5 : 0)]);
    pushPts(c, st, out, selfI, pts);
};

const pyRemainder = function(x, y) {   // math.remainder (IEEE): x - n*y, n the nearest integer (ties to even)
    const q = x / y;
    var n = round(q);
    if (abs(q - floor(q) - 0.5) < 1e-15) n = (floor(q) % 2 == 0) ? floor(q) : floor(q) + 1;
    return x - n * y;
};
const cqPolarArray = function(c, st, out, selfI, a) {   // Workplane.polarArray
    if (a.count < 1) cqErr("At least 1 element required");
    var angle = a.angle;
    if (a.fill) {
        if (abs(pyRemainder(angle, 360)) < TOL6) angle = angle / a.count;
        else angle = (a.count > 1) ? angle / (a.count - 1) : a.startAngle;
    }
    var locs = [];
    for (var i = 0; i < a.count; i += 1) {
        const phi = a.startAngle + angle * i;
        const x = a.radius * cos(phi * degree);
        const y = a.radius * sin(phi * degree);
        const R = a.rotate ? rotationMatrix3d(vector(0, 0, 1), phi * degree) : identityMatrix(3);
        locs = append(locs, {"loc": {"R": [[R[0][0], R[0][1], R[0][2]], [R[1][0], R[1][1], R[1][2]], [R[2][0], R[2][1], R[2][2]]], "t": [x, y, 0]}});
    }
    pushPts(c, st, out, selfI, locs);
};

// 2D: wires made at every placement (eachpoint), added to pending wires
const placeWire = function(c, st, out, selfI, localWire) {
    const w = wpGet(st, selfI);
    var objs = [];
    for (var T in placements(c, w)) objs = append(objs, wXform(T, localWire));
    addPendingWires(st, w, objs);
    newObject(st, out, selfI, objs);
};
const polyWire = function(pts, constr) {
    var es = [];
    for (var i = 0; i + 1 < size(pts); i += 1) es = append(es, eLine(pts[i], pts[i + 1]));
    return {"t": "wire", "edges": es, "closed": norm(pts[0] - pts[size(pts) - 1]) < 1e-9, "constr": constr};
};
const cqRect = function(c, st, out, selfI, a) {   // Workplane.rect
    const cen = (a.centered is boolean) ? [a.centered, a.centered] : a.centered;
    const off = vector(cen[0] ? 0 : a.xLen / 2, cen[1] ? 0 : a.yLen / 2, 0);
    const pts = [vector(-a.xLen / 2, -a.yLen / 2, 0) + off, vector(a.xLen / 2, -a.yLen / 2, 0) + off,
                 vector(a.xLen / 2, a.yLen / 2, 0) + off, vector(-a.xLen / 2, a.yLen / 2, 0) + off];
    placeWire(c, st, out, selfI, polyWire(append(pts, pts[0]), a.forConstruction));
};
const cqCircle = function(c, st, out, selfI, a) {   // Workplane.circle
    if (a.radius <= 0) cqErr("circle radius must be positive");
    placeWire(c, st, out, selfI, {"t": "wire", "closed": true, "constr": a.forConstruction,
        "edges": [{"t": "edge", "k": "circle", "c": vector(0, 0, 0), "n": vector(0, 0, 1), "r": a.radius, "s": vector(a.radius, 0, 0)}]});
};
const cqPolygon = function(c, st, out, selfI, a) {   // Workplane.polygon
    const angle = 2 * PI / a.nSides;
    var radius = a.diameter / 2;
    if (a.circumscribed) radius /= cos(angle / 2 * radian);
    var pts = [];
    for (var i = 0; i <= a.nSides; i += 1) {
        var o = angle * i;
        if (a.circumscribed) o += angle / 2;
        pts = append(pts, vector(radius * cos(o * radian), radius * sin(o * radian), 0));
    }
    placeWire(c, st, out, selfI, polyWire(pts, a.forConstruction));
};
const cqEllipse = function(c, st, out, selfI, a) {   // Workplane.ellipse (closed)
    const xa = rotationMatrix3d(vector(0, 0, 1), a.rotation_angle * degree) * vector(1, 0, 0);
    placeWire(c, st, out, selfI, {"t": "wire", "closed": true, "constr": a.forConstruction,
        "edges": [{"t": "edge", "k": "ellipse", "c": vector(0, 0, 0), "n": vector(0, 0, 1), "xa": xa, "r1": a.x_radius, "r2": a.y_radius, "s": xa * a.x_radius}]});
};

// edges drawn one at a time (pending edges)
const drawEdge = function(c, st, out, selfI, e, constr) {
    const w = wpGet(st, selfI);
    if (constr != true) addPendingEdge(st, w, e);
    newObject(st, out, selfI, [e]);
};
const cqLineTo = function(c, st, out, selfI, a) {    // Workplane.lineTo
    const w = wpGet(st, selfI);
    drawEdge(c, st, out, selfI, eLine(fromPoint(c, w, false), plW(w.plane, [a.x, a.y])), a.forConstruction);
};
const cqLine = function(c, st, out, selfI, a) {      // Workplane.line
    const w = wpGet(st, selfI);
    const p = fromPoint(c, w, true);
    cqLineTo(c, st, out, selfI, {"x": a.xDist + p[0], "y": a.yDist + p[1], "forConstruction": a.forConstruction});
};
const cqHLine = function(c, st, out, selfI, a) { cqLine(c, st, out, selfI, {"xDist": a.distance, "yDist": 0, "forConstruction": a.forConstruction}); };
const cqVLine = function(c, st, out, selfI, a) { cqLine(c, st, out, selfI, {"xDist": 0, "yDist": a.distance, "forConstruction": a.forConstruction}); };
const cqHLineTo = function(c, st, out, selfI, a) {
    const p = fromPoint(c, wpGet(st, selfI), true);
    cqLineTo(c, st, out, selfI, {"x": a.xCoord, "y": p[1], "forConstruction": a.forConstruction});
};
const cqVLineTo = function(c, st, out, selfI, a) {
    const p = fromPoint(c, wpGet(st, selfI), true);
    cqLineTo(c, st, out, selfI, {"x": p[0], "y": a.yCoord, "forConstruction": a.forConstruction});
};
const cqPolarLine = function(c, st, out, selfI, a) {
    cqLine(c, st, out, selfI, {"xDist": cos(a.angle * degree) * a.distance, "yDist": sin(a.angle * degree) * a.distance, "forConstruction": a.forConstruction});
};
const cqPolarLineTo = function(c, st, out, selfI, a) {
    cqLineTo(c, st, out, selfI, {"x": cos(a.angle * degree) * a.distance, "y": sin(a.angle * degree) * a.distance, "forConstruction": a.forConstruction});
};
const cqThreePointArc = function(c, st, out, selfI, a) {   // Workplane.threePointArc
    const w = wpGet(st, selfI);
    drawEdge(c, st, out, selfI, eArc(fromPoint(c, w, false), plW(w.plane, a.point1), plW(w.plane, a.point2)), a.forConstruction);
};
const cqPolyline = function(c, st, out, selfI, a) {   // Workplane.polyline
    const w = wpGet(st, selfI);
    var start; var pts;
    if (a.includeCurrent) { start = fromPoint(c, w, false); pts = a.listOfXYTuple; }
    else {
        start = plW(w.plane, a.listOfXYTuple[0]);
        pts = subArray(a.listOfXYTuple, 1, size(a.listOfXYTuple));
    }
    var edges = [];
    for (var t in pts) {
        const e = plW(w.plane, t);
        const ed = eLine(start, e);
        edges = append(edges, ed);
        start = e;
        if (!a.forConstruction) addPendingEdge(st, w, ed);
    }
    newObject(st, out, selfI, edges);
};
const cqWire = function(c, st, out, selfI, a) {       // Workplane.wire
    const w = wpGet(st, selfI);
    var cx = ctxGet(st, w);
    if (size(cx.pe) == 0) { wpPut(st, out, w); return; }
    const edges = cx.pe;
    cx.pe = [];
    ctxPut(st, w, cx);
    // Wire.assembleEdges: the edges in order, connected end to start
    for (var i = 0; i + 1 < size(edges); i += 1)
        if (norm(eEnd(edges[i]) - eStart(edges[i + 1])) > 1e-6) cqErr("assembleEdges: edges are not connected");
    const closed = norm(eEnd(edges[size(edges) - 1]) - eStart(edges[0])) < 1e-6;
    const wire = {"t": "wire", "edges": edges, "closed": closed, "constr": a.forConstruction};
    if (!a.forConstruction) addPendingWires(st, w, [wire]);
    var others = [];
    for (var o in w.objs) if (o.t != "edge") others = append(others, o);
    newObject(st, out, selfI, append(others, wire));
};
const cqClose = function(c, st, out, selfI, a) {      // Workplane.close
    const w = wpGet(st, selfI);
    const endPoint = fromPoint(c, w, true);
    const cx = ctxGet(st, w);
    if (cx.fp == undefined) cqErr("No start point specified - cannot close");
    const startPoint = cx.fp;
    if (norm(endPoint - startPoint) > 1e-6) {
        // self.polyline([endPoint, startPoint]): its result is discarded, its edge stays pending
        addPendingEdge(st, w, eLine(plW(w.plane, endPoint), plW(w.plane, startPoint)));
    }
    var cx2 = ctxGet(st, w);
    cx2.fp = undefined;
    ctxPut(st, w, cx2);
    cqWire(c, st, out, selfI, {"forConstruction": false});
};

// 3D
// Solid.extrudeLinear's taper (LocOpe_DPrism, its height divided by cos(taper) so the solid is
// `dist` tall): every side face leans in by `taper` degrees from the extrusion's direction,
// about the face it starts from; out for a negative taper. Onshape's extrude does the same with
// opDraft about its start cap (std extrude.fs, extrudeWithDraft).
const taperDraft = function(c, st, id, dir, taper) {
    for (var b in ents(c, qCreatedBy(id, EntityType.BODY))) {
        const start = qOwnedByBody(qCapEntity(id, CapType.START, EntityType.FACE), b);
        const end = qOwnedByBody(qCapEntity(id, CapType.END, EntityType.FACE), b);
        opDraft(c, nid(st, "dr"), {"draftType": DraftType.NEUTRAL_PLANE, "neutralPlane": start,
                                   "draftFaces": qSubtraction(qOwnedByBody(b, EntityType.FACE), qUnion([start, end])),
                                   "pullVec": dir * (taper > 0 ? 1 : -1), "angle": abs(taper) * degree,
                                   "tangentPropagation": false, "reFillet": false});
    }
};
const extrudeRaw = function(c, st, selfI, dist, both, taper) {   // Workplane._extrude (distance)
    const w = wpGet(st, selfI);
    const faces = getFaces(c, st, selfI);
    if (abs(dist) < 1e-12) cqErr("extrude distance is zero");
    const t = (taper == undefined) ? 0 : taper;
    const dir = w.plane.z * (dist > 0 ? 1 : -1);
    if (t == 0) {
        const id = nid(st, "ex");
        var def = {"entities": faces.q, "direction": dir, "endBound": BoundingType.BLIND, "endDepth": abs(dist) * MM};
        if (both) { def.startBound = BoundingType.BLIND; def.startDepth = abs(dist) * MM; }
        opExtrude(c, id, def);
        return qCreatedBy(id, EntityType.BODY);
    }
    if (faces.inner) cqErr("Inner wires not allowed with tapered extrusion");
    // each way its own tapered extrusion (both: s1 fused with s2)
    var qs = [];
    for (var d in (both ? [dir, -dir] : [dir])) {
        const id = nid(st, "ex");
        opExtrude(c, id, {"entities": faces.q, "direction": d, "endBound": BoundingType.BLIND, "endDepth": abs(dist) * MM});
        taperDraft(c, st, id, d, t);
        qs = append(qs, qCreatedBy(id, EntityType.BODY));
    }
    if (size(qs) > 1) opBoolean(c, nid(st, "bo"), {"tools": qUnion(qs), "operationType": BooleanOperationType.UNION});
    return qUnion(qs);
};
const cqCutBlind = function(c, st, out, selfI, a) {   // Workplane.cutBlind
    if (!(a.until is number)) cqUnsup("cutBlind until " ~ toString(a.until));
    const toCut = extrudeRaw(c, st, selfI, a.until, a.both == true, a.taper);
    const base = findSolid(st, selfI);
    newObject(st, out, selfI, [solidObj(boolOp(c, st, "cut", base, toCut))]);
};
const cqExtrude = function(c, st, out, selfI, a) {    // Workplane.extrude
    if (a.combine == "cut" || a.combine == "s") return cqCutBlind(c, st, out, selfI, {"until": a.until, "both": a.both, "taper": a.taper});
    if (!(a.until is number)) cqUnsup("extrude until " ~ toString(a.until));
    const r = extrudeRaw(c, st, selfI, a.until, a.both == true, a.taper);
    combineWithBase(c, st, out, selfI, r, a.combine);
};
const cqCutThruAll = function(c, st, out, selfI, a) {   // Workplane.cutThruAll: both directions
    if (a.taper != 0) cqUnsup("cutThruAll with a taper");
    const w = wpGet(st, selfI);
    const base = findSolid(st, selfI);
    const faces = getFaces(c, st, selfI).q;
    const bb = evBox3d(c, {"topology": qUnion([base, faces]), "tight": false});
    const depth = norm(bb.maxCorner - bb.minCorner) / MM * 2 + 1;
    const id = nid(st, "ex");
    opExtrude(c, id, {"entities": faces, "direction": w.plane.z, "endBound": BoundingType.BLIND, "endDepth": depth * MM,
                      "startBound": BoundingType.BLIND, "startDepth": depth * MM});
    newObject(st, out, selfI, [solidObj(boolOp(c, st, "cut", base, qCreatedBy(id, EntityType.BODY)))]);
};
const largestDimension = function(c, st, selfI) {   // findSolid().BoundingBox().DiagonalLength
    const bb = evBox3d(c, {"topology": findSolid(st, selfI), "tight": false});
    return norm(bb.maxCorner - bb.minCorner) / MM;
};
// a cylinder (radius r, along local -Z from local z0 to z0 - h) placed by T
const cylAlongNegZ = function(c, st, T, r, z0, h) {
    const id = nid(st, "cy");
    fCylinder(c, id, {"bottomCenter": T * (vector(0, 0, z0 - h) * MM), "topCenter": T * (vector(0, 0, z0) * MM), "radius": r * MM});
    return qCreatedBy(id, EntityType.BODY);
};
const cutEach = function(c, st, out, selfI, toolsAt) {   // Workplane.cutEach
    const w = wpGet(st, selfI);
    const base = findSolid(st, selfI);
    var tools = [];
    for (var T in placements(c, w)) tools = append(tools, toolsAt(T));
    newObject(st, out, selfI, [solidObj(boolOp(c, st, "cut", base, qUnion(tools)))]);
};
const cqHole = function(c, st, out, selfI, a) {     // Workplane.hole
    const depth = (a.depth == undefined) ? largestDimension(c, st, selfI) : a.depth;
    cutEach(c, st, out, selfI, function(T) { return cylAlongNegZ(c, st, T, a.diameter / 2, 0, depth); });
};
const cqCboreHole = function(c, st, out, selfI, a) {   // Workplane.cboreHole
    const depth = (a.depth == undefined) ? largestDimension(c, st, selfI) : a.depth;
    cutEach(c, st, out, selfI, function(T) {
        return qUnion([cylAlongNegZ(c, st, T, a.diameter / 2, 0, depth), cylAlongNegZ(c, st, T, a.cboreDiameter / 2, 0, a.cboreDepth)]);
    });
};
const cqCskHole = function(c, st, out, selfI, a) {     // Workplane.cskHole
    const depth = (a.depth == undefined) ? largestDimension(c, st, selfI) : a.depth;
    const r = a.cskDiameter / 2;
    const h = r / tan(a.cskAngle / 2 * degree);
    cutEach(c, st, out, selfI, function(T) {
        const id = nid(st, "co");
        fCone(c, id, {"bottomCenter": T * (vector(0, 0, 0) * MM), "topCenter": T * (vector(0, 0, -h) * MM), "bottomRadius": r * MM, "topRadius": 0 * MM});
        return qUnion([cylAlongNegZ(c, st, T, a.diameter / 2, 0, depth), qCreatedBy(id, EntityType.BODY)]);
    });
};
const solidsAtPlacements = function(c, st, out, selfI, a, make) {   // eachpoint(..., True, combine, clean)
    const w = wpGet(st, selfI);
    var qs = [];
    for (var T in placements(c, w)) qs = append(qs, make(T));
    combineWithBase(c, st, out, selfI, qUnion(qs), a.combine);
};
const cqBox = function(c, st, out, selfI, a) {      // Workplane.box
    const cen = (a.centered is boolean) ? [a.centered, a.centered, a.centered] : a.centered;
    const off = vector(cen[0] ? -a.length / 2 : 0, cen[1] ? -a.width / 2 : 0, cen[2] ? -a.height / 2 : 0);
    if (a.length <= 0 || a.width <= 0 || a.height <= 0) cqErr("box with a non-positive dimension");
    solidsAtPlacements(c, st, out, selfI, a, function(T) {
        const id = nid(st, "bx");
        fCuboid(c, id, {"corner1": off * MM, "corner2": (off + vector(a.length, a.width, a.height)) * MM});
        opTransform(c, nid(st, "tr"), {"bodies": qCreatedBy(id, EntityType.BODY), "transform": T});
        return qCreatedBy(id, EntityType.BODY);
    });
};
const cqSphere = function(c, st, out, selfI, a) {   // Workplane.sphere (whole spheres)
    if (a.angle1 != -90 || a.angle2 != 90 || a.angle3 != 360) cqUnsup("partial sphere");
    const cen = (a.centered is boolean) ? [a.centered, a.centered, a.centered] : a.centered;
    const off = vector(cen[0] ? 0 : a.radius, cen[1] ? 0 : a.radius, cen[2] ? 0 : a.radius);
    solidsAtPlacements(c, st, out, selfI, a, function(T) {
        const id = nid(st, "sp");
        opSphere(c, id, {"center": T * (off * MM), "radius": a.radius * MM});
        return qCreatedBy(id, EntityType.BODY);
    });
};
const cqCylinder = function(c, st, out, selfI, a) {   // Workplane.cylinder (whole cylinders)
    if (a.angle != 360) cqUnsup("partial cylinder");
    const cen = (a.centered is boolean) ? [a.centered, a.centered, a.centered] : a.centered;
    const off = vector(cen[0] ? 0 : a.radius, cen[1] ? 0 : a.radius, cen[2] ? -a.height / 2 : 0);
    const d = normalize(v3(a.direct));
    const R = toWorld(coordSystem(vector(0, 0, 0) * MM, defaultXDir(d), d));   // Plane(Vector(), normal=direct).location
    solidsAtPlacements(c, st, out, selfI, a, function(T) {
        const id = nid(st, "cy");
        fCylinder(c, id, {"bottomCenter": (T * R) * (off * MM), "topCenter": (T * R) * ((off + vector(0, 0, a.height)) * MM), "radius": a.radius * MM});
        return qCreatedBy(id, EntityType.BODY);
    });
};
const cqRevolve = function(c, st, out, selfI, a) {   // Workplane.revolve
    const w = wpGet(st, selfI);
    var angle = a.angleDegrees % 360;
    if (angle < 0) angle += 360;
    if (angle == 0) angle = 360;
    const s = (a.axisStart == undefined) ? plW(w.plane, [0, 0]) : plW(w.plane, a.axisStart);
    var e;
    if (a.axisEnd == undefined) e = (s[1] != 0) ? plW(w.plane, [0, s[1]]) : plW(w.plane, [0, 1]);
    else e = plW(w.plane, a.axisEnd);
    if (norm(e - s) < 1e-12) cqErr("revolve axis of zero length");
    const faces = getFaces(c, st, selfI).q;
    const id = nid(st, "rv");
    opRevolve(c, id, {"entities": faces, "axis": line(s * MM, normalize(e - s)), "angleForward": (angle == 360 ? 2 * PI : angle * PI / 180) * radian});
    combineWithBase(c, st, out, selfI, qCreatedBy(id, EntityType.BODY), a.combine);
};

// Workplane.loft: each pending wire one section, in order (or the outer wires of the faces on
// the stack); OCCT's ThruSections in CadQuery. Two sections make a ruled loft whatever the
// kernel; so does `ruled` between each pair. A smooth loft through three or more is each
// kernel's own surface: the result carries a warning saying so.
const cqLoft = function(c, st, out, selfI, a) {
    const w = wpGet(st, selfI);
    var cx = ctxGet(st, w);
    var profiles = [];
    if (size(cx.pw) > 0) {
        const wires = cx.pw;
        cx.pw = [];
        ctxPut(st, w, cx);
        for (var x in wires) {
            const f = sketchRegions(c, st, [x]);
            if (size(ents(c, f.q)) != 1) cqErr("loft: a section that isn't one closed wire");
            profiles = append(profiles, f.q);
        }
    } else {
        for (var o in w.objs) {
            if (o.t == "face") profiles = append(profiles, o.q);
            else if (o.t == "vertex") cqUnsup("loft to a vertex");
        }
    }
    if (size(profiles) == 0) cqErr("Nothing to loft");
    if (size(profiles) == 1) cqErr("More than one wire or face is required");
    var q;
    if (a.ruled && size(profiles) > 2) {
        var parts = [];
        for (var j = 0; j + 1 < size(profiles); j += 1) {
            const id = nid(st, "lo");
            opLoft(c, id, {"profileSubqueries": [profiles[j], profiles[j + 1]], "bodyType": ToolBodyType.SOLID});
            parts = append(parts, qCreatedBy(id, EntityType.BODY));
        }
        opBoolean(c, nid(st, "bo"), {"tools": qUnion(parts), "operationType": BooleanOperationType.UNION});
        q = qUnion(parts);
    } else {
        if (size(profiles) > 2) warn(st, "smooth loft through " ~ size(profiles) ~ " sections: Parasolid's surface, not OCCT's ThruSections");
        const id = nid(st, "lo");
        opLoft(c, id, {"profileSubqueries": profiles, "bodyType": ToolBodyType.SOLID});
        q = qCreatedBy(id, EntityType.BODY);
    }
    combineWithBase(c, st, out, selfI, q, a.combine);
};

const cqNewObject = function(c, st, out, selfI, a) {   // Workplane.newObject(objlist)
    var objs = [];
    for (var x in a.objlist) objs = concatenateArrays([objs, argShapes(c, st, x)]);
    newObject(st, out, selfI, objs);
};

const cqSlot2D = function(c, st, out, selfI, a) {   // Workplane.slot2D
    const r = a.diameter / 2;
    const p1 = vector(-a.length / 2 + r, a.diameter / 2, 0);
    const p2 = p1 + vector(a.length - a.diameter, 0, 0);
    const p3 = p1 + vector(a.length - a.diameter, -a.diameter, 0);
    const p4 = p1 + vector(0, -a.diameter, 0);
    const arc1 = p2 + vector(r, -r, 0);
    const arc2 = p4 + vector(-r, r, 0);
    var w = {"t": "wire", "closed": true, "constr": false, "edges": [eLine(p1, p2), eArc(p2, arc1, p3), eLine(p3, p4), eArc(p4, arc2, p1)]};
    w = wXform(transform(rotationMatrix3d(vector(0, 0, 1), a.angle * degree), vector(0, 0, 0) * MM), w);
    placeWire(c, st, out, selfI, w);
};

const cqSpline = function(c, st, out, selfI, a) {    // Workplane.spline
    const w = wpGet(st, selfI);
    var pts = [];
    if (a.includeCurrent) pts = append(pts, fromPoint(c, w, false));
    for (var p in a.listOfXYTuple) pts = append(pts, plW(w.plane, p));
    if (a.parameters != undefined) cqUnsup("spline with parameters");
    var tans = undefined;
    if (a.tangents != undefined) {
        tans = [];
        for (var t in a.tangents) tans = append(tans, t == undefined ? undefined : plW(w.plane, t) - w.plane.o);
    }
    const e = {"t": "edge", "k": "spline", "pts": pts, "tans": tans, "periodic": a.periodic};
    if (a.makeWire) {
        const wire = {"t": "wire", "edges": [e], "closed": a.periodic == true || norm(pts[0] - pts[size(pts) - 1]) < 1e-9, "constr": a.forConstruction};
        if (!a.forConstruction) addPendingWires(st, w, [wire]);
        newObject(st, out, selfI, [wire]);
    } else {
        if (!a.forConstruction) addPendingEdge(st, w, e);
        newObject(st, out, selfI, [e]);
    }
};

// A sweep's path as edges Parasolid can follow: one sketch when the path lies in a plane, else
// (straight segments only) a 3D polyline.
const pathEdges = function(c, st, p) {
    var edges = (p.t == "wire") ? p.edges : [p];
    var pts = [];
    for (var e in edges) pts = concatenateArrays([pts, (e.k == "line") ? [e.a, e.b] : (e.k == "arc" ? [e.a, e.m, e.b] : (e.k == "spline" ? e.pts : [e.c, e.s]))]);
    var planar = undefined;
    try silent { planar = wPlane({"edges": edges}); }
    if (planar == undefined && size(edges) == 1 && edges[0].k == "line") {
        const d = unitOf(edges[0].b - edges[0].a);
        planar = plMake(edges[0].a, d, defaultXDir(d));
    }
    var flat = planar != undefined;
    if (flat) for (var q in pts) if (abs(dot(q - planar.o, planar.z)) > 1e-7 * max(1, norm(q))) flat = false;
    if (flat) {
        const id = nid(st, "pk");
        var sk = newSketchOnPlane(c, id, {"sketchPlane": plane(planar.o * MM, planar.z, planar.x)});
        const to2 = function(q) { const l = plL(planar, q); return vector(l[0], l[1]) * MM; };
        var j = 0;
        for (var e in edges) {
            j += 1;
            if (e.k == "line") skLineSegment(sk, "p" ~ j, {"start": to2(e.a), "end": to2(e.b)});
            else if (e.k == "arc") skArc(sk, "p" ~ j, {"start": to2(e.a), "mid": to2(e.m), "end": to2(e.b)});
            else if (e.k == "circle") skCircle(sk, "p" ~ j, {"center": to2(e.c), "radius": e.r * MM});
            else if (e.k == "spline") cqSkSpline(sk, "p" ~ j, e, to2, planar, st);
            else cqUnsup("sweep path edge " ~ e.k);
        }
        skSolve(sk);
        return qCreatedBy(id, EntityType.EDGE);
    }
    for (var e in edges) if (e.k != "line") cqUnsup("a sweep path off one plane with a " ~ e.k);
    var chain = [edges[0].a * MM];
    for (var e in edges) chain = append(chain, e.b * MM);
    const id = nid(st, "pl");
    opPolyline(c, id, {"points": chain});
    return qCreatedBy(id, EntityType.EDGE);
};
const cqSweep = function(c, st, out, selfI, a) {     // Workplane.sweep / _sweep
    if (a.multisection == true || a.sweepAlongWires == true) cqUnsup("multisection sweep");
    if (a.normal != undefined || a.auxSpine != undefined) cqUnsup("sweep with a normal or an auxiliary spine");
    if (a.makeSolid == false) cqUnsup("sweep as a shell");
    var p;
    if (a.path.wp != undefined) {
        // path.wire(): its pending edges made a wire (and a pending wire of its context)
        const tmp = toString(a.path.wp) ~ "_wire";
        cqWire(c, st, tmp, a.path.wp, {"forConstruction": false});
        const objs = wpGet(st, tmp).objs;
        if (size(objs) == 0) cqErr("Wire or Edge instance required");
        p = objs[0];
    } else p = shapeGet(st, a.path.shape);
    if (p.t != "wire" && p.t != "edge") cqErr("Wire or Edge instance required");
    var corners = false;
    if (p.t == "wire") for (var j = 0; j + 1 < size(p.edges); j += 1) corners = true;
    if (corners) warn(st, "sweep along several edges: Parasolid's joins where they meet at an angle, not OCCT's '" ~ a.transition ~ "' transition");
    const path = pathEdges(c, st, p);
    const faces = getFaces(c, st, selfI).q;
    const id = nid(st, "sw");
    opSweep(c, id, {"profiles": faces, "path": path});
    combineWithBase(c, st, out, selfI, qCreatedBy(id, EntityType.BODY), a.combine);
};


const unionLike = function(c, st, out, selfI, kind, a, argName) {
    const arg = a[argName];
    if (kind == "union") {      // Workplane.union
        var newS = [];
        if (arg != undefined) {
            if (arg.wp != undefined) {
                const sq = select(c, st, wpGet(st, arg.wp), "Solids", undefined, undefined);
                if (size(sq) < 1) cqErr("Workplane object must have at least one solid on the stack to union!");
                for (var o in sq) newS = append(newS, o.q);
            } else {
                for (var o in argShapes(c, st, arg)) if (o.t == "solid") newS = append(newS, o.q);
            }
        }
        const solidRef = findSolidQ(st, selfI, true, true);
        var r;
        if (solidRef != undefined) r = boolOp(c, st, "union", solidRef, size(newS) == 0 ? undefined : qUnion(newS));
        else if (size(newS) > 1) r = boolOp(c, st, "union", newS[0], qUnion(subArray(newS, 1, size(newS))));
        else if (size(newS) == 1) r = newS[0];
        else cqErr("union of nothing");
        newObject(st, out, selfI, [solidObj(r)]);
        return;
    }
    const solidRef = findSolid(st, selfI);   // Workplane.cut / intersect
    const tools = solidsQ(c, argShapes(c, st, arg));
    if (tools == undefined) cqErr("Cannot " ~ kind ~ " a non-solid");
    newObject(st, out, selfI, [solidObj(boolOp(c, st, kind, solidRef, tools))]);
};
const cqUnion = function(c, st, out, selfI, a) { unionLike(c, st, out, selfI, "union", a, "toUnion"); };
const cqCut = function(c, st, out, selfI, a) { unionLike(c, st, out, selfI, "cut", a, "toCut"); };
const cqIntersect = function(c, st, out, selfI, a) { unionLike(c, st, out, selfI, "intersect", a, "toIntersect"); };

const cqCombine = function(c, st, out, selfI, a) {   // Workplane.combine
    const w = wpGet(st, selfI);
    var qs = [];
    for (var o in w.objs) {
        if (o.t == "solid") qs = append(qs, o.q);
        else if (isShapeObj(o)) cqUnsup("combine of " ~ o.t);
    }
    if (size(qs) == 0) cqErr("pop from empty list");
    var s = qs[0];
    if (size(qs) > 1) s = boolOp(c, st, "union", s, qUnion(subArray(qs, 1, size(qs))));
    newObject(st, out, selfI, [solidObj(s)]);
};

const edgesOfStack = function(c, st, selfI) {   // self.edges().vals()
    return select(c, st, wpGet(st, selfI), "Edges", undefined, undefined);
};
const cqFillet = function(c, st, out, selfI, a) {   // Workplane.fillet
    const solid = findSolid(st, selfI);
    const edges = edgesOfStack(c, st, selfI);
    if (size(edges) < 1) cqErr("Fillets requires that edges be selected");
    const cw = copyWithEntities(c, st, solid, edges);
    const eq = taggedIn(cw.base, EntityType.EDGE, cw.tag);
    if (size(ents(c, eq)) != size(edges)) cqErr("fillet: selected edges do not belong to the solid");
    opFillet(c, nid(st, "fi"), {"entities": eq, "radius": a.radius * MM});
    newObject(st, out, selfI, [solidObj(cw.base)]);
};
const cqChamfer = function(c, st, out, selfI, a) {   // Workplane.chamfer
    if (a.length2 != undefined && a.length2 != a.length) cqUnsup("chamfer with two lengths (CadQuery's side depends on OCCT's edge-face map order)");
    const solid = findSolid(st, selfI);
    const edges = edgesOfStack(c, st, selfI);
    if (size(edges) < 1) cqErr("Chamfer requires that edges be selected");
    const cw = copyWithEntities(c, st, solid, edges);
    const eq = taggedIn(cw.base, EntityType.EDGE, cw.tag);
    if (size(ents(c, eq)) != size(edges)) cqErr("chamfer: selected edges do not belong to the solid");
    opChamfer(c, nid(st, "ch"), {"entities": eq, "chamferType": ChamferType.EQUAL_OFFSETS, "width": a.length * MM});
    newObject(st, out, selfI, [solidObj(cw.base)]);
};
const cqShell = function(c, st, out, selfI, a) {     // Workplane.shell
    const solid = findSolid(st, selfI);
    const w = wpGet(st, selfI);
    var faces = [];
    for (var o in w.objs) if (o.t == "face") faces = append(faces, o);
    if (a.kind != "arc") warn(st, "shell kind " ~ a.kind ~ ": Onshape's shell has one join type");
    const cw = copyWithEntities(c, st, solid, faces);
    var entsQ = cw.base;
    if (size(faces) > 0) {
        entsQ = taggedIn(cw.base, EntityType.FACE, cw.tag);
        if (size(ents(c, entsQ)) != size(faces)) cqErr("shell: selected faces do not belong to the solid");
    }
    opShell(c, nid(st, "sh"), {"entities": entsQ, "thickness": a.thickness * MM});
    newObject(st, out, selfI, [solidObj(cw.base)]);
};

const xformObjs = function(c, st, objs, T) {   // Shape.rotate / translate / mirror over the stack's shapes
    var out = [];
    for (var o in objs) {
        if (o.t == "solid") out = append(out, solidObj(copyBodies(c, st, o.q, T)));
        else if (o.t == "wire") out = append(out, wXform(T, o));
        else if (o.t == "edge") out = append(out, eXform(T, o));
        else if (o.t == "face" || o.t == "edgeq" || o.t == "vertex") cqUnsup("transforming a selected " ~ o.t);
        else out = append(out, o);   // points and locations are not Shapes: kept as they are
    }
    return out;
};
const rotT = function(s, e, deg) {
    const sv = v3(s); const ev = v3(e);
    if (norm(ev - sv) < 1e-12) cqErr("rotation axis of zero length");
    return rotationAround(line(sv * MM, normalize(ev - sv)), deg * degree);
};
const cqRotate = function(c, st, out, selfI, a) {
    newObject(st, out, selfI, xformObjs(c, st, wpGet(st, selfI).objs, rotT(a.axisStartPoint, a.axisEndPoint, a.angleDegrees)));
};
const cqTranslate = function(c, st, out, selfI, a) {
    newObject(st, out, selfI, xformObjs(c, st, wpGet(st, selfI).objs, transform(v3(a.vec) * MM)));
};
const mirrorT = function(c, st, mp, bp) {   // Shape.mirror(mirrorPlane, basePointVector)
    var n;
    if (mp is string) {
        if (mp == "XY" || mp == "YX") n = vector(0, 0, 1);
        else if (mp == "XZ" || mp == "ZX") n = vector(0, 1, 0);
        else if (mp == "YZ" || mp == "ZY") n = vector(1, 0, 0);
        else cqErr("mirror plane " ~ mp);
    } else n = v3(mp);
    return mirrorAcross(plane(v3(bp) * MM, normalize(n)));
};
const cqMirror = function(c, st, out, selfI, a) {   // Workplane.mirror
    var mp = a.mirrorPlane; var bp = a.basePointVector;
    if (mp is map && mp.wp != undefined) {
        const fo = wpGet(st, mp.wp).objs;
        if (size(fo) == 0 || fo[0].t != "face") cqErr("Face required");
        mp = faceNormal0(c, fo[0].q);
        if (bp == undefined) bp = centroid(c, fo[0].q);
    }
    if (bp == undefined) bp = [0, 0, 0];
    const w = wpGet(st, selfI);
    var shapes = [];
    for (var o in w.objs) if (isShapeObj(o)) shapes = append(shapes, o);
    const mirrored = xformObjs(c, st, shapes, mirrorT(c, st, mp, bp));
    if (a.union == true) {
        const tmp = out ~ "_m";
        newObject(st, tmp, selfI, mirrored);
        cqUnion(c, st, out, selfI, {"toUnion": {"wp": tmp}});
    } else newObject(st, out, selfI, mirrored);
};

const cqSplit = function(c, st, out, selfI, a) {   // Workplane.split(keepTop, keepBottom)
    if (!a.keepTop && !a.keepBottom) cqErr("You have to keep at least one half");
    const w = wpGet(st, selfI);
    const solid = findSolid(st, selfI);
    const bb = evBox3d(c, {"topology": solid, "tight": false});
    const maxDim = norm(bb.maxCorner - bb.minCorner) / MM * 10;
    const mk = function(sign) {
        var qs = [];
        for (var T in placements(c, w)) {
            const id = nid(st, "bx");
            fCuboid(c, id, {"corner1": vector(-maxDim / 2, -maxDim / 2, sign > 0 ? 0 : -maxDim) * MM, "corner2": vector(maxDim / 2, maxDim / 2, sign > 0 ? maxDim : 0) * MM});
            opTransform(c, nid(st, "tr"), {"bodies": qCreatedBy(id, EntityType.BODY), "transform": T});
            qs = append(qs, qCreatedBy(id, EntityType.BODY));
        }
        return qUnion(qs);
    };
    var rv = [];
    if (a.keepTop) rv = append(rv, solidObj(boolOp(c, st, "cut", solid, mk(-1))));
    if (a.keepBottom) rv = append(rv, solidObj(boolOp(c, st, "cut", solid, mk(1))));
    newObject(st, out, selfI, rv);
};

const cqSelect = function(kind) {
    return function(c, st, out, selfI, a) {
        const w = wpGet(st, selfI);
        var tagW = undefined;
        if (a.tag != undefined) {
            const cx = ctxGet(st, w);
            if (cx.tags[a.tag] == undefined) cqErr("No Workplane object named " ~ a.tag ~ " in chain");
            tagW = wpGet(st, cx.tags[a.tag]);
        }
        newObject(st, out, selfI, select(c, st, w, kind, a.selector, tagW));
    };
};

const cqAdd = function(c, st, out, selfI, a) {      // Workplane.add: mutates self
    var w = wpGet(st, selfI);
    const o = a.obj;
    if (o is array) { for (var x in o) w.objs = concatenateArrays([w.objs, argShapes(c, st, x)]); }
    else w.objs = concatenateArrays([w.objs, argShapes(c, st, o)]);
    wpPut(st, selfI, w);
};
const cqTag = function(c, st, out, selfI, a) {
    const w = wpGet(st, selfI);
    var cx = ctxGet(st, w);
    cx.tags[a.name] = selfI;
    ctxPut(st, w, cx);
};
const cqEnd = function(c, st, out, selfI, a) {
    var cur = selfI;
    for (var i = 0; i < a.n; i += 1) {
        const p = wpGet(st, cur).parent;
        if (p == undefined) cqErr("Cannot End the chain-- no parents!");
        cur = p;
    }
    wpPut(st, out, wpGet(st, cur));
};
const cqWorkplaneFromTagged = function(c, st, out, selfI, a) {
    const w = wpGet(st, selfI);
    const cx = ctxGet(st, w);
    if (cx.tags[a.name] == undefined) cqErr("No Workplane object named " ~ a.name ~ " in chain");
    wpPut(st, out, {"plane": wpGet(st, cx.tags[a.name]).plane, "objs": [], "parent": selfI, "ctx": w.ctx});
};
const cqCopyWorkplane = function(c, st, out, selfI, a) {
    const w = wpGet(st, selfI);
    wpPut(st, out, {"plane": wpGet(st, a.obj.wp).plane, "objs": [], "parent": selfI, "ctx": w.ctx});
};
const cqToPending = function(c, st, out, selfI, a) {
    const w = wpGet(st, selfI);
    var cx = ctxGet(st, w);
    for (var o in w.objs) {
        if (o.t == "wire") cx.pw = append(cx.pw, o);
        if (o.t == "edge") cx.pe = append(cx.pe, o);
    }
    ctxPut(st, w, cx);
};
const cqClean = function(c, st, out, selfI, a) { newObject(st, out, selfI, wpGet(st, selfI).objs); };
const cqSlice = function(which) {
    return function(c, st, out, selfI, a) {
        const objs = wpGet(st, selfI).objs;
        if (size(objs) > 1) warn(st, which ~ "() of " ~ size(objs) ~ " objects: Parasolid's order may not be OCCT's");
        if (size(objs) == 0) { newObject(st, out, selfI, []); return; }
        var i = (which == "first") ? 0 : ((which == "last") ? size(objs) - 1 : a.i);
        if (i < 0) i += size(objs);
        newObject(st, out, selfI, [objs[i]]);
    };
};
const cqVal = function(c, st, outShape, selfI) {   // Workplane.val
    const w = wpGet(st, selfI);
    var s = st[];
    if (size(w.objs) > 1) {
        var sameKind = true;
        for (var o in w.objs) if (o.t != w.objs[0].t || (o.t != "solid" && o.t != "vec")) sameKind = false;
        if (w.objs[0].t != "solid" || size(w.objs) > 1) warn(st, "val() of " ~ size(w.objs) ~ " objects: Parasolid's order may not be OCCT's");
    }
    s = st[];
    s.shapes[outShape] = (size(w.objs) == 0) ? {"t": "vec", "p": w.plane.o} : w.objs[0];
    st[] = s;
};

// ---- shape-level calls (cq.Solid.make*, Shape methods)
const putShape = function(st, i, o) { var s = st[]; s.shapes[i] = o; st[] = s; };
const shCompoundMake = function(c, st, out, a) {    // Compound.makeCompound(listOfShapes)
    var qs = [];
    for (var x in a.listOfShapes) for (var o in argShapes(c, st, x)) {
        if (o.t == "solid") qs = append(qs, o.q);
        else cqUnsup("a compound of " ~ o.t);
    }
    putShape(st, out, solidObj(qUnion(qs)));
};

const shSolidMakeBox = function(c, st, out, a) {   // Solid.makeBox(length, width, height, pnt, dir)
    const T = toWorld(coordSystem(v3(a.pnt) * MM, defaultXDir(normalize(v3(a.dir))), normalize(v3(a.dir))));
    const id = nid(st, "bx");
    fCuboid(c, id, {"corner1": vector(0, 0, 0) * MM, "corner2": vector(a.length, a.width, a.height) * MM});
    opTransform(c, nid(st, "tr"), {"bodies": qCreatedBy(id, EntityType.BODY), "transform": T});
    putShape(st, out, solidObj(qCreatedBy(id, EntityType.BODY)));
};
const shSolidMakeCylinder = function(c, st, out, a) {   // Solid.makeCylinder(radius, height, pnt, dir, angleDegrees)
    if (a.angleDegrees != 360) cqUnsup("partial cylinder");
    const p = v3(a.pnt); const d = normalize(v3(a.dir));
    const id = nid(st, "cy");
    fCylinder(c, id, {"bottomCenter": p * MM, "topCenter": (p + d * a.height) * MM, "radius": a.radius * MM});
    putShape(st, out, solidObj(qCreatedBy(id, EntityType.BODY)));
};
const shSolidMakeCone = function(c, st, out, a) {   // Solid.makeCone(radius1, radius2, height, pnt, dir, angleDegrees)
    if (a.angleDegrees != 360) cqUnsup("partial cone");
    const p = v3(a.pnt); const d = normalize(v3(a.dir));
    const id = nid(st, "co");
    fCone(c, id, {"bottomCenter": p * MM, "topCenter": (p + d * a.height) * MM, "bottomRadius": a.radius1 * MM, "topRadius": a.radius2 * MM});
    putShape(st, out, solidObj(qCreatedBy(id, EntityType.BODY)));
};
const shSolidMakeSphere = function(c, st, out, a) {   // Solid.makeSphere(radius, pnt, dir, angleDegrees1, 2, 3)
    if (a.angleDegrees1 != 0 && a.angleDegrees1 != -90) cqUnsup("partial sphere");
    if (a.angleDegrees2 != 90 || a.angleDegrees3 != 360) cqUnsup("partial sphere");
    if (a.angleDegrees1 == 0) cqUnsup("hemisphere (makeSphere's default angleDegrees1=0)");
    const id = nid(st, "sp");
    opSphere(c, id, {"center": v3(a.pnt) * MM, "radius": a.radius * MM});
    putShape(st, out, solidObj(qCreatedBy(id, EntityType.BODY)));
};
const shapeXform = function(c, st, out, src, T) {
    if (src.t == "solid") putShape(st, out, solidObj(copyBodies(c, st, src.q, T)));
    else if (src.t == "wire") putShape(st, out, wXform(T, src));
    else if (src.t == "edge") putShape(st, out, eXform(T, src));
    else cqUnsup("transforming a " ~ src.t);
};
const shTranslate = function(c, st, out, selfS, a) { shapeXform(c, st, out, shapeGet(st, selfS), transform(v3(a.vector) * MM)); };
const shRotate = function(c, st, out, selfS, a) { shapeXform(c, st, out, shapeGet(st, selfS), rotT(a.startVector, a.endVector, a.angleDegrees)); };
const shMirror = function(c, st, out, selfS, a) { shapeXform(c, st, out, shapeGet(st, selfS), mirrorT(c, st, a.mirrorPlane, a.basePointVector)); };
const shMoved = function(c, st, out, selfS, a) { shapeXform(c, st, out, shapeGet(st, selfS), locFromTape(a.loc.loc)); };
const shBool = function(kind) {
    return function(c, st, out, selfS, a) {
        const s = shapeGet(st, selfS);
        if (s.t != "solid") cqUnsup(kind ~ " of a " ~ s.t);
        var qs = [];
        for (var x in a.toolShapes) for (var o in argShapes(c, st, x)) if (o.t == "solid") qs = append(qs, o.q);
        if (size(qs) == 0) { putShape(st, out, s); return; }
        putShape(st, out, solidObj(boolOp(c, st, kind, s.q, qUnion(qs))));
    };
};

// ---- measuring the result (worker.py's measure)
const cqMeasure = function(c, st, res) {
    var objs = [];
    if (res.wp != undefined) objs = wpGet(st, res.wp).objs;
    else if (res.shape != undefined) objs = [shapeGet(st, res.shape)];
    else if (res.list != undefined) for (var r in res.list) objs = concatenateArrays([objs, argShapes(c, st, r)]);
    var qs = [];
    var other = [];
    for (var o in objs) {
        if (o.t == "solid") qs = append(qs, o.q);
        else other = append(other, o.t);
    }
    if (size(qs) == 0) return {"status": "no-shape", "other": other, "warn": st[].warn, "log": st[].log};
    const B = qUnion(qs);
    const faces = qOwnedByBody(B, EntityType.FACE);
    const bb = evBox3d(c, {"topology": B, "tight": true});
    return {"status": "ok", "result_q": B,
            "vol": evVolume(c, {"entities": B, "accuracy": "HIGH"}) / (MM ^ 3),
            "area": evArea(c, {"entities": faces}) / (MM ^ 2),
            "solids": size(ents(c, qBodyType(qEntityFilter(B, EntityType.BODY), BodyType.SOLID))),
            "faces": size(ents(c, faces)),
            "edges": size(ents(c, qOwnedByBody(B, EntityType.EDGE))),
            "vertices": size(ents(c, qOwnedByBody(B, EntityType.VERTEX))),
            "bbox": [bb.minCorner[0] / MM, bb.minCorner[1] / MM, bb.minCorner[2] / MM, bb.maxCorner[0] / MM, bb.maxCorner[1] / MM, bb.maxCorner[2] / MM],
            "other": other, "warn": st[].warn, "log": st[].log};
};
const newState = function(base) { return new box({"base": base, "wps": {}, "ctx": [], "shapes": {}, "n": 0, "log": [], "warn": [], "cur": 0}); };
const atStep = function(st, i) { var s = st[]; s.cur = i; st[] = s; };
const stepVol = function(c, st, out, op) {   // after a solid-producing step: its solids and volume
    const w = st[].wps[out];
    if (w == undefined) return;
    const q = solidsQ(c, w.objs);
    if (q == undefined) return;
    logStep(c, st, out, op, {"solids": size(ents(c, q)), "vol": evVolume(c, {"entities": q}) / (MM ^ 3)});
};

const case_5587 = function(c, base) {
    var st = newState(base);
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 2);
        cqSphere(c, st, 2, 1, {"radius": 10, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 2, "sphere");
        atStep(st, 3);
        cqWorkplaneNew(c, st, 3, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 4);
        cqSphere(c, st, 4, 3, {"radius": 9, "direct": [0, 0, 1], "angle1": -90, "angle2": 90, "angle3": 360, "centered": true, "combine": true, "clean": true});
        stepVol(c, st, 4, "sphere");
        atStep(st, 5);
        cqCut(c, st, 5, 2, {"toCut": {"wp": 4}, "clean": true, "tol": undefined});
        stepVol(c, st, 5, "cut");
        atStep(st, 6);
        cqWorkplaneNew(c, st, 6, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, 0, 1]}, "obj": undefined});
        atStep(st, 7);
        cqBox(c, st, 7, 6, {"length": 20, "width": 20, "height": 10, "centered": [true, true, false], "combine": true, "clean": true});
        stepVol(c, st, 7, "box");
        atStep(st, 8);
        cqTranslate(c, st, 8, 7, {"vec": [0, 0, -10]});
        stepVol(c, st, 8, "translate");
        atStep(st, 9);
        cqCut(c, st, 9, 5, {"toCut": {"wp": 8}, "clean": true, "tol": undefined});
        stepVol(c, st, 9, "cut");
        atStep(st, 10);
        cqWorkplaneNew(c, st, 10, {"plane": {"origin": [2.9562802988843666, -5.696278614798906, 3.1227230737905742], "xDir": [0.7936054412267357, 0, 0.6084327437402743], "zDir": [-0.5595267423392979, 0.39280986754873376, 0.7298152043932211]}, "obj": undefined});
        atStep(st, 11);
        cqCircle(c, st, 11, 10, {"radius": 0.2, "forConstruction": false});
        atStep(st, 12);
        cqExtrude(c, st, 12, 11, {"until": 3.6407050759967623, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 12, "extrude");
        atStep(st, 13);
        cqWorkplaneNew(c, st, 13, {"plane": {"origin": [5.396589308222097, -4.255941653379915, 1.5982184788730889], "xDir": [1.3877787807814457e-17, 0.4710326883526908, 0.8821157557278052], "zDir": [0.22868596691118576, 0.8587399050093207, -0.4585504378827689]}, "obj": undefined});
        atStep(st, 14);
        cqCircle(c, st, 14, 13, {"radius": 0.2, "forConstruction": false});
        atStep(st, 15);
        cqExtrude(c, st, 15, 14, {"until": 3.8129114691796064, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 15, "extrude");
        atStep(st, 16);
        cqUnion(c, st, 16, 12, {"toUnion": {"wp": 15}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 16, "union");
        atStep(st, 17);
        cqWorkplaneNew(c, st, 17, {"plane": {"origin": [3.449177004286556, -5.437158722630899, 3.46183431207547], "xDir": [-0.5477011681948759, 0, -0.8366740287339918], "zDir": [-0.8112352360256169, 0.244713882682124, 0.5310484982118878]}, "obj": undefined});
        atStep(st, 18);
        cqCircle(c, st, 18, 17, {"radius": 0.2, "forConstruction": false});
        atStep(st, 19);
        cqExtrude(c, st, 19, 18, {"until": 3.726249955074114, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 19, "extrude");
        atStep(st, 20);
        cqUnion(c, st, 20, 16, {"toUnion": {"wp": 19}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 20, "union");
        atStep(st, 21);
        cqWorkplaneNew(c, st, 21, {"plane": {"origin": [4.8653120946906085, -3.773076453546532, 2.8212273222064628], "xDir": [0, -0.16234579417558795, 0.986733927213156], "zDir": [-0.044311460860281184, 0.9857647224417226, 0.1621863324260751]}, "obj": undefined});
        atStep(st, 22);
        cqCircle(c, st, 22, 21, {"radius": 0.2, "forConstruction": false});
        atStep(st, 23);
        cqExtrude(c, st, 23, 22, {"until": 4.301259251818759, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 23, "extrude");
        atStep(st, 24);
        cqUnion(c, st, 24, 20, {"toUnion": {"wp": 23}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 24, "union");
        atStep(st, 25);
        cqWorkplaneNew(c, st, 25, {"plane": {"origin": [6.146117817497505, -0.16202786722589746, 1.0083413406966333], "xDir": [0.9219677060439812, 0.3872667672444912, 0], "zDir": [-0.3870472410437176, 0.9214450790466177, -0.03366599918640569]}, "obj": undefined});
        atStep(st, 26);
        cqCircle(c, st, 26, 25, {"radius": 0.2, "forConstruction": false});
        atStep(st, 27);
        cqExtrude(c, st, 27, 26, {"until": 6.339638905851551, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 27, "extrude");
        atStep(st, 28);
        cqUnion(c, st, 28, 24, {"toUnion": {"wp": 27}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 28, "union");
        atStep(st, 29);
        cqWorkplaneNew(c, st, 29, {"plane": {"origin": [6.071501122778148, -2.367952020438811, 2.142543266729726], "xDir": [-0.619643319172999, 2.775557561562892e-17, -0.784883530852998], "zDir": [-0.7207532730290819, 0.3959008075476738, 0.5690142968335378]}, "obj": undefined});
        atStep(st, 30);
        cqCircle(c, st, 30, 29, {"radius": 0.2, "forConstruction": false});
        atStep(st, 31);
        cqExtrude(c, st, 31, 30, {"until": 3.6114621113304888, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 31, "extrude");
        atStep(st, 32);
        cqUnion(c, st, 32, 28, {"toUnion": {"wp": 31}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 32, "union");
        atStep(st, 33);
        cqWorkplaneNew(c, st, 33, {"plane": {"origin": [-0.9453520300960612, -5.371466345246673, 3.0778829951833933], "xDir": [-0.43005048122860734, 6.938893903907228e-18, 0.9028048424742988], "zDir": [0.8961388257580677, 0.1212964530417104, 0.4268751286365274]}, "obj": undefined});
        atStep(st, 34);
        cqCircle(c, st, 34, 33, {"radius": 0.2, "forConstruction": false});
        atStep(st, 35);
        cqExtrude(c, st, 35, 34, {"until": 6.434486087680072, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 35, "extrude");
        atStep(st, 36);
        cqUnion(c, st, 36, 32, {"toUnion": {"wp": 35}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 36, "union");
        atStep(st, 37);
        cqWorkplaneNew(c, st, 37, {"plane": {"origin": [-3.5399387863183023, -3.871838137520823, 2.5282826618697545], "xDir": [6.938893903907228e-18, -0.3995739767555509, 0.9167009529283552], "zDir": [0.13859413359922915, 0.9078541147009208, 0.3957177940811938]}, "obj": undefined});
        atStep(st, 38);
        cqCircle(c, st, 38, 37, {"radius": 0.2, "forConstruction": false});
        atStep(st, 39);
        cqExtrude(c, st, 39, 38, {"until": 4.163374592710528, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 39, "extrude");
        atStep(st, 40);
        cqUnion(c, st, 40, 36, {"toUnion": {"wp": 39}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 40, "union");
        atStep(st, 41);
        cqWorkplaneNew(c, st, 41, {"plane": {"origin": [-3.9188946939387135, -4.5581662525476885, 2.9769031980318132], "xDir": [0, 0.726486549465266, -0.6871806847154914], "zDir": [-0.05157302465739921, 0.6862662025848553, 0.7255197601150845]}, "obj": undefined});
        atStep(st, 42);
        cqCircle(c, st, 42, 41, {"radius": 0.2, "forConstruction": false});
        atStep(st, 43);
        cqExtrude(c, st, 43, 42, {"until": 3.507502651177352, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 43, "extrude");
        atStep(st, 44);
        cqUnion(c, st, 44, 40, {"toUnion": {"wp": 43}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 44, "union");
        atStep(st, 45);
        cqWorkplaneNew(c, st, 45, {"plane": {"origin": [-4.788105491672951, -4.048152768509407, 1.5618505530450015], "xDir": [0.8724914809320292, 0.48862932341503496, 0], "zDir": [-0.4873451147699195, 0.8701984112186284, -0.07245318642039793]}, "obj": undefined});
        atStep(st, 46);
        cqCircle(c, st, 46, 45, {"radius": 0.2, "forConstruction": false});
        atStep(st, 47);
        cqExtrude(c, st, 47, 46, {"until": 3.938305849417748, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 47, "extrude");
        atStep(st, 48);
        cqUnion(c, st, 48, 44, {"toUnion": {"wp": 47}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 48, "union");
        atStep(st, 49);
        cqWorkplaneNew(c, st, 49, {"plane": {"origin": [-5.508428411835695, -3.985114318987899, 1.343021285184569], "xDir": [0.7265868691544087, 0.6870746113577435, 0], "zDir": [-0.6796863485267193, 0.7187737224158389, -0.14625595232637054]}, "obj": undefined});
        atStep(st, 50);
        cqCircle(c, st, 50, 49, {"radius": 0.2, "forConstruction": false});
        atStep(st, 51);
        cqExtrude(c, st, 51, 50, {"until": 4.94339773601756, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 51, "extrude");
        atStep(st, 52);
        cqUnion(c, st, 52, 48, {"toUnion": {"wp": 51}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 52, "union");
        atStep(st, 53);
        cqWorkplaneNew(c, st, 53, {"plane": {"origin": [1.0978267250465503, -1.1040166231510071, 6.46403042182595], "xDir": [0.4508484392046364, -2.7755575615628907e-17, 0.8926005180744314], "zDir": [0.8287534142942832, 0.3714056911454875, -0.41859955910190727]}, "obj": undefined});
        atStep(st, 54);
        cqCircle(c, st, 54, 53, {"radius": 0.2, "forConstruction": false});
        atStep(st, 55);
        cqExtrude(c, st, 55, 54, {"until": 4.200613344472963, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 55, "extrude");
        atStep(st, 56);
        cqUnion(c, st, 56, 52, {"toUnion": {"wp": 55}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 56, "union");
        atStep(st, 57);
        cqWorkplaneNew(c, st, 57, {"plane": {"origin": [0.6474673863772811, -3.4326543249131185, 5.8972309510902585], "xDir": [2.7755575615628914e-17, 0.6824829990998498, -0.7309014680103432], "zDir": [0.520130599612113, -0.6242532520327987, -0.5828996797679846]}, "obj": undefined});
        atStep(st, 58);
        cqCircle(c, st, 58, 57, {"radius": 0.2, "forConstruction": false});
        atStep(st, 59);
        cqExtrude(c, st, 59, 58, {"until": 4.961357735822509, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 59, "extrude");
        atStep(st, 60);
        cqUnion(c, st, 60, 56, {"toUnion": {"wp": 59}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 60, "union");
        atStep(st, 61);
        cqWorkplaneNew(c, st, 61, {"plane": {"origin": [-0.808000447405848, -2.4831618779144695, 7.765939232724429], "xDir": [0, -0.5765391654696888, -0.8170695139824488], "zDir": [-0.2197904618005679, -0.7970897984583681, 0.5624411134466357]}, "obj": undefined});
        atStep(st, 62);
        cqCircle(c, st, 62, 61, {"radius": 0.2, "forConstruction": false});
        atStep(st, 63);
        cqExtrude(c, st, 63, 62, {"until": 1.5031666562604558, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 63, "extrude");
        atStep(st, 64);
        cqUnion(c, st, 64, 60, {"toUnion": {"wp": 63}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 64, "union");
        atStep(st, 65);
        cqWorkplaneNew(c, st, 65, {"plane": {"origin": [0.910245786233568, -1.0332519548722148, 5.93797320784596], "xDir": [0.6709393966522944, -2.77555756156289e-17, 0.7415121887196832], "zDir": [0.6869905088289618, 0.37636294008552074, -0.6216067712864034]}, "obj": undefined});
        atStep(st, 66);
        cqCircle(c, st, 66, 65, {"radius": 0.2, "forConstruction": false});
        atStep(st, 67);
        cqExtrude(c, st, 67, 66, {"until": 4.521329965723662, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 67, "extrude");
        atStep(st, 68);
        cqUnion(c, st, 68, 64, {"toUnion": {"wp": 67}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 68, "union");
        atStep(st, 69);
        cqWorkplaneNew(c, st, 69, {"plane": {"origin": [-1.94711936984496, -1.9330261171872696, 5.34763061777662], "xDir": [-0.8370651409642376, -1.7347234759768069e-18, 0.5471032350320377], "zDir": [-0.546987976871847, -0.020525480063912537, -0.8368887965708406]}, "obj": undefined});
        atStep(st, 70);
        cqCircle(c, st, 70, 69, {"radius": 0.2, "forConstruction": false});
        atStep(st, 71);
        cqExtrude(c, st, 71, 70, {"until": 4.769061933206067, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 71, "extrude");
        atStep(st, 72);
        cqUnion(c, st, 72, 68, {"toUnion": {"wp": 71}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 72, "union");
        atStep(st, 73);
        cqWorkplaneNew(c, st, 73, {"plane": {"origin": [0.05031627245227965, -0.15738731511113435, 5.822899945581062], "xDir": [0, 0.6608302567923506, 0.7505353900435049], "zDir": [0.2884704405092832, 0.7186292310782474, -0.6327375705553345]}, "obj": undefined});
        atStep(st, 74);
        cqCircle(c, st, 74, 73, {"radius": 0.2, "forConstruction": false});
        atStep(st, 75);
        cqExtrude(c, st, 75, 74, {"until": 4.805524419507875, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 75, "extrude");
        atStep(st, 76);
        cqUnion(c, st, 76, 72, {"toUnion": {"wp": 75}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 76, "union");
        atStep(st, 77);
        cqWorkplaneNew(c, st, 77, {"plane": {"origin": [-2.326075277465371, -2.619354232214134, 5.7962511539386785], "xDir": [0.676668460076745, 0, -0.7362878480169063], "zDir": [-0.7009279901296157, -0.30617421986214866, -0.6441717936580755]}, "obj": undefined});
        atStep(st, 78);
        cqCircle(c, st, 78, 77, {"radius": 0.2, "forConstruction": false});
        atStep(st, 79);
        cqExtrude(c, st, 79, 78, {"until": 4.802963216006266, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 79, "extrude");
        atStep(st, 80);
        cqUnion(c, st, 80, 76, {"toUnion": {"wp": 79}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 80, "union");
        atStep(st, 81);
        cqWorkplaneNew(c, st, 81, {"plane": {"origin": [-0.4517909183665335, 1.3044649933017003, 6.651490434783646], "xDir": [1.7347234759768069e-18, 0.2120096373188892, 0.977267575275018], "zDir": [0.05844581536143409, 0.9755970167635828, -0.21164722428781144]}, "obj": undefined});
        atStep(st, 82);
        cqCircle(c, st, 82, 81, {"radius": 0.2, "forConstruction": false});
        atStep(st, 83);
        cqExtrude(c, st, 83, 82, {"until": 6.536607662556763, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 83, "extrude");
        atStep(st, 84);
        cqUnion(c, st, 84, 80, {"toUnion": {"wp": 83}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 84, "union");
        atStep(st, 85);
        cqWorkplaneNew(c, st, 85, {"plane": {"origin": [-2.383741712842345, -2.5697151374627865, 6.7342600309316225], "xDir": [0.3664370392771863, -0.9304428495322912, -1.3877787807814457e-17], "zDir": [-0.8847636302134749, -0.34844715640356944, -0.30948004434169885]}, "obj": undefined});
        atStep(st, 86);
        cqCircle(c, st, 86, 85, {"radius": 0.2, "forConstruction": false});
        atStep(st, 87);
        cqExtrude(c, st, 87, 86, {"until": 3.9353609320215774, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 87, "extrude");
        atStep(st, 88);
        cqUnion(c, st, 88, 84, {"toUnion": {"wp": 87}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 88, "union");
        atStep(st, 89);
        cqWorkplaneNew(c, st, 89, {"plane": {"origin": [-2.458196016526814, -0.7596016707909736, 7.241571812987477], "xDir": [-0.5265814800193179, -0.8501246643290998, 0], "zDir": [-0.8491632213705478, 0.5259859461202185, -0.047545851087268234]}, "obj": undefined});
        atStep(st, 90);
        cqCircle(c, st, 90, 89, {"radius": 0.2, "forConstruction": false});
        atStep(st, 91);
        cqExtrude(c, st, 91, 90, {"until": 4.275706649099303, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 91, "extrude");
        atStep(st, 92);
        cqUnion(c, st, 92, 88, {"toUnion": {"wp": 91}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 92, "union");
        atStep(st, 93);
        cqWorkplaneNew(c, st, 93, {"plane": {"origin": [2.6355341052394463, -1.1874460085869811, 6.52050835403382], "xDir": [0.2434053054626362, 0, 0.9699246657718532], "zDir": [0.9499576425021488, 0.2018626459242173, -0.23839452517252535]}, "obj": undefined});
        atStep(st, 94);
        cqCircle(c, st, 94, 93, {"radius": 0.2, "forConstruction": false});
        atStep(st, 95);
        cqExtrude(c, st, 95, 94, {"until": 6.902083965004847, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 95, "extrude");
        atStep(st, 96);
        cqUnion(c, st, 96, 92, {"toUnion": {"wp": 95}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 96, "union");
        atStep(st, 97);
        cqWorkplaneNew(c, st, 97, {"plane": {"origin": [-1.0357982904535925, -4.167926082756776, 4.350264236226002], "xDir": [0.2638220069778429, 0.9645713807874372, -1.734723475976807e-18], "zDir": [0.96405431662026, -0.263680583637862, 0.03273872963692938]}, "obj": undefined});
        atStep(st, 98);
        cqCircle(c, st, 98, 97, {"radius": 0.2, "forConstruction": false});
        atStep(st, 99);
        cqExtrude(c, st, 99, 98, {"until": 6.168828068250697, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 99, "extrude");
        atStep(st, 100);
        cqUnion(c, st, 100, 96, {"toUnion": {"wp": 99}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 100, "union");
        atStep(st, 101);
        cqWorkplaneNew(c, st, 101, {"plane": {"origin": [-3.6303850466758334, -2.6682978750309267, 3.8006639029123623], "xDir": [2.7755575615628907e-17, 0.5471361890030189, 0.8370436014230398], "zDir": [0.41953328581777233, 0.7598179193310193, -0.4966573784355007]}, "obj": undefined});
        atStep(st, 102);
        cqCircle(c, st, 102, 101, {"radius": 0.2, "forConstruction": false});
        atStep(st, 103);
        cqExtrude(c, st, 103, 102, {"until": 1.8065594336894333, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 103, "extrude");
        atStep(st, 104);
        cqUnion(c, st, 104, 100, {"toUnion": {"wp": 103}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 104, "union");
        atStep(st, 105);
        cqWorkplaneNew(c, st, 105, {"plane": {"origin": [-4.878551752030482, -2.844612506019511, 2.834231794087609], "xDir": [-0.8520856077942367, 0, 0.5234024426671378], "zDir": [-0.5003390558620857, 0.2935762714244006, -0.8145390119789127]}, "obj": undefined});
        atStep(st, 106);
        cqCircle(c, st, 106, 105, {"radius": 0.2, "forConstruction": false});
        atStep(st, 107);
        cqExtrude(c, st, 107, 106, {"until": 3.47448710049862, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 107, "extrude");
        atStep(st, 108);
        cqUnion(c, st, 108, 104, {"toUnion": {"wp": 107}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 108, "union");
        atStep(st, 109);
        cqWorkplaneNew(c, st, 109, {"plane": {"origin": [-4.993814958916528, -1.838992430391568, 4.246956587470506], "xDir": [0.8386174732763685, 0.54472078491238, 1.0842021724855042e-19], "zDir": [-0.5447203330633213, 0.8386167776382979, -0.001288025986522093]}, "obj": undefined});
        atStep(st, 110);
        cqCircle(c, st, 110, 109, {"radius": 0.2, "forConstruction": false});
        atStep(st, 111);
        cqExtrude(c, st, 111, 110, {"until": 3.614603475820105, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 111, "extrude");
        atStep(st, 112);
        cqUnion(c, st, 112, 108, {"toUnion": {"wp": 111}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 112, "union");
        atStep(st, 113);
        cqWorkplaneNew(c, st, 113, {"plane": {"origin": [-4.067007389673218, -3.3049868953064436, 5.187293316067365], "xDir": [0.9981155987148622, 0, 0.061361646018274395], "zDir": [-0.061276226932898366, 0.052746392504465174, 0.9967261620378152]}, "obj": undefined});
        atStep(st, 114);
        cqCircle(c, st, 114, 113, {"radius": 0.2, "forConstruction": false});
        atStep(st, 115);
        cqExtrude(c, st, 115, 114, {"until": 1.8821797053569396, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 115, "extrude");
        atStep(st, 116);
        cqUnion(c, st, 116, 112, {"toUnion": {"wp": 115}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 116, "union");
        atStep(st, 117);
        cqWorkplaneNew(c, st, 117, {"plane": {"origin": [-2.491266124236722, -3.218433635758127, 6.218972517860172], "xDir": [0.7920542152155227, -3.469446951953615e-18, -0.6104507516248322], "zDir": [0.609537342379607, 0.05468394841180816, 0.790869075151444]}, "obj": undefined});
        atStep(st, 118);
        cqCircle(c, st, 118, 117, {"radius": 0.2, "forConstruction": false});
        atStep(st, 119);
        cqExtrude(c, st, 119, 118, {"until": 4.9810724446611445, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 119, "extrude");
        atStep(st, 120);
        cqUnion(c, st, 120, 116, {"toUnion": {"wp": 119}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 120, "union");
        atStep(st, 121);
        cqWorkplaneNew(c, st, 121, {"plane": {"origin": [-5.598874672193226, -2.7815740564980027, 2.615402526227177], "xDir": [-0.716767419463878, 1.3877787807814457e-17, 0.697312316250827], "zDir": [-0.6762702241522506, 0.243806064128094, -0.6951382502922945]}, "obj": undefined});
        atStep(st, 122);
        cqCircle(c, st, 122, 121, {"radius": 0.2, "forConstruction": false});
        atStep(st, 123);
        cqExtrude(c, st, 123, 122, {"until": 4.700883348485637, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 123, "extrude");
        atStep(st, 124);
        cqUnion(c, st, 124, 120, {"toUnion": {"wp": 123}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 124, "union");
        atStep(st, 125);
        cqWorkplaneNew(c, st, 125, {"plane": {"origin": [-5.051481394293502, -1.78935333564022, 5.18496546446345], "xDir": [0, 0.5399089115479858, 0.8417234505650115], "zDir": [-0.4697685052844897, 0.743064743310919, -0.4766259945607819]}, "obj": undefined});
        atStep(st, 126);
        cqCircle(c, st, 126, 125, {"radius": 0.2, "forConstruction": false});
        atStep(st, 127);
        cqExtrude(c, st, 127, 126, {"until": 3.9458054714929816, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 127, "extrude");
        atStep(st, 128);
        cqUnion(c, st, 128, 124, {"toUnion": {"wp": 127}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 128, "union");
        atStep(st, 129);
        cqWorkplaneNew(c, st, 129, {"plane": {"origin": [-3.688051482052807, -2.618658780279579, 4.738672779905306], "xDir": [1.3877787807814457e-17, -0.908779001256426, -0.41727775746542417], "zDir": [0.27511429441775576, 0.4011756461525841, -0.8737106076619862]}, "obj": undefined});
        atStep(st, 130);
        cqCircle(c, st, 130, 129, {"radius": 0.2, "forConstruction": false});
        atStep(st, 131);
        cqExtrude(c, st, 131, 130, {"until": 3.1741160081953588, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 131, "extrude");
        atStep(st, 132);
        cqUnion(c, st, 132, 128, {"toUnion": {"wp": 131}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 132, "union");
        atStep(st, 133);
        cqWorkplaneNew(c, st, 133, {"plane": {"origin": [-1.0934647258305663, -4.118286988005428, 5.2882731132189456], "xDir": [0.2738055964443681, 0.9617850567334388, -1.387778780781446e-17], "zDir": [0.9295602999327376, -0.26463169766696576, -0.25668602100398563]}, "obj": undefined});
        atStep(st, 134);
        cqCircle(c, st, 134, 133, {"radius": 0.2, "forConstruction": false});
        atStep(st, 135);
        cqExtrude(c, st, 135, 134, {"until": 6.521812731113759, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 135, "extrude");
        atStep(st, 136);
        cqUnion(c, st, 136, 132, {"toUnion": {"wp": 135}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 136, "union");
        atStep(st, 137);
        cqWorkplaneNew(c, st, 137, {"plane": {"origin": [-5.656541107570201, -2.731934961746654, 3.553411403220121], "xDir": [-0.8591489790932377, 1.3877787807814457e-17, 0.5117255433560525], "zDir": [-0.50407826738931, 0.1722349187338514, -0.8463097737321335]}, "obj": undefined});
        atStep(st, 138);
        cqCircle(c, st, 138, 137, {"radius": 0.2, "forConstruction": false});
        atStep(st, 139);
        cqExtrude(c, st, 139, 138, {"until": 6.077894571625779, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 139, "extrude");
        atStep(st, 140);
        cqUnion(c, st, 140, 136, {"toUnion": {"wp": 139}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 140, "union");
        atStep(st, 141);
        cqWorkplaneNew(c, st, 141, {"plane": {"origin": [3.881089275959031, -2.569664170634554, 4.936810951466696], "xDir": [-0.9070130440326156, -0.42110252665436326, 0], "zDir": [0.4073802011014569, -0.8774565168613503, -0.25320235537625524]}, "obj": undefined});
        atStep(st, 142);
        cqCircle(c, st, 142, 141, {"radius": 0.2, "forConstruction": false});
        atStep(st, 143);
        cqExtrude(c, st, 143, 142, {"until": 5.118688745365756, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 143, "extrude");
        atStep(st, 144);
        cqUnion(c, st, 144, 140, {"toUnion": {"wp": 143}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 144, "union");
        atStep(st, 145);
        cqWorkplaneNew(c, st, 145, {"plane": {"origin": [4.376170430920494, -0.407380157367512, 5.641320907056834], "xDir": [0.05417601698071921, 0.998531401200836, 4.336808689942018e-19], "zDir": [0.9978605479528894, -0.05413961937027989, 0.03665008122229398]}, "obj": undefined});
        atStep(st, 146);
        cqCircle(c, st, 146, 145, {"radius": 0.2, "forConstruction": false});
        atStep(st, 147);
        cqExtrude(c, st, 147, 146, {"until": 3.082008569929941, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 147, "extrude");
        atStep(st, 148);
        cqUnion(c, st, 148, 144, {"toUnion": {"wp": 147}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 148, "union");
        atStep(st, 149);
        cqWorkplaneNew(c, st, 149, {"plane": {"origin": [0.9326358782751998, -1.7030960266950004, 6.886751785747444], "xDir": [0.5862486356981209, -0.8101311851435493, 0], "zDir": [-0.7088246846086238, -0.5129385362783245, 0.4842123753996124]}, "obj": undefined});
        atStep(st, 150);
        cqCircle(c, st, 150, 149, {"radius": 0.2, "forConstruction": false});
        atStep(st, 151);
        cqExtrude(c, st, 151, 150, {"until": 5.377428901209042, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 151, "extrude");
        atStep(st, 152);
        cqUnion(c, st, 152, 148, {"toUnion": {"wp": 151}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 152, "union");
        atStep(st, 153);
        cqWorkplaneNew(c, st, 153, {"plane": {"origin": [2.388103712058329, -2.65258847369365, 5.018043504113273], "xDir": [-6.9388939039072276e-18, 0.23649894475242525, -0.971631745637713], "zDir": [-0.18468150097131317, -0.9549181488065834, -0.23243079029851196]}, "obj": undefined});
        atStep(st, 154);
        cqCircle(c, st, 154, 153, {"radius": 0.2, "forConstruction": false});
        atStep(st, 155);
        cqExtrude(c, st, 155, 154, {"until": 4.877146181947311, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 155, "extrude");
        atStep(st, 156);
        cqUnion(c, st, 156, 152, {"toUnion": {"wp": 155}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 156, "union");
        atStep(st, 157);
        cqWorkplaneNew(c, st, 157, {"plane": {"origin": [3.8042388024623808, -0.9885062046092826, 4.377436514244266], "xDir": [-0.7809159479404655, 0, -0.6246361198747991], "zDir": [0.5738723865434513, -0.394884642170974, -0.7174514641008306]}, "obj": undefined});
        atStep(st, 158);
        cqCircle(c, st, 158, 157, {"radius": 0.2, "forConstruction": false});
        atStep(st, 159);
        cqExtrude(c, st, 159, 158, {"until": 3.365820605350413, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 159, "extrude");
        atStep(st, 160);
        cqUnion(c, st, 160, 156, {"toUnion": {"wp": 159}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 160, "union");
        atStep(st, 161);
        cqWorkplaneNew(c, st, 161, {"plane": {"origin": [2.6508821119146155, -0.2531861036527454, 5.058785760868975], "xDir": [-0.9419100257136386, 0, 0.335865305532043], "zDir": [-0.3332013319648859, 0.12569977459199697, -0.93443910397861]}, "obj": undefined});
        atStep(st, 162);
        cqCircle(c, st, 162, 161, {"radius": 0.2, "forConstruction": false});
        atStep(st, 163);
        cqExtrude(c, st, 163, 162, {"until": 1.1259315063767534, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 163, "extrude");
        atStep(st, 164);
        cqUnion(c, st, 164, 160, {"toUnion": {"wp": 163}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 164, "union");
        atStep(st, 165);
        cqWorkplaneNew(c, st, 165, {"plane": {"origin": [2.737475952373167, 2.4179867614376884, 3.9205145142074858], "xDir": [0, 0.5188826098087224, 0.8548455048943575], "zDir": [-0.03146884655712053, 0.8544221282997166, -0.5186256244808457]}, "obj": undefined});
        atStep(st, 166);
        cqCircle(c, st, 166, 165, {"radius": 0.2, "forConstruction": false});
        atStep(st, 167);
        cqExtrude(c, st, 167, 166, {"until": 6.418226875340013, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 167, "extrude");
        atStep(st, 168);
        cqUnion(c, st, 168, 164, {"toUnion": {"wp": 167}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 168, "union");
        atStep(st, 169);
        cqWorkplaneNew(c, st, 169, {"plane": {"origin": [1.2888454073145144, 2.0845308445211694, 5.772302987806661], "xDir": [0.8409706604754539, 0.5410807224615186, -6.9388939039072276e-18], "zDir": [-0.5399253245567781, 0.8391748919353343, 0.06531573047572806]}, "obj": undefined});
        atStep(st, 170);
        cqCircle(c, st, 170, 169, {"radius": 0.2, "forConstruction": false});
        atStep(st, 171);
        cqExtrude(c, st, 171, 170, {"until": 5.740118393909961, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 171, "extrude");
        atStep(st, 172);
        cqUnion(c, st, 172, 168, {"toUnion": {"wp": 171}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 172, "union");
        atStep(st, 173);
        cqWorkplaneNew(c, st, 173, {"plane": {"origin": [1.7909525981333274, 0.6226785361083349, 4.943712498604077], "xDir": [-0.6704776910644165, -0.7419296905940137, 2.7755575615628914e-17], "zDir": [-0.6755414189546656, 0.6104829831469185, -0.41346622421276]}, "obj": undefined});
        atStep(st, 174);
        cqCircle(c, st, 174, 173, {"radius": 0.2, "forConstruction": false});
        atStep(st, 175);
        cqExtrude(c, st, 175, 174, {"until": 3.10124715732513, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 175, "extrude");
        atStep(st, 176);
        cqUnion(c, st, 176, 172, {"toUnion": {"wp": 175}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 176, "union");
        atStep(st, 177);
        cqWorkplaneNew(c, st, 177, {"plane": {"origin": [3.640554959773472, -0.4352708065512373, 5.384209753566416], "xDir": [0.24266111714905717, 3.469446951953614e-18, 0.9701111184931196], "zDir": [0.9614359724899402, -0.13343493998027986, -0.24049113828742671]}, "obj": undefined});
        atStep(st, 178);
        cqCircle(c, st, 178, 177, {"radius": 0.2, "forConstruction": false});
        atStep(st, 179);
        cqExtrude(c, st, 179, 178, {"until": 1.6685290170048557, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 179, "extrude");
        atStep(st, 180);
        cqUnion(c, st, 180, 176, {"toUnion": {"wp": 179}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 180, "union");
        atStep(st, 181);
        cqWorkplaneNew(c, st, 181, {"plane": {"origin": [3.4307299372897617, -4.898301872396665, 4.370011480731004], "xDir": [0.0554571258538008, -0.9984610694423872, 4.336808689942018e-19], "zDir": [-0.9969909507491812, -0.05537547163624108, 0.05424574882244512]}, "obj": undefined});
        atStep(st, 182);
        cqCircle(c, st, 182, 181, {"radius": 0.2, "forConstruction": false});
        atStep(st, 183);
        cqExtrude(c, st, 183, 182, {"until": 2.994983179694479, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 183, "extrude");
        atStep(st, 184);
        cqUnion(c, st, 184, 180, {"toUnion": {"wp": 183}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 184, "union");
        atStep(st, 185);
        cqWorkplaneNew(c, st, 185, {"plane": {"origin": [5.418796656151927, -2.653093556070528, 4.993288883674565], "xDir": [-6.938893903907231e-18, -0.3097890077758613, 0.9508053274257814], "zDir": [0.21271617449521393, 0.9290452259491196, 0.3026991860730235]}, "obj": undefined});
        atStep(st, 186);
        cqCircle(c, st, 186, 185, {"radius": 0.2, "forConstruction": false});
        atStep(st, 187);
        cqExtrude(c, st, 187, 186, {"until": 4.654851998314799, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 187, "extrude");
        atStep(st, 188);
        cqUnion(c, st, 188, 184, {"toUnion": {"wp": 187}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 188, "union");
        atStep(st, 189);
        cqWorkplaneNew(c, st, 189, {"plane": {"origin": [5.378142241225302, -3.717084803145682, 2.506395647528622], "xDir": [-1.3877787807814457e-17, -0.8513508405711013, -0.5245967463289105], "zDir": [0.21211653739139555, 0.5126592211294184, -0.8319778227549476]}, "obj": undefined});
        atStep(st, 190);
        cqCircle(c, st, 190, 189, {"radius": 0.2, "forConstruction": false});
        atStep(st, 191);
        cqExtrude(c, st, 191, 190, {"until": 4.284689403507793, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 191, "extrude");
        atStep(st, 192);
        cqUnion(c, st, 192, 188, {"toUnion": {"wp": 191}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 192, "union");
        atStep(st, 193);
        cqWorkplaneNew(c, st, 193, {"plane": {"origin": [4.991841439381714, -4.980837191794651, 2.5844190021451805], "xDir": [0, -0.9953207652146654, 0.09662595061624396], "zDir": [0.03975308303713461, -0.0965495710659292, -0.9945339977678128]}, "obj": undefined});
        atStep(st, 194);
        cqCircle(c, st, 194, 193, {"radius": 0.2, "forConstruction": false});
        atStep(st, 195);
        cqExtrude(c, st, 195, 194, {"until": 3.4274543248689464, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 195, "extrude");
        atStep(st, 196);
        cqUnion(c, st, 196, 192, {"toUnion": {"wp": 195}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 196, "union");
        atStep(st, 197);
        cqWorkplaneNew(c, st, 197, {"plane": {"origin": [4.846865027693814, -3.2342196033122983, 3.729404490861997], "xDir": [0, 0.333519181448678, 0.9427433137422954], "zDir": [-0.045772993022983544, 0.9417551936132742, -0.3331696090764318]}, "obj": undefined});
        atStep(st, 198);
        cqCircle(c, st, 198, 197, {"radius": 0.2, "forConstruction": false});
        atStep(st, 199);
        cqExtrude(c, st, 199, 198, {"until": 3.3578959303824427, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 199, "extrude");
        atStep(st, 200);
        cqUnion(c, st, 200, 196, {"toUnion": {"wp": 199}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 200, "union");
        atStep(st, 201);
        cqWorkplaneNew(c, st, 201, {"plane": {"origin": [4.683181185004905, -2.680984205254253, 4.736177730184147], "xDir": [0, -0.20515541890709224, 0.9787294080046108], "zDir": [-0.1096325339862054, 0.972829809644616, 0.20391878029888297]}, "obj": undefined});
        atStep(st, 202);
        cqCircle(c, st, 202, 201, {"radius": 0.2, "forConstruction": false});
        atStep(st, 203);
        cqExtrude(c, st, 203, 202, {"until": 4.388009789426644, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 203, "extrude");
        atStep(st, 204);
        cqUnion(c, st, 204, 200, {"toUnion": {"wp": 203}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 204, "union");
        atStep(st, 205);
        cqWorkplaneNew(c, st, 205, {"plane": {"origin": [6.310358296611661, -0.6095385798209317, 5.631559904671344], "xDir": [0.28687102269902576, 0.9579692147118376, 0], "zDir": [0.9459310326790452, -0.28326609934851593, -0.15803416837706974]}, "obj": undefined});
        atStep(st, 206);
        cqCircle(c, st, 206, 205, {"radius": 0.2, "forConstruction": false});
        atStep(st, 207);
        cqExtrude(c, st, 207, 206, {"until": 0.8382862424449021, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 207, "extrude");
        atStep(st, 208);
        cqUnion(c, st, 208, 204, {"toUnion": {"wp": 207}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 208, "union");
        atStep(st, 209);
        cqWorkplaneNew(c, st, 209, {"plane": {"origin": [2.4703432584680955, -1.7865254121309742, 6.9432297179553135], "xDir": [0.3521693971713028, -0.9359362775830422, 0], "zDir": [-0.886522847746106, -0.33357636021500603, 0.3206307102101158]}, "obj": undefined});
        atStep(st, 210);
        cqCircle(c, st, 210, 209, {"radius": 0.2, "forConstruction": false});
        atStep(st, 211);
        cqExtrude(c, st, 211, 210, {"until": 7.768631257276968, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 211, "extrude");
        atStep(st, 212);
        cqUnion(c, st, 212, 208, {"toUnion": {"wp": 211}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 212, "union");
        atStep(st, 213);
        cqWorkplaneNew(c, st, 213, {"plane": {"origin": [2.8265527875074103, 2.001101459085195, 5.82878092001453], "xDir": [-0.6280781149555977, -0.7781502949391095, -3.4694469519536138e-18], "zDir": [-0.7777265888527053, 0.6277361237981377, 0.03299563411156846]}, "obj": undefined});
        atStep(st, 214);
        cqCircle(c, st, 214, 213, {"radius": 0.2, "forConstruction": false});
        atStep(st, 215);
        cqExtrude(c, st, 215, 214, {"until": 7.939358298551608, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 215, "extrude");
        atStep(st, 216);
        cqUnion(c, st, 216, 212, {"toUnion": {"wp": 215}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 216, "union");
        atStep(st, 217);
        cqWorkplaneNew(c, st, 217, {"plane": {"origin": [4.70848934454952, 3.800232492604029, 3.012663588884038], "xDir": [0, 0.5304582845857025, 0.8477110405759701], "zDir": [-0.23165151486238855, 0.8246523136843597, -0.5160292018839565]}, "obj": undefined});
        atStep(st, 218);
        cqCircle(c, st, 218, 217, {"radius": 0.2, "forConstruction": false});
        atStep(st, 219);
        cqExtrude(c, st, 219, 218, {"until": 10.406912014194472, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 219, "extrude");
        atStep(st, 220);
        cqUnion(c, st, 220, 216, {"toUnion": {"wp": 219}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 220, "union");
        atStep(st, 221);
        cqWorkplaneNew(c, st, 221, {"plane": {"origin": [5.341946182655277, -1.0719355900452563, 4.433914446452135], "xDir": [-2.7755575615628914e-17, -0.9085616114715942, 0.4177508805019326], "zDir": [-0.3802567925094892, -0.3863698312394649, -0.8403113263894416]}, "obj": undefined});
        atStep(st, 222);
        cqCircle(c, st, 222, 221, {"radius": 0.2, "forConstruction": false});
        atStep(st, 223);
        cqExtrude(c, st, 223, 222, {"until": 3.0081336598022292, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 223, "extrude");
        atStep(st, 224);
        cqUnion(c, st, 224, 220, {"toUnion": {"wp": 223}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 224, "union");
        atStep(st, 225);
        cqWorkplaneNew(c, st, 225, {"plane": {"origin": [6.572299068224532, -0.19184646603803396, 4.271696537128392], "xDir": [-0.9079060390381394, 0, -0.4191737399671844], "zDir": [0.4117815740536084, 0.18697374212146212, -0.8918950358798]}, "obj": undefined});
        atStep(st, 226);
        cqCircle(c, st, 226, 225, {"radius": 0.2, "forConstruction": false});
        atStep(st, 227);
        cqExtrude(c, st, 227, 226, {"until": 3.1979151015893894, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 227, "extrude");
        atStep(st, 228);
        cqUnion(c, st, 228, 224, {"toUnion": {"wp": 227}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 228, "union");
        atStep(st, 229);
        cqWorkplaneNew(c, st, 229, {"plane": {"origin": [4.275183332566063, 2.3345573760017144, 3.9769924464153545], "xDir": [0, 0.5201714251001851, 0.8540618762766796], "zDir": [-0.443877292381943, 0.7653142390621325, -0.4661191530032041]}, "obj": undefined});
        atStep(st, 230);
        cqCircle(c, st, 230, 229, {"radius": 0.2, "forConstruction": false});
        atStep(st, 231);
        cqExtrude(c, st, 231, 230, {"until": 7.383547240065977, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 231, "extrude");
        atStep(st, 232);
        cqUnion(c, st, 232, 228, {"toUnion": {"wp": 231}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 232, "union");
        atStep(st, 233);
        cqWorkplaneNew(c, st, 233, {"plane": {"origin": [5.416562877374633, 1.133988563167657, 3.2997125204190425], "xDir": [0, -0.827872518436782, -0.5609162978708501], "zDir": [-0.1692084940451353, 0.552828041120494, -0.8159348273568513]}, "obj": undefined});
        atStep(st, 234);
        cqCircle(c, st, 234, 233, {"radius": 0.2, "forConstruction": false});
        atStep(st, 235);
        cqExtrude(c, st, 235, 234, {"until": 5.8781320233971375, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 235, "extrude");
        atStep(st, 236);
        cqUnion(c, st, 236, 232, {"toUnion": {"wp": 235}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 236, "union");
        atStep(st, 237);
        cqWorkplaneNew(c, st, 237, {"plane": {"origin": [5.178262339966368, -0.5187001919872111, 5.440687685774285], "xDir": [0.3299453909358568, 0, -0.9440000206568772], "zDir": [-0.9433959544523935, -0.03576858649526499, -0.3297342586735388]}, "obj": undefined});
        atStep(st, 238);
        cqCircle(c, st, 238, 237, {"radius": 0.2, "forConstruction": false});
        atStep(st, 239);
        cqExtrude(c, st, 239, 238, {"until": 1.5595052484065826, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 239, "extrude");
        atStep(st, 240);
        cqUnion(c, st, 240, 236, {"toUnion": {"wp": 239}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 240, "union");
        atStep(st, 241);
        cqWorkplaneNew(c, st, 241, {"plane": {"origin": [4.642941600658439, -3.010163699998785, 1.2998314498637242], "xDir": [6.9388939039072276e-18, 0.1928226138437391, -0.9812336315019315], "zDir": [0.18814010606556464, -0.9637109572919412, -0.18937922611809532]}, "obj": undefined});
        atStep(st, 242);
        cqCircle(c, st, 242, 241, {"radius": 0.2, "forConstruction": false});
        atStep(st, 243);
        cqExtrude(c, st, 243, 242, {"until": 4.433140659219126, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 243, "extrude");
        atStep(st, 244);
        cqUnion(c, st, 244, 240, {"toUnion": {"wp": 243}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 244, "union");
        atStep(st, 245);
        cqWorkplaneNew(c, st, 245, {"plane": {"origin": [4.2209580335506445, -1.0764073691814229, 1.0654361328186324], "xDir": [0, -0.955328653806766, 0.29554553492778834], "zDir": [-0.007240031818364429, -0.29553778886437926, -0.9553036152409468]}, "obj": undefined});
        atStep(st, 246);
        cqCircle(c, st, 246, 245, {"radius": 0.2, "forConstruction": false});
        atStep(st, 247);
        cqExtrude(c, st, 247, 246, {"until": 1.3695492831101412, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 247, "extrude");
        atStep(st, 248);
        cqUnion(c, st, 248, 244, {"toUnion": {"wp": 247}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 248, "union");
        atStep(st, 249);
        cqWorkplaneNew(c, st, 249, {"plane": {"origin": [2.753159749733722, -2.955174745157177, 0.899838780981612], "xDir": [-0.8162811320057948, 0.577654839442498, -2.7755575615628907e-17], "zDir": [-0.5499270024289812, -0.7770990657613405, -0.3061002025355869]}, "obj": undefined});
        atStep(st, 250);
        cqCircle(c, st, 250, 249, {"radius": 0.2, "forConstruction": false});
        atStep(st, 251);
        cqExtrude(c, st, 251, 250, {"until": 5.356187521271554, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 251, "extrude");
        atStep(st, 252);
        cqUnion(c, st, 252, 248, {"toUnion": {"wp": 251}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 252, "union");
        atStep(st, 253);
        cqWorkplaneNew(c, st, 253, {"plane": {"origin": [4.497965188970538, -1.2635461115164333, 2.4448169385805407], "xDir": [0, 0.8809695041779042, 0.4731730473183543], "zDir": [0.3137869649323537, -0.4492746914655264, 0.8364747409498376]}, "obj": undefined});
        atStep(st, 254);
        cqCircle(c, st, 254, 253, {"radius": 0.2, "forConstruction": false});
        atStep(st, 255);
        cqExtrude(c, st, 255, 254, {"until": 1.7339749296805518, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 255, "extrude");
        atStep(st, 256);
        cqUnion(c, st, 256, 252, {"toUnion": {"wp": 255}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 256, "union");
        atStep(st, 257);
        cqWorkplaneNew(c, st, 257, {"plane": {"origin": [0.4872433423442446, -1.4280001728749507, 2.535823595135909], "xDir": [-0.2132941437452903, 3.469446951953614e-18, -0.9769880287106714], "zDir": [-0.9669091198666132, -0.14326963610445767, 0.21109373578879032]}, "obj": undefined});
        atStep(st, 258);
        cqCircle(c, st, 258, 257, {"radius": 0.2, "forConstruction": false});
        atStep(st, 259);
        cqExtrude(c, st, 259, 258, {"until": 7.733244840870719, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 259, "extrude");
        atStep(st, 260);
        cqUnion(c, st, 260, 256, {"toUnion": {"wp": 259}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 260, "union");
        atStep(st, 261);
        cqWorkplaneNew(c, st, 261, {"plane": {"origin": [3.3446084984227724, -0.528226010559896, 3.12616618520525], "xDir": [0.8474004888807158, -1.3877787807814457e-17, 0.5309542460935064], "zDir": [-0.5197937265318974, 0.20395501503653476, 0.8295883519550706]}, "obj": undefined});
        atStep(st, 262);
        cqCircle(c, st, 262, 261, {"radius": 0.2, "forConstruction": false});
        atStep(st, 263);
        cqExtrude(c, st, 263, 262, {"until": 3.3909886954630553, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 263, "extrude");
        atStep(st, 264);
        cqUnion(c, st, 264, 260, {"toUnion": {"wp": 263}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 264, "union");
        atStep(st, 265);
        cqWorkplaneNew(c, st, 265, {"plane": {"origin": [3.0818300985664857, -2.9276283806008, 3.085423928449548], "xDir": [0, -0.5537889507913978, -0.8326570710570848], "zDir": [-0.42081145952059423, -0.7553432984116397, 0.5023686067826811]}, "obj": undefined});
        atStep(st, 266);
        cqCircle(c, st, 266, 265, {"radius": 0.2, "forConstruction": false});
        atStep(st, 267);
        cqExtrude(c, st, 267, 266, {"until": 5.437521718067354, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 267, "extrude");
        atStep(st, 268);
        cqUnion(c, st, 268, 264, {"toUnion": {"wp": 267}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 268, "union");
        atStep(st, 269);
        cqWorkplaneNew(c, st, 269, {"plane": {"origin": [2.4846789846414845, 0.3476386292011843, 3.0110929229403514], "xDir": [-0.595729017080591, 0, -0.8031854942715243], "zDir": [-0.6997307483515209, 0.4909381049240541, 0.5189958159224398]}, "obj": undefined});
        atStep(st, 270);
        cqCircle(c, st, 270, 269, {"radius": 0.2, "forConstruction": false});
        atStep(st, 271);
        cqExtrude(c, st, 271, 270, {"until": 4.976876729240686, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 271, "extrude");
        atStep(st, 272);
        cqUnion(c, st, 272, 268, {"toUnion": {"wp": 271}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 272, "union");
        atStep(st, 273);
        cqWorkplaneNew(c, st, 273, {"plane": {"origin": [4.572581883689894, 0.9423780416964802, 1.310615012547448], "xDir": [0, 0.21966395538155023, 0.975575597637688], "zDir": [0.1830452144942689, 0.95909275592548, -0.21595262207719235]}, "obj": undefined});
        atStep(st, 274);
        cqCircle(c, st, 274, 273, {"radius": 0.2, "forConstruction": false});
        atStep(st, 275);
        cqExtrude(c, st, 275, 274, {"until": 3.787764251622502, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 275, "extrude");
        atStep(st, 276);
        cqUnion(c, st, 276, 272, {"toUnion": {"wp": 275}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 276, "union");
        atStep(st, 277);
        cqWorkplaneNew(c, st, 277, {"plane": {"origin": [2.3886474051503237, 1.0312698463784464, 1.9534622110391104], "xDir": [0.7198415712281537, 0.6941383956616888, -1.3877787807814457e-17], "zDir": [-0.6914447825153405, 0.7170482165719835, 0.08801118021735338]}, "obj": undefined});
        atStep(st, 278);
        cqCircle(c, st, 278, 277, {"radius": 0.2, "forConstruction": false});
        atStep(st, 279);
        cqExtrude(c, st, 279, 278, {"until": 5.314288183388893, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 279, "extrude");
        atStep(st, 280);
        cqUnion(c, st, 280, 276, {"toUnion": {"wp": 279}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 280, "union");
        atStep(st, 281);
        cqWorkplaneNew(c, st, 281, {"plane": {"origin": [5.728318074539793, -0.3834569875092107, 2.282599029256798], "xDir": [-0.35090177704567954, -1.3877787807814457e-17, 0.93641227184728], "zDir": [0.8954873972568546, 0.29240003761651234, 0.3355659985099899]}, "obj": undefined});
        atStep(st, 282);
        cqCircle(c, st, 282, 281, {"radius": 0.2, "forConstruction": false});
        atStep(st, 283);
        cqExtrude(c, st, 283, 282, {"until": 3.355496136289998, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 283, "extrude");
        atStep(st, 284);
        cqUnion(c, st, 284, 280, {"toUnion": {"wp": 283}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 284, "union");
        atStep(st, 285);
        cqWorkplaneNew(c, st, 285, {"plane": {"origin": [4.334281346281628, -0.7103107134583879, 3.4515901779026903], "xDir": [0, 0.9955620206880488, -0.09410771999963052], "zDir": [0.06216902929298307, 0.09392568132119192, 0.993636240374272]}, "obj": undefined});
        atStep(st, 286);
        cqCircle(c, st, 286, 285, {"radius": 0.2, "forConstruction": false});
        atStep(st, 287);
        cqExtrude(c, st, 287, 286, {"until": 3.486157778884534, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 287, "extrude");
        atStep(st, 288);
        cqUnion(c, st, 288, 284, {"toUnion": {"wp": 287}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 288, "union");
        atStep(st, 289);
        cqWorkplaneNew(c, st, 289, {"plane": {"origin": [2.0236810840488424, 0.9934889029853728, 0.96232350226972], "xDir": [-0.6467673406599266, -0.7626873586573247, 0], "zDir": [-0.7377374672371098, 0.6256095559655344, -0.2536850664094783]}, "obj": undefined});
        atStep(st, 290);
        cqCircle(c, st, 290, 289, {"radius": 0.2, "forConstruction": false});
        atStep(st, 291);
        cqExtrude(c, st, 291, 290, {"until": 5.9702396516273675, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 291, "extrude");
        atStep(st, 292);
        cqUnion(c, st, 292, 288, {"toUnion": {"wp": 291}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 292, "union");
        atStep(st, 293);
        cqWorkplaneNew(c, st, 293, {"plane": {"origin": [5.029242402502026, -1.746411311349816, 1.2218080952471668], "xDir": [-0.735621569467819, -0.677392726956603, -2.7755575615628907e-17], "zDir": [0.6245978000031769, -0.6782883778347971, -0.38704323366444615]}, "obj": undefined});
        atStep(st, 294);
        cqCircle(c, st, 294, 293, {"radius": 0.2, "forConstruction": false});
        atStep(st, 295);
        cqExtrude(c, st, 295, 294, {"until": 2.5723003787526264, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 295, "extrude");
        atStep(st, 296);
        cqUnion(c, st, 296, 292, {"toUnion": {"wp": 295}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 296, "union");
        atStep(st, 297);
        cqWorkplaneNew(c, st, 297, {"plane": {"origin": [3.4988558754810124, -5.063761494853745, 2.665651554791758], "xDir": [0.752844482852262, 3.469446951953614e-18, 0.6581984386481863], "zDir": [-0.6578002781595942, 0.03477762862244772, 0.7523890686344131]}, "obj": undefined});
        atStep(st, 298);
        cqCircle(c, st, 298, 297, {"radius": 0.2, "forConstruction": false});
        atStep(st, 299);
        cqExtrude(c, st, 299, 298, {"until": 4.746460449848576, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 299, "extrude");
        atStep(st, 300);
        cqUnion(c, st, 300, 296, {"toUnion": {"wp": 299}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 300, "union");
        atStep(st, 301);
        cqWorkplaneNew(c, st, 301, {"plane": {"origin": [4.637983810465171, -3.2125404834343683, 0.6456637591608418], "xDir": [0.9770080693748656, 0.2132023273240646, 0], "zDir": [-0.2117228319599596, 0.9702282235473098, -0.11760372723296694]}, "obj": undefined});
        atStep(st, 302);
        cqCircle(c, st, 302, 301, {"radius": 0.2, "forConstruction": false});
        atStep(st, 303);
        cqExtrude(c, st, 303, 302, {"until": 3.9861885768427543, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 303, "extrude");
        atStep(st, 304);
        cqUnion(c, st, 304, 300, {"toUnion": {"wp": 303}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 304, "union");
        atStep(st, 305);
        cqWorkplaneNew(c, st, 305, {"plane": {"origin": [4.914990965885064, -3.399679225769378, 2.02504456492275], "xDir": [0, -0.5482439412100304, 0.8363184686030154], "zDir": [-0.06925120268620612, 0.8343106801103032, 0.5469277465812075]}, "obj": undefined});
        atStep(st, 306);
        cqCircle(c, st, 306, 305, {"radius": 0.2, "forConstruction": false});
        atStep(st, 307);
        cqExtrude(c, st, 307, 306, {"until": 4.186971664443835, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 307, "extrude");
        atStep(st, 308);
        cqUnion(c, st, 308, 304, {"toUnion": {"wp": 307}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 308, "union");
        atStep(st, 309);
        cqWorkplaneNew(c, st, 309, {"plane": {"origin": [0.4822855521509768, -1.630376956310534, 1.881655904433027], "xDir": [-0.3664231604067522, 0, -0.930448315339185], "zDir": [-0.9268972986467132, -0.08728316085176202, 0.365024721893122]}, "obj": undefined});
        atStep(st, 310);
        cqCircle(c, st, 310, 309, {"radius": 0.2, "forConstruction": false});
        atStep(st, 311);
        cqExtrude(c, st, 311, 310, {"until": 8.056371933886721, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 311, "extrude");
        atStep(st, 312);
        cqUnion(c, st, 312, 308, {"toUnion": {"wp": 311}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 312, "union");
        atStep(st, 313);
        cqWorkplaneNew(c, st, 313, {"plane": {"origin": [2.0187232938555746, 0.7911121195497893, 0.30815581156683763], "xDir": [-0.6856932802868194, -0.7278905998634009, 0], "zDir": [-0.7274663295634831, 0.6852936058113783, -0.034138148536105216]}, "obj": undefined});
        atStep(st, 314);
        cqCircle(c, st, 314, 313, {"radius": 0.2, "forConstruction": false});
        atStep(st, 315);
        cqExtrude(c, st, 315, 314, {"until": 6.0409035035897265, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 315, "extrude");
        atStep(st, 316);
        cqUnion(c, st, 316, 312, {"toUnion": {"wp": 315}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 316, "union");
        atStep(st, 317);
        cqWorkplaneNew(c, st, 317, {"plane": {"origin": [2.2809249766591013, 1.957089292434793, 0.21249321941311575], "xDir": [0.8582465948058078, 0.5132375497800563, 3.4694469519536138e-18], "zDir": [-0.5125257630960764, 0.8570563301846483, -0.05264778298473557]}, "obj": undefined});
        atStep(st, 318);
        cqCircle(c, st, 318, 317, {"radius": 0.2, "forConstruction": false});
        atStep(st, 319);
        cqExtrude(c, st, 319, 318, {"until": 7.551133644517034, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 319, "extrude");
        atStep(st, 320);
        cqUnion(c, st, 320, 316, {"toUnion": {"wp": 319}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 320, "union");
        atStep(st, 321);
        cqWorkplaneNew(c, st, 321, {"plane": {"origin": [4.567624093496626, 0.7400012582608967, 0.6564473218445656], "xDir": [0.9851680018186432, -0.17159256450284205, 0], "zDir": [0.1703773797136128, 0.9781912358141608, 0.11880006169311812]}, "obj": undefined});
        atStep(st, 322);
        cqCircle(c, st, 322, 321, {"radius": 0.2, "forConstruction": false});
        atStep(st, 323);
        cqExtrude(c, st, 323, 322, {"until": 4.127588424358849, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 323, "extrude");
        atStep(st, 324);
        cqUnion(c, st, 324, 320, {"toUnion": {"wp": 323}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 324, "union");
        atStep(st, 325);
        cqWorkplaneNew(c, st, 325, {"plane": {"origin": [5.7233602843465246, -0.5858337709447942, 1.6284313385539155], "xDir": [-0.6282370769898006, 0, 0.7780219631188514], "zDir": [0.7325745497893548, 0.33677276833394365, 0.5915392053867741]}, "obj": undefined});
        atStep(st, 326);
        cqCircle(c, st, 326, 325, {"radius": 0.2, "forConstruction": false});
        atStep(st, 327);
        cqExtrude(c, st, 327, 326, {"until": 4.115239988674399, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 327, "extrude");
        atStep(st, 328);
        cqUnion(c, st, 328, 324, {"toUnion": {"wp": 327}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 328, "union");
        atStep(st, 329);
        cqWorkplaneNew(c, st, 329, {"plane": {"origin": [4.493007398777269, -1.4659228949520169, 1.7906492478776586], "xDir": [0.98042577883119, 0, -0.1968890352540104], "zDir": [0.19517008448139267, -0.13185177146742594, 0.9718661165430288]}, "obj": undefined});
        atStep(st, 330);
        cqCircle(c, st, 330, 329, {"radius": 0.2, "forConstruction": false});
        atStep(st, 331);
        cqExtrude(c, st, 331, 330, {"until": 2.838623103084256, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 331, "extrude");
        atStep(st, 332);
        cqUnion(c, st, 332, 328, {"toUnion": {"wp": 331}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 332, "union");
        atStep(st, 333);
        cqWorkplaneNew(c, st, 333, {"plane": {"origin": [-0.9855127316659452, -3.509144332286288, 1.7160585525960066], "xDir": [-0.5853705328696045, 0, -0.8107658966976568], "zDir": [-0.7114736785805792, 0.4795165908296445, 0.5136818507657576]}, "obj": undefined});
        atStep(st, 334);
        cqCircle(c, st, 334, 333, {"radius": 0.2, "forConstruction": false});
        atStep(st, 335);
        cqExtrude(c, st, 335, 334, {"until": 6.369642266766857, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 335, "extrude");
        atStep(st, 336);
        cqUnion(c, st, 336, 332, {"toUnion": {"wp": 335}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 336, "union");
        atStep(st, 337);
        cqWorkplaneNew(c, st, 337, {"plane": {"origin": [0.5509250100386526, -1.0876552564259645, 0.1425584597298173], "xDir": [0.9833601826383056, 0.18166659352109352, 0], "zDir": [-0.18164460283172532, 0.9832411471684688, 0.01555907378151129]}, "obj": undefined});
        atStep(st, 338);
        cqCircle(c, st, 338, 337, {"radius": 0.2, "forConstruction": false});
        atStep(st, 339);
        cqExtrude(c, st, 339, 338, {"until": 8.031933284146794, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 339, "extrude");
        atStep(st, 340);
        cqUnion(c, st, 340, 336, {"toUnion": {"wp": 339}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 340, "union");
        atStep(st, 341);
        cqWorkplaneNew(c, st, 341, {"plane": {"origin": [1.6090740245562958, -5.008772540012137, 2.265658885909646], "xDir": [0.9888810220006226, 2.1684043449710093e-19, -0.14870885759430824], "zDir": [0.14869730877855744, 0.01246254884480148, 0.9888042249294372]}, "obj": undefined});
        atStep(st, 342);
        cqCircle(c, st, 342, 341, {"radius": 0.2, "forConstruction": false});
        atStep(st, 343);
        cqExtrude(c, st, 343, 342, {"until": 4.4206630440396895, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 343, "extrude");
        atStep(st, 344);
        cqUnion(c, st, 344, 340, {"toUnion": {"wp": 343}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 344, "union");
        atStep(st, 345);
        cqWorkplaneNew(c, st, 345, {"plane": {"origin": [1.871852424412583, -2.6093701699712333, 2.3064011426653472], "xDir": [0, -0.6759926005467789, 0.7369084095096237], "zDir": [0.17675732485726017, 0.7253053909471728, 0.6653487341028566]}, "obj": undefined});
        atStep(st, 346);
        cqCircle(c, st, 346, 345, {"radius": 0.2, "forConstruction": false});
        atStep(st, 347);
        cqExtrude(c, st, 347, 346, {"until": 6.692212038925948, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 347, "extrude");
        atStep(st, 348);
        cqUnion(c, st, 348, 344, {"toUnion": {"wp": 347}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 348, "union");
        atStep(st, 349);
        cqWorkplaneNew(c, st, 349, {"plane": {"origin": [-2.233679437020594, -3.685458963274872, 0.7496264437712534], "xDir": [-0.3588146741272994, -0.9334088223447002, 0], "zDir": [-0.9189884894977124, 0.35327130780438243, 0.17509865577123815]}, "obj": undefined});
        atStep(st, 350);
        cqCircle(c, st, 350, 349, {"radius": 0.2, "forConstruction": false});
        atStep(st, 351);
        cqExtrude(c, st, 351, 350, {"until": 7.647719537085397, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 351, "extrude");
        atStep(st, 352);
        cqUnion(c, st, 352, 348, {"toUnion": {"wp": 351}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 352, "union");
        atStep(st, 353);
        cqWorkplaneNew(c, st, 353, {"plane": {"origin": [3.0252091149603477, -3.3446902709277704, 1.6250518960406382], "xDir": [-0.6960813817188906, 0.7179628890300112, -2.7755575615628914e-17], "zDir": [0.6058896100325977, 0.5874237838259369, 0.5364989083401435]}, "obj": undefined});
        atStep(st, 354);
        cqCircle(c, st, 354, 353, {"radius": 0.2, "forConstruction": false});
        atStep(st, 355);
        cqExtrude(c, st, 355, 354, {"until": 5.759482949849373, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 355, "extrude");
        atStep(st, 356);
        cqUnion(c, st, 356, 352, {"toUnion": {"wp": 355}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 356, "union");
        atStep(st, 357);
        cqWorkplaneNew(c, st, 357, {"plane": {"origin": [5.301291767728652, -2.13592683712041, 1.9470212103061928], "xDir": [-0.9171977337754796, 0, -0.3984323244380711], "zDir": [0.3746255106637879, -0.3404881998738903, -0.8623940587147698]}, "obj": undefined});
        atStep(st, 358);
        cqCircle(c, st, 358, 357, {"radius": 0.2, "forConstruction": false});
        atStep(st, 359);
        cqExtrude(c, st, 359, 358, {"until": 2.8363109206852157, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 359, "extrude");
        atStep(st, 360);
        cqUnion(c, st, 360, 356, {"toUnion": {"wp": 359}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 360, "union");
        atStep(st, 361);
        cqWorkplaneNew(c, st, 361, {"plane": {"origin": [3.6166578636493982, -0.9177415363304898, 3.851379300264276], "xDir": [-0.5375860386065691, -0.8432089012192034, -2.7755575615628907e-17], "zDir": [-0.7547531537222645, 0.4811912652353372, 0.4458729002838268]}, "obj": undefined});
        atStep(st, 362);
        cqCircle(c, st, 362, 361, {"radius": 0.2, "forConstruction": false});
        atStep(st, 363);
        cqExtrude(c, st, 363, 362, {"until": 3.0562487479772216, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 363, "extrude");
        atStep(st, 364);
        cqUnion(c, st, 364, 360, {"toUnion": {"wp": 363}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 364, "union");
        atStep(st, 365);
        cqWorkplaneNew(c, st, 365, {"plane": {"origin": [3.3538794637931115, -3.3171439063713937, 3.8106370435085744], "xDir": [-0.761563137904399, 0.6480907243474524, 2.7755575615628907e-17], "zDir": [-0.6219148149453056, -0.7308041608771134, 0.28133119520637007]}, "obj": undefined});
        atStep(st, 366);
        cqCircle(c, st, 366, 365, {"radius": 0.2, "forConstruction": false});
        atStep(st, 367);
        cqExtrude(c, st, 367, 366, {"until": 4.554112738184551, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 367, "extrude");
        atStep(st, 368);
        cqUnion(c, st, 368, 364, {"toUnion": {"wp": 367}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 368, "union");
        atStep(st, 369);
        cqWorkplaneNew(c, st, 369, {"plane": {"origin": [5.738426668153548, -1.1906646270627024, 4.367675511858776], "xDir": [0.7775973234510263, 0, -0.628762596344439], "zDir": [0.6022086498980159, 0.2875423337141474, 0.7447577782811207]}, "obj": undefined});
        atStep(st, 370);
        cqCircle(c, st, 370, 369, {"radius": 0.2, "forConstruction": false});
        atStep(st, 371);
        cqExtrude(c, st, 371, 370, {"until": 3.2162012754894347, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 371, "extrude");
        atStep(st, 372);
        cqUnion(c, st, 372, 368, {"toUnion": {"wp": 371}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 372, "union");
        atStep(st, 373);
        cqWorkplaneNew(c, st, 373, {"plane": {"origin": [6.000367439766419, -0.7729725132798045, 3.007812144315824], "xDir": [-0.581792064084544, 0.8133375647099093, 0], "zDir": [0.8087010176038479, 0.5784754752189815, -0.10662452202319432]}, "obj": undefined});
        atStep(st, 374);
        cqCircle(c, st, 374, 373, {"radius": 0.2, "forConstruction": false});
        atStep(st, 375);
        cqExtrude(c, st, 375, 374, {"until": 3.0427880237241363, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 375, "extrude");
        atStep(st, 376);
        cqUnion(c, st, 376, 372, {"toUnion": {"wp": 375}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 376, "union");
        atStep(st, 377);
        cqWorkplaneNew(c, st, 377, {"plane": {"origin": [4.606330711508255, -1.0998262392289817, 4.176803292961717], "xDir": [-6.938893903907228e-18, 0.8763959196436487, -0.4815913122471825], "zDir": [-0.14106199629751726, 0.4767757671772166, 0.8676326302261435]}, "obj": undefined});
        atStep(st, 378);
        cqCircle(c, st, 378, 377, {"radius": 0.2, "forConstruction": false});
        atStep(st, 379);
        cqExtrude(c, st, 379, 378, {"until": 2.32073622924887, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 379, "extrude");
        atStep(st, 380);
        cqUnion(c, st, 380, 376, {"toUnion": {"wp": 379}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 380, "union");
        atStep(st, 381);
        cqWorkplaneNew(c, st, 381, {"plane": {"origin": [-0.6568423828331815, -3.481597967729911, 3.901643700063943], "xDir": [-0.5004110946389073, -0.8657879280529907, -1.3877787807814457e-17], "zDir": [-0.8515851869666125, 0.4922021453297035, -0.18038768658607848]}, "obj": undefined});
        atStep(st, 382);
        cqCircle(c, st, 382, 381, {"radius": 0.2, "forConstruction": false});
        atStep(st, 383);
        cqExtrude(c, st, 383, 382, {"until": 6.093546003223199, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 383, "extrude");
        atStep(st, 384);
        cqUnion(c, st, 384, 380, {"toUnion": {"wp": 383}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 384, "union");
        atStep(st, 385);
        cqWorkplaneNew(c, st, 385, {"plane": {"origin": [0.4822765396059306, -4.0317337284571115, 6.3199523150117525], "xDir": [0.7889371034134277, 2.7755575615628914e-17, 0.6144739594625881], "zDir": [-0.5703555131602964, 0.37207847489292784, 0.7322924259672551]}, "obj": undefined});
        atStep(st, 386);
        cqCircle(c, st, 386, 385, {"radius": 0.2, "forConstruction": false});
        atStep(st, 387);
        cqExtrude(c, st, 387, 386, {"until": 5.103721451620561, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 387, "extrude");
        atStep(st, 388);
        cqUnion(c, st, 388, 384, {"toUnion": {"wp": 387}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 388, "union");
        atStep(st, 389);
        cqWorkplaneNew(c, st, 389, {"plane": {"origin": [2.2005227732453463, -2.581823805414857, 4.491986290133283], "xDir": [0.994056288772108, -0.1088673263776765, 2.1684043449710093e-19], "zDir": [0.10885182105805229, 0.9939147113035072, 0.016876839360839033]}, "obj": undefined});
        atStep(st, 390);
        cqCircle(c, st, 390, 389, {"radius": 0.2, "forConstruction": false});
        atStep(st, 391);
        cqExtrude(c, st, 391, 390, {"until": 4.828185643603396, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 391, "extrude");
        atStep(st, 392);
        cqUnion(c, st, 392, 388, {"toUnion": {"wp": 391}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 392, "union");
        atStep(st, 393);
        cqWorkplaneNew(c, st, 393, {"plane": {"origin": [3.1901956211042024, -2.763908508313349, 4.817410282830724], "xDir": [0.8706999411988012, -0.4918146118166927, 0], "zDir": [0.48680806736825294, 0.8618364429372033, 0.1423230521403158]}, "obj": undefined});
        atStep(st, 394);
        cqCircle(c, st, 394, 393, {"radius": 0.2, "forConstruction": false});
        atStep(st, 395);
        cqExtrude(c, st, 395, 394, {"until": 5.145564881395886, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 395, "extrude");
        atStep(st, 396);
        cqUnion(c, st, 396, 392, {"toUnion": {"wp": 395}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 396, "union");
        atStep(st, 397);
        cqWorkplaneNew(c, st, 397, {"plane": {"origin": [6.074984134485775, 1.4329516399331088, 1.8736102182827312], "xDir": [0.7538065202208708, 0.6570964389437078, 2.7755575615628907e-17], "zDir": [-0.5751200089559805, 0.6597649705078426, -0.48368084414131873]}, "obj": undefined});
        atStep(st, 398);
        cqCircle(c, st, 398, 397, {"radius": 0.2, "forConstruction": false});
        atStep(st, 399);
        cqExtrude(c, st, 399, 398, {"until": 4.019113134136699, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 399, "extrude");
        atStep(st, 400);
        cqUnion(c, st, 400, 396, {"toUnion": {"wp": 399}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 400, "union");
        atStep(st, 401);
        cqWorkplaneNew(c, st, 401, {"plane": {"origin": [6.531644653297907, -1.2558377131131873, 1.7848033009824498], "xDir": [0, 0.614198651331055, -0.7891514535899389], "zDir": [-0.3751954426084435, -0.7315005674533638, -0.5693288150631944]}, "obj": undefined});
        atStep(st, 402);
        cqCircle(c, st, 402, 401, {"radius": 0.2, "forConstruction": false});
        atStep(st, 403);
        cqExtrude(c, st, 403, 402, {"until": 3.726461425957812, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 403, "extrude");
        atStep(st, 404);
        cqUnion(c, st, 404, 400, {"toUnion": {"wp": 403}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 404, "union");
        atStep(st, 405);
        cqWorkplaneNew(c, st, 405, {"plane": {"origin": [4.933604589677205, 2.6335204527671663, 2.550890144279043], "xDir": [0.7398839546122155, 0.6727345194855021, 0], "zDir": [-0.6702428542679958, 0.7371435822048233, -0.0859875311712759]}, "obj": undefined});
        atStep(st, 406);
        cqCircle(c, st, 406, 405, {"radius": 0.2, "forConstruction": false});
        atStep(st, 407);
        cqExtrude(c, st, 407, 406, {"until": 6.854577325310716, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 407, "extrude");
        atStep(st, 408);
        cqUnion(c, st, 408, 404, {"toUnion": {"wp": 407}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 408, "union");
        atStep(st, 409);
        cqWorkplaneNew(c, st, 409, {"plane": {"origin": [5.83668359707751, -0.2197371152217592, 4.014585383637973], "xDir": [-0.64254741891744, 0, -0.7662459229532878], "zDir": [-0.7541713422129019, -0.17682727306215457, 0.6324220917126411]}, "obj": undefined});
        atStep(st, 410);
        cqCircle(c, st, 410, 409, {"radius": 0.2, "forConstruction": false});
        atStep(st, 411);
        cqExtrude(c, st, 411, 410, {"until": 3.6968700618290766, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 411, "extrude");
        atStep(st, 412);
        cqUnion(c, st, 412, 408, {"toUnion": {"wp": 411}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 412, "union");
        atStep(st, 413);
        cqWorkplaneNew(c, st, 413, {"plane": {"origin": [3.4529740209604896, -0.3645061382724445, 4.858152539586426], "xDir": [0.3123662037505586, 6.938893903907228e-18, -0.9499617648908112], "zDir": [-0.9357764594495356, 0.17216858981926106, -0.3077017949569661]}, "obj": undefined});
        atStep(st, 414);
        cqCircle(c, st, 414, 413, {"radius": 0.2, "forConstruction": false});
        atStep(st, 415);
        cqExtrude(c, st, 415, 414, {"until": 2.115190733566913, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 415, "extrude");
        atStep(st, 416);
        cqUnion(c, st, 416, 412, {"toUnion": {"wp": 415}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 416, "union");
        atStep(st, 417);
        cqWorkplaneNew(c, st, 417, {"plane": {"origin": [3.539567861419041, 2.3066667268179892, 3.719881292924936], "xDir": [1.3877787807814457e-17, 0.4564366966078016, 0.8897558889885235], "zDir": [-0.2710710588622919, 0.8564428403535771, -0.439347404970569]}, "obj": undefined});
        atStep(st, 418);
        cqCircle(c, st, 418, 417, {"radius": 0.2, "forConstruction": false});
        atStep(st, 419);
        cqExtrude(c, st, 419, 418, {"until": 6.663042607282411, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 419, "extrude");
        atStep(st, 420);
        cqUnion(c, st, 420, 416, {"toUnion": {"wp": 419}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 420, "union");
        atStep(st, 421);
        cqWorkplaneNew(c, st, 421, {"plane": {"origin": [4.680947406227611, 1.1060979139839318, 3.042601366928624], "xDir": [-6.9388939039072276e-18, -0.7915892172777795, -0.6110536073779061], "zDir": [0.08776756894451135, 0.6086955398399461, -0.7885344593741781]}, "obj": undefined});
        atStep(st, 422);
        cqCircle(c, st, 422, 421, {"radius": 0.2, "forConstruction": false});
        atStep(st, 423);
        cqExtrude(c, st, 423, 422, {"until": 5.430264054799665, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 423, "extrude");
        atStep(st, 424);
        cqUnion(c, st, 424, 420, {"toUnion": {"wp": 423}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 424, "union");
        atStep(st, 425);
        cqWorkplaneNew(c, st, 425, {"plane": {"origin": [5.375908462448009, 0.06999731609250359, 0.8128192842730999], "xDir": [0.9858821072466742, 0.16744094663748568, 0], "zDir": [-0.1673522480957923, 0.9853598556293104, 0.03254504526758388]}, "obj": undefined});
        atStep(st, 426);
        cqCircle(c, st, 426, 425, {"radius": 0.2, "forConstruction": false});
        atStep(st, 427);
        cqExtrude(c, st, 427, 426, {"until": 5.457476956637477, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 427, "extrude");
        atStep(st, 428);
        cqUnion(c, st, 428, 424, {"toUnion": {"wp": 427}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 428, "union");
        atStep(st, 429);
        cqWorkplaneNew(c, st, 429, {"plane": {"origin": [2.632548826798351, 3.9758747033126953, 0.45767209914193147], "xDir": [-0.4698412566270158, -0.8827509238574301, 0], "zDir": [-0.870065587455572, 0.4630895283255136, -0.16891998780737527]}, "obj": undefined});
        atStep(st, 430);
        cqCircle(c, st, 430, 429, {"radius": 0.2, "forConstruction": false});
        atStep(st, 431);
        cqExtrude(c, st, 431, 430, {"until": 5.256383311342699, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 431, "extrude");
        atStep(st, 432);
        cqUnion(c, st, 432, 428, {"toUnion": {"wp": 431}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 432, "union");
        atStep(st, 433);
        cqWorkplaneNew(c, st, 433, {"plane": {"origin": [4.211174410810763, 5.425030598575172, 0.6145772700383767], "xDir": [0.9664983930033668, 0.25667266376049735, 3.4694469519536138e-18], "zDir": [-0.2552943288527874, 0.9613082864536288, -0.10349485036074432]}, "obj": undefined});
        atStep(st, 434);
        cqCircle(c, st, 434, 433, {"radius": 0.2, "forConstruction": false});
        atStep(st, 435);
        cqExtrude(c, st, 435, 434, {"until": 5.54711525326061, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 435, "extrude");
        atStep(st, 436);
        cqUnion(c, st, 436, 432, {"toUnion": {"wp": 435}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 436, "union");
        atStep(st, 437);
        cqWorkplaneNew(c, st, 437, {"plane": {"origin": [2.3703471439948247, 2.8098975304276923, 0.5533346912956534], "xDir": [0.13538572364806323, 0, -0.9907929681988517], "zDir": [-0.9905974844141108, 0.019863578302515977, -0.135359011999406]}, "obj": undefined});
        atStep(st, 438);
        cqCircle(c, st, 438, 437, {"radius": 0.2, "forConstruction": false});
        atStep(st, 439);
        cqExtrude(c, st, 439, 438, {"until": 5.146188718919673, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 439, "extrude");
        atStep(st, 440);
        cqUnion(c, st, 440, 436, {"toUnion": {"wp": 439}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 440, "union");
        atStep(st, 441);
        cqWorkplaneNew(c, st, 441, {"plane": {"origin": [2.831345044587467, 2.164047256643504, 2.602104111966285], "xDir": [-0.6314996745165821, 1.3877787807814457e-17, -0.7753761416792827], "zDir": [-0.7571289720462832, -0.2156682862135023, 0.6166383948554206]}, "obj": undefined});
        atStep(st, 442);
        cqCircle(c, st, 442, 441, {"radius": 0.2, "forConstruction": false});
        atStep(st, 443);
        cqExtrude(c, st, 443, 442, {"until": 5.515316349354484, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 443, "extrude");
        atStep(st, 444);
        cqUnion(c, st, 444, 440, {"toUnion": {"wp": 443}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 444, "union");
        atStep(st, 445);
        cqWorkplaneNew(c, st, 445, {"plane": {"origin": [3.6912745583687543, 1.2881826168824235, 2.7171773742311833], "xDir": [0, 0.7770622870147067, 0.6294237063373715], "zDir": [-0.4652341957253021, -0.5571580798821926, 0.6878459254439323]}, "obj": undefined});
        atStep(st, 446);
        cqCircle(c, st, 446, 445, {"radius": 0.2, "forConstruction": false});
        atStep(st, 447);
        cqExtrude(c, st, 447, 446, {"until": 5.278947233673164, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 447, "extrude");
        atStep(st, 448);
        cqUnion(c, st, 448, 444, {"toUnion": {"wp": 447}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 448, "union");
        atStep(st, 449);
        cqWorkplaneNew(c, st, 449, {"plane": {"origin": [2.735313465096306, 2.847678473820766, 1.5444734000650435], "xDir": [-0.2823739279465661, 0, -0.9593044171773774], "zDir": [-0.9585739725055754, 0.039016449978006136, 0.2821589195223046]}, "obj": undefined});
        atStep(st, 450);
        cqCircle(c, st, 450, 449, {"radius": 0.2, "forConstruction": false});
        atStep(st, 451);
        cqExtrude(c, st, 451, 450, {"until": 4.556632124761489, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 451, "extrude");
        atStep(st, 452);
        cqUnion(c, st, 452, 448, {"toUnion": {"wp": 451}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 452, "union");
        atStep(st, 453);
        cqWorkplaneNew(c, st, 453, {"plane": {"origin": [3.777868398827306, 3.9593554819728576, 1.5789061275696932], "xDir": [0.7247464262024383, 0.6890156875621873, -2.7755575615628914e-17], "zDir": [-0.6377695238151183, 0.6708427565143356, 0.37844422379643383]}, "obj": undefined});
        atStep(st, 454);
        cqCircle(c, st, 454, 453, {"radius": 0.2, "forConstruction": false});
        atStep(st, 455);
        cqExtrude(c, st, 455, 454, {"until": 3.579285312916402, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 455, "extrude");
        atStep(st, 456);
        cqUnion(c, st, 456, 452, {"toUnion": {"wp": 455}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 456, "union");
        atStep(st, 457);
        cqWorkplaneNew(c, st, 457, {"plane": {"origin": [3.560932213205117, 3.7319784546055734, 1.4445448217101189], "xDir": [-0.5824130443913803, -0.8128930100097822, 0], "zDir": [-0.7731086634096609, 0.553908773672876, 0.3090114318421541]}, "obj": undefined});
        atStep(st, 458);
        cqCircle(c, st, 458, 457, {"radius": 0.2, "forConstruction": false});
        atStep(st, 459);
        cqExtrude(c, st, 459, 458, {"until": 3.5139063749205586, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 459, "extrude");
        atStep(st, 460);
        cqUnion(c, st, 460, 456, {"toUnion": {"wp": 459}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 460, "union");
        atStep(st, 461);
        cqWorkplaneNew(c, st, 461, {"plane": {"origin": [-0.3940639829768946, -1.0821955976890074, 3.9423859568196447], "xDir": [0.3003567245173229, -0.9538269434426904, -6.938893903907228e-18], "zDir": [-0.9358288673302608, -0.29468919412734335, -0.1933458298902209]}, "obj": undefined});
        atStep(st, 462);
        cqCircle(c, st, 462, 461, {"radius": 0.2, "forConstruction": false});
        atStep(st, 463);
        cqExtrude(c, st, 463, 462, {"until": 6.106597596695301, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 463, "extrude");
        atStep(st, 464);
        cqUnion(c, st, 464, 460, {"toUnion": {"wp": 463}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 464, "union");
        atStep(st, 465);
        cqWorkplaneNew(c, st, 465, {"plane": {"origin": [1.603371659320345, 0.6934432043871277, 4.417655284624087], "xDir": [0.7135681712054179, 0.7005858013423877, 0], "zDir": [-0.6975271793617714, 0.7104528707112671, -0.0933410550065232]}, "obj": undefined});
        atStep(st, 466);
        cqCircle(c, st, 466, 465, {"radius": 0.2, "forConstruction": false});
        atStep(st, 467);
        cqExtrude(c, st, 467, 466, {"until": 2.4656516311467978, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 467, "extrude");
        atStep(st, 468);
        cqUnion(c, st, 468, 464, {"toUnion": {"wp": 467}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 468, "union");
        atStep(st, 469);
        cqWorkplaneNew(c, st, 469, {"plane": {"origin": [1.5073400798291845, 1.3770744215643898, 3.3600245727228457], "xDir": [2.7755575615628914e-17, 0.601010443095988, 0.7992411696663057], "zDir": [-0.4399643975479464, 0.7177307319356051, -0.5397165231169536]}, "obj": undefined});
        atStep(st, 470);
        cqCircle(c, st, 470, 469, {"radius": 0.2, "forConstruction": false});
        atStep(st, 471);
        cqExtrude(c, st, 471, 470, {"until": 4.345629321828344, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 471, "extrude");
        atStep(st, 472);
        cqUnion(c, st, 472, 468, {"toUnion": {"wp": 471}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 472, "union");
        atStep(st, 473);
        cqWorkplaneNew(c, st, 473, {"plane": {"origin": [2.5498950135601848, 2.4887514297164812, 3.3944573002274954], "xDir": [0, 0.39202223902133154, 0.919955740300968], "zDir": [0.029809788349598805, 0.9195469023186617, -0.39184802022557913]}, "obj": undefined});
        atStep(st, 474);
        cqCircle(c, st, 474, 473, {"radius": 0.2, "forConstruction": false});
        atStep(st, 475);
        cqExtrude(c, st, 475, 474, {"until": 5.809758824384056, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 475, "extrude");
        atStep(st, 476);
        cqUnion(c, st, 476, 472, {"toUnion": {"wp": 475}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 476, "union");
        atStep(st, 477);
        cqWorkplaneNew(c, st, 477, {"plane": {"origin": [2.332958827937995, 2.2613744023491975, 3.2600959943679206], "xDir": [-3.4694469519536138e-18, 0.461883663567432, 0.8869405173570137], "zDir": [-0.047253034750338256, 0.8859497617423462, -0.4613677170929533]}, "obj": undefined});
        atStep(st, 478);
        cqCircle(c, st, 478, 477, {"radius": 0.2, "forConstruction": false});
        atStep(st, 479);
        cqExtrude(c, st, 479, 478, {"until": 5.516781973996084, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 479, "extrude");
        atStep(st, 480);
        cqUnion(c, st, 480, 476, {"toUnion": {"wp": 479}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 480, "union");
        atStep(st, 481);
        cqWorkplaneNew(c, st, 481, {"plane": {"origin": [-5.977099603197051, 1.5245889072584031, 3.9871431530096952], "xDir": [-1.35525271560688e-20, 0.13800277434090183, 0.9904318423163776], "zDir": [0.0006374531397405548, 0.9904316410870984, -0.13800274630242643]}, "obj": undefined});
        atStep(st, 482);
        cqCircle(c, st, 482, 481, {"radius": 0.2, "forConstruction": false});
        atStep(st, 483);
        cqExtrude(c, st, 483, 482, {"until": 3.7316008522414306, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 483, "extrude");
        atStep(st, 484);
        cqUnion(c, st, 484, 480, {"toUnion": {"wp": 483}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 484, "union");
        atStep(st, 485);
        cqWorkplaneNew(c, st, 485, {"plane": {"origin": [-5.1259356979779716, 0.02076013103159266, 5.692277246519304], "xDir": [0.8617272498744065, 0, -0.5073718033394173], "zDir": [0.49705094139038886, 0.20067345393633465, 0.8441981559729861]}, "obj": undefined});
        atStep(st, 486);
        cqCircle(c, st, 486, 485, {"radius": 0.2, "forConstruction": false});
        atStep(st, 487);
        cqExtrude(c, st, 487, 486, {"until": 3.4296414897618828, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 487, "extrude");
        atStep(st, 488);
        cqUnion(c, st, 488, 484, {"toUnion": {"wp": 487}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 488, "union");
        atStep(st, 489);
        cqWorkplaneNew(c, st, 489, {"plane": {"origin": [-7.178897827982301, -1.0760566218605894, 2.3258676733285513], "xDir": [-0.8477235345631378, 0, 0.5304383177597378], "zDir": [-0.5033364766690873, -0.3155567523051228, -0.8044105465053233]}, "obj": undefined});
        atStep(st, 490);
        cqCircle(c, st, 490, 489, {"radius": 0.2, "forConstruction": false});
        atStep(st, 491);
        cqExtrude(c, st, 491, 490, {"until": 4.770601456865267, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 491, "extrude");
        atStep(st, 492);
        cqUnion(c, st, 492, 488, {"toUnion": {"wp": 491}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 492, "union");
        atStep(st, 493);
        cqWorkplaneNew(c, st, 493, {"plane": {"origin": [-5.557252527411549, 1.7333211710782988, 4.591461235193448], "xDir": [0.9796820108151072, -0.20055711826127792, 0], "zDir": [0.19787484285508877, 0.9665796238927346, 0.16300115717519875]}, "obj": undefined});
        atStep(st, 494);
        cqCircle(c, st, 494, 493, {"radius": 0.2, "forConstruction": false});
        atStep(st, 495);
        cqExtrude(c, st, 495, 494, {"until": 4.2555832772901105, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 495, "extrude");
        atStep(st, 496);
        cqUnion(c, st, 496, 492, {"toUnion": {"wp": 495}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 496, "union");
        atStep(st, 497);
        cqWorkplaneNew(c, st, 497, {"plane": {"origin": [-5.8630257566507655, -1.328978946353287, 2.8319039424836943], "xDir": [0, -0.8146785804516643, 0.5799127611574529], "zDir": [0.06632269371816712, -0.5786359244598335, -0.8128848425345869]}, "obj": undefined});
        atStep(st, 498);
        cqCircle(c, st, 498, 497, {"radius": 0.2, "forConstruction": false});
        atStep(st, 499);
        cqExtrude(c, st, 499, 498, {"until": 3.4758300793947643, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 499, "extrude");
        atStep(st, 500);
        cqUnion(c, st, 500, 496, {"toUnion": {"wp": 499}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 500, "union");
        atStep(st, 501);
        cqWorkplaneNew(c, st, 501, {"plane": {"origin": [-4.614859051296117, -1.152664315364703, 3.798336051308448], "xDir": [0.5196687369286783, 0.8543678387316274, 2.7755575615628907e-17], "zDir": [0.8227978785431551, -0.5004663271558715, -0.26932713648645884]}, "obj": undefined});
        atStep(st, 502);
        cqCircle(c, st, 502, 501, {"radius": 0.2, "forConstruction": false});
        atStep(st, 503);
        cqExtrude(c, st, 503, 502, {"until": 3.314130840139699, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 503, "extrude");
        atStep(st, 504);
        cqUnion(c, st, 504, 500, {"toUnion": {"wp": 503}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 504, "union");
        atStep(st, 505);
        cqWorkplaneNew(c, st, 505, {"plane": {"origin": [-6.583348676813509, -1.265940496831779, 2.6130746746232623], "xDir": [0, -0.8658866944858047, 0.5002401746285945], "zDir": [-0.3057368089410309, -0.4762867072599848, -0.824424633393494]}, "obj": undefined});
        atStep(st, 506);
        cqCircle(c, st, 506, 505, {"radius": 0.2, "forConstruction": false});
        atStep(st, 507);
        cqExtrude(c, st, 507, 506, {"until": 3.9580429675603734, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 507, "extrude");
        atStep(st, 508);
        cqUnion(c, st, 508, 504, {"toUnion": {"wp": 507}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 508, "union");
        atStep(st, 509);
        cqWorkplaneNew(c, st, 509, {"plane": {"origin": [-5.402919333423815, 1.480180894901482, 4.014886481672098], "xDir": [0.9526941149469468, -0.3039307870970851, -6.93889390390723e-18], "zDir": [0.30171709654363377, 0.9457551339285052, -0.12047414785403038]}, "obj": undefined});
        atStep(st, 510);
        cqCircle(c, st, 510, 509, {"radius": 0.2, "forConstruction": false});
        atStep(st, 511);
        cqExtrude(c, st, 511, 510, {"until": 3.8139676982460142, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 511, "extrude");
        atStep(st, 512);
        cqUnion(c, st, 512, 508, {"toUnion": {"wp": 511}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 512, "union");
        atStep(st, 513);
        cqWorkplaneNew(c, st, 513, {"plane": {"origin": [-1.7149913973508248, 0.4395193158562614, 1.7785432738841147], "xDir": [-2.7755575615628914e-17, 0.544874466578244, 0.8385176299107098], "zDir": [0.4696993688720256, 0.7402655244593532, -0.48102957931725937]}, "obj": undefined});
        atStep(st, 514);
        cqCircle(c, st, 514, 513, {"radius": 0.2, "forConstruction": false});
        atStep(st, 515);
        cqExtrude(c, st, 515, 514, {"until": 6.542217612062477, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 515, "extrude");
        atStep(st, 516);
        cqUnion(c, st, 516, 512, {"toUnion": {"wp": 515}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 516, "union");
        atStep(st, 517);
        cqWorkplaneNew(c, st, 517, {"plane": {"origin": [-4.499595844410071, -2.158284390992646, 2.385611257925551], "xDir": [0.6122165621615989, 6.9388939039072276e-18, -0.7906901295798711], "zDir": [-0.7858038296229835, -0.11100176895843882, -0.6084331915978539]}, "obj": undefined});
        atStep(st, 518);
        cqCircle(c, st, 518, 517, {"radius": 0.2, "forConstruction": false});
        atStep(st, 519);
        cqExtrude(c, st, 519, 518, {"until": 3.1767895708869216, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 519, "extrude");
        atStep(st, 520);
        cqUnion(c, st, 520, 516, {"toUnion": {"wp": 519}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 520, "union");
        atStep(st, 521);
        cqWorkplaneNew(c, st, 521, {"plane": {"origin": [-3.762505785737277, -0.8085453136077663, 5.24598456196116], "xDir": [1.3877787807814457e-17, 0.8500672961539764, -0.526674085188808], "zDir": [-0.22358217364503777, 0.513341374599779, 0.8285479133714415]}, "obj": undefined});
        atStep(st, 522);
        cqCircle(c, st, 522, 521, {"radius": 0.2, "forConstruction": false});
        atStep(st, 523);
        cqExtrude(c, st, 523, 522, {"until": 4.571711942413148, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 523, "extrude");
        atStep(st, 524);
        cqUnion(c, st, 524, 520, {"toUnion": {"wp": 523}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 524, "union");
        atStep(st, 525);
        cqWorkplaneNew(c, st, 525, {"plane": {"origin": [-1.253993496758183, -0.206330957927927, 3.827312694554746], "xDir": [-0.6643931958615271, 0.7473832225123245, 0], "zDir": [0.7358386009935088, 0.6541304982856913, 0.1750852492371335]}, "obj": undefined});
        atStep(st, 526);
        cqCircle(c, st, 526, 525, {"radius": 0.2, "forConstruction": false});
        atStep(st, 527);
        cqExtrude(c, st, 527, 526, {"until": 5.429004783386895, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 527, "extrude");
        atStep(st, 528);
        cqUnion(c, st, 528, 524, {"toUnion": {"wp": 527}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 528, "union");
        atStep(st, 529);
        cqWorkplaneNew(c, st, 529, {"plane": {"origin": [-1.3500250762493438, 0.47730025924933506, 2.769681982653505], "xDir": [0.7911201397212515, -0.6116607920468888, 1.3877787807814457e-17], "zDir": [0.6012018774526251, 0.7775926125644802, -0.18413590478932973]}, "obj": undefined});
        atStep(st, 530);
        cqCircle(c, st, 530, 529, {"radius": 0.2, "forConstruction": false});
        atStep(st, 531);
        cqExtrude(c, st, 531, 530, {"until": 6.325343064005685, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 531, "extrude");
        atStep(st, 532);
        cqUnion(c, st, 532, 528, {"toUnion": {"wp": 531}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 532, "union");
        atStep(st, 533);
        cqWorkplaneNew(c, st, 533, {"plane": {"origin": [-2.779035826489044, 1.1538828608180691, 4.029747726025594], "xDir": [0, -0.2112381497634828, 0.9774346239439754], "zDir": [0.1456727233713829, 0.967008167166337, 0.20898483748627877]}, "obj": undefined});
        atStep(st, 534);
        cqCircle(c, st, 534, 533, {"radius": 0.2, "forConstruction": false});
        atStep(st, 535);
        cqExtrude(c, st, 535, 534, {"until": 6.48567970219165, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 535, "extrude");
        atStep(st, 536);
        cqUnion(c, st, 536, 532, {"toUnion": {"wp": 535}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 536, "union");
        atStep(st, 537);
        cqWorkplaneNew(c, st, 537, {"plane": {"origin": [-4.03948942118312, 0.6508754502621229, 3.5685937971139543], "xDir": [0.9580056896789784, 0.2867491910061917, 3.469446951953614e-18], "zDir": [-0.2858631389324347, 0.9550454618749084, 0.07855209450955661]}, "obj": undefined});
        atStep(st, 538);
        cqCircle(c, st, 538, 537, {"radius": 0.2, "forConstruction": false});
        atStep(st, 539);
        cqExtrude(c, st, 539, 538, {"until": 5.513549491345647, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 539, "extrude");
        atStep(st, 540);
        cqUnion(c, st, 540, 536, {"toUnion": {"wp": 539}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 540, "union");
        atStep(st, 541);
        cqWorkplaneNew(c, st, 541, {"plane": {"origin": [0.6474105660478959, 2.25293906132547, 3.2449513104579473], "xDir": [0, -0.8398301248730828, -0.5428492989363275], "zDir": [-0.07603480436785368, 0.5412778393654901, -0.8373989545829209]}, "obj": undefined});
        atStep(st, 542);
        cqCircle(c, st, 542, 541, {"radius": 0.2, "forConstruction": false});
        atStep(st, 543);
        cqExtrude(c, st, 543, 542, {"until": 2.5259900459942894, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 543, "extrude");
        atStep(st, 544);
        cqUnion(c, st, 544, 540, {"toUnion": {"wp": 543}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 544, "union");
        atStep(st, 545);
        cqWorkplaneNew(c, st, 545, {"plane": {"origin": [0.28244424494641485, 2.2151581179323965, 2.2538126016885567], "xDir": [0, -0.9537335363056962, -0.30065319177722244], "zDir": [-0.20982435633171995, 0.2939603798829002, -0.9325025654383372]}, "obj": undefined});
        atStep(st, 546);
        cqCircle(c, st, 546, 545, {"radius": 0.2, "forConstruction": false});
        atStep(st, 547);
        cqExtrude(c, st, 547, 546, {"until": 4.394131440716361, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 547, "extrude");
        atStep(st, 548);
        cqUnion(c, st, 548, 544, {"toUnion": {"wp": 547}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 548, "union");
        atStep(st, 549);
        cqWorkplaneNew(c, st, 549, {"plane": {"origin": [1.6899654997788964, 3.3646160694775613, 3.279384037962597], "xDir": [0, 0.4951563983860259, 0.8688038565391956], "zDir": [0.4164427462056381, 0.7898834025518202, -0.4501773533907118]}, "obj": undefined});
        atStep(st, 550);
        cqCircle(c, st, 550, 549, {"radius": 0.2, "forConstruction": false});
        atStep(st, 551);
        cqExtrude(c, st, 551, 550, {"until": 4.5457550305003975, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 551, "extrude");
        atStep(st, 552);
        cqUnion(c, st, 552, 548, {"toUnion": {"wp": 551}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 552, "union");
        atStep(st, 553);
        cqWorkplaneNew(c, st, 553, {"plane": {"origin": [0.2413349547202436, 3.0311601525610428, 5.131172511561773], "xDir": [0, -0.49310604712233513, 0.869969209967448], "zDir": [-0.2863026995859209, 0.8335516199478874, 0.4724642431889071]}, "obj": undefined});
        atStep(st, 554);
        cqCircle(c, st, 554, 553, {"radius": 0.2, "forConstruction": false});
        atStep(st, 555);
        cqExtrude(c, st, 555, 554, {"until": 3.507526764819263, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 555, "extrude");
        atStep(st, 556);
        cqUnion(c, st, 556, 552, {"toUnion": {"wp": 555}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 556, "union");
        atStep(st, 557);
        cqWorkplaneNew(c, st, 557, {"plane": {"origin": [-0.7816001841918043, 2.929521662894204, 4.505017053830036], "xDir": [-0.6656260023347828, -0.7462854849290692, 0], "zDir": [-0.7426504348019142, 0.6623838330399388, 0.0985798631356649]}, "obj": undefined});
        atStep(st, 558);
        cqCircle(c, st, 558, 557, {"radius": 0.2, "forConstruction": false});
        atStep(st, 559);
        cqExtrude(c, st, 559, 558, {"until": 4.107026019954147, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 559, "extrude");
        atStep(st, 560);
        cqUnion(c, st, 560, 556, {"toUnion": {"wp": 559}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 560, "union");
        atStep(st, 561);
        cqWorkplaneNew(c, st, 561, {"plane": {"origin": [-1.7650701434400375, 0.9670934884683686, 5.7212538897656025], "xDir": [-0.4922721218228122, 1.3877787807814457e-17, -0.870441358206322], "zDir": [-0.852037359702225, -0.20454718593268295, 0.4818638670814422]}, "obj": undefined});
        atStep(st, 562);
        cqCircle(c, st, 562, 561, {"radius": 0.2, "forConstruction": false});
        atStep(st, 563);
        cqExtrude(c, st, 563, 562, {"until": 5.8882683028260265, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 563, "extrude");
        atStep(st, 564);
        cqUnion(c, st, 564, 560, {"toUnion": {"wp": 563}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 564, "union");
        atStep(st, 565);
        cqWorkplaneNew(c, st, 565, {"plane": {"origin": [1.4730293141567068, 3.137239042110278, 3.145022732103022], "xDir": [0, 0.5939442119945374, 0.8045062293346074], "zDir": [0.3505909370420736, 0.7534431683503223, -0.556245797224742]}, "obj": undefined});
        atStep(st, 566);
        cqCircle(c, st, 566, 565, {"radius": 0.2, "forConstruction": false});
        atStep(st, 567);
        cqExtrude(c, st, 567, 566, {"until": 4.162042377781682, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 567, "extrude");
        atStep(st, 568);
        cqUnion(c, st, 568, 564, {"toUnion": {"wp": 567}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 568, "union");
        atStep(st, 569);
        cqWorkplaneNew(c, st, 569, {"plane": {"origin": [-2.0420537788858804, 2.426514252338258, 4.0438631249183965], "xDir": [-0.29412682836568566, -0.955766398674667, 0], "zDir": [-0.9520225446784202, 0.29297469756959654, -0.08842454980393746]}, "obj": undefined});
        atStep(st, 570);
        cqCircle(c, st, 570, 569, {"radius": 0.2, "forConstruction": false});
        atStep(st, 571);
        cqExtrude(c, st, 571, 570, {"until": 5.85174361677714, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 571, "extrude");
        atStep(st, 572);
        cqUnion(c, st, 572, 568, {"toUnion": {"wp": 571}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 572, "union");
        atStep(st, 573);
        cqWorkplaneNew(c, st, 573, {"plane": {"origin": [4.671663818064334, 2.215828338984269, 3.910753511821995], "xDir": [0.822590197886778, 0.5686346510199598, 2.7755575615628914e-17], "zDir": [-0.5161491510405295, 0.7466643679416195, -0.4196217052613967]}, "obj": undefined});
        atStep(st, 574);
        cqCircle(c, st, 574, 573, {"radius": 0.2, "forConstruction": false});
        atStep(st, 575);
        cqExtrude(c, st, 575, 574, {"until": 7.885995588456527, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 575, "extrude");
        atStep(st, 576);
        cqUnion(c, st, 576, 572, {"toUnion": {"wp": 575}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 576, "union");
        atStep(st, 577);
        cqWorkplaneNew(c, st, 577, {"plane": {"origin": [1.924475293973238, 6.642118632749067, 0.17062316760692692], "xDir": [0.6762525155149415, -0.7366698957197274, -6.9388939039072276e-18], "zDir": [-0.7347030828918194, -0.6744470092357411, -0.07302473363334251]}, "obj": undefined});
        atStep(st, 578);
        cqCircle(c, st, 578, 577, {"radius": 0.2, "forConstruction": false});
        atStep(st, 579);
        cqExtrude(c, st, 579, 578, {"until": 4.297315802184689, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 579, "extrude");
        atStep(st, 580);
        cqUnion(c, st, 580, 576, {"toUnion": {"wp": 579}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 580, "union");
        atStep(st, 581);
        cqWorkplaneNew(c, st, 581, {"plane": {"origin": [3.0697948660021925, 6.62559941140923, 1.2918571960346883], "xDir": [0, -0.5496439144177868, -0.8353990467695618], "zDir": [-0.2397695813839035, -0.8110303098355305, 0.5336107048880028]}, "obj": undefined});
        atStep(st, 582);
        cqCircle(c, st, 582, 581, {"radius": 0.2, "forConstruction": false});
        atStep(st, 583);
        cqExtrude(c, st, 583, 582, {"until": 3.614353492903465, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 583, "extrude");
        atStep(st, 584);
        cqUnion(c, st, 584, 580, {"toUnion": {"wp": 583}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 584, "union");
        atStep(st, 585);
        cqWorkplaneNew(c, st, 585, {"plane": {"origin": [2.8528586803800033, 6.398222384041945, 1.1574958901751142], "xDir": [-1.3877787807814457e-17, -0.4401743338961958, -0.897912331900526], "zDir": [-0.32601533769106483, -0.848854424516415, 0.41612518016708255]}, "obj": undefined});
        atStep(st, 586);
        cqCircle(c, st, 586, 585, {"radius": 0.2, "forConstruction": false});
        atStep(st, 587);
        cqExtrude(c, st, 587, 586, {"until": 3.9890282599024336, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 587, "extrude");
        atStep(st, 588);
        cqUnion(c, st, 588, 584, {"toUnion": {"wp": 587}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 588, "union");
        atStep(st, 589);
        cqWorkplaneNew(c, st, 589, {"plane": {"origin": [-3.2901124731708986, 2.327307307214365, 5.92368892123645], "xDir": [-2.7755575615628914e-17, 0.5267930412890417, 0.8499935832989811], "zDir": [0.391899412722128, 0.7820009567498382, -0.48465384961886854]}, "obj": undefined});
        atStep(st, 590);
        cqCircle(c, st, 590, 589, {"radius": 0.2, "forConstruction": false});
        atStep(st, 591);
        cqExtrude(c, st, 591, 590, {"until": 5.018991747995049, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 591, "extrude");
        atStep(st, 592);
        cqUnion(c, st, 592, 588, {"toUnion": {"wp": 591}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 592, "union");
        atStep(st, 593);
        cqWorkplaneNew(c, st, 593, {"plane": {"origin": [-4.550566067864975, 1.8242998966584183, 5.462534992324811], "xDir": [0, -0.754422855887104, -0.6563887221114831], "zDir": [-0.1236205384248993, 0.65135393406066, -0.7486361032325649]}, "obj": undefined});
        atStep(st, 594);
        cqCircle(c, st, 594, 593, {"radius": 0.2, "forConstruction": false});
        atStep(st, 595);
        cqExtrude(c, st, 595, 594, {"until": 4.481191215877342, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 595, "extrude");
        atStep(st, 596);
        cqUnion(c, st, 596, 592, {"toUnion": {"wp": 595}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 596, "union");
        atStep(st, 597);
        cqWorkplaneNew(c, st, 597, {"plane": {"origin": [1.4911692819897815, 5.176443516146753, 1.1349520251382432], "xDir": [0.6995532107324828, 0, -0.7145805100573865], "zDir": [-0.7145425598034222, 0.010306020250065969, -0.6995160585548972]}, "obj": undefined});
        atStep(st, 598);
        cqCircle(c, st, 598, 597, {"radius": 0.2, "forConstruction": false});
        atStep(st, 599);
        cqExtrude(c, st, 599, 598, {"until": 3.2057420690071803, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 599, "extrude");
        atStep(st, 600);
        cqUnion(c, st, 600, 596, {"toUnion": {"wp": 599}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 600, "union");
        atStep(st, 601);
        cqWorkplaneNew(c, st, 601, {"plane": {"origin": [1.4785694697210277, 6.768425963496332, 1.1332357136979296], "xDir": [0.8115826240560676, 0.5842376608284232, 0], "zDir": [-0.5083156253745849, 0.70611697387882, -0.492964546595658]}, "obj": undefined});
        atStep(st, 602);
        cqCircle(c, st, 602, 601, {"radius": 0.2, "forConstruction": false});
        atStep(st, 603);
        cqExtrude(c, st, 603, 602, {"until": 4.555907103758306, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 603, "extrude");
        atStep(st, 604);
        cqUnion(c, st, 604, 600, {"toUnion": {"wp": 603}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 604, "union");
        atStep(st, 605);
        cqWorkplaneNew(c, st, 605, {"plane": {"origin": [1.2953096830964508, 6.493625440112893, 2.529600031283933], "xDir": [-0.7051272035405936, -0.7090808323647046, 0], "zDir": [-0.7017865395010123, 0.6978735815358514, 0.14306682763491116]}, "obj": undefined});
        atStep(st, 606);
        cqCircle(c, st, 606, 605, {"radius": 0.2, "forConstruction": false});
        atStep(st, 607);
        cqExtrude(c, st, 607, 606, {"until": 3.822185509217367, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 607, "extrude");
        atStep(st, 608);
        cqUnion(c, st, 608, 604, {"toUnion": {"wp": 607}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 608, "union");
        atStep(st, 609);
        cqWorkplaneNew(c, st, 609, {"plane": {"origin": [1.1878583089600832, 4.8264683778903965, 4.107974527165181], "xDir": [0.7876278582959751, 0, 0.616151245098227], "zDir": [-0.610046028136233, -0.14042466408521836, 0.7798235423938932]}, "obj": undefined});
        atStep(st, 610);
        cqCircle(c, st, 610, 609, {"radius": 0.2, "forConstruction": false});
        atStep(st, 611);
        cqExtrude(c, st, 611, 610, {"until": 4.749249985232756, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 611, "extrude");
        atStep(st, 612);
        cqUnion(c, st, 612, 608, {"toUnion": {"wp": 611}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 612, "union");
        atStep(st, 613);
        cqWorkplaneNew(c, st, 613, {"plane": {"origin": [2.4195526683965465, 4.932547267439631, 2.1218247477064307], "xDir": [-0.723523229527184, 0.6903000335611712, 2.775557561562892e-17], "zDir": [-0.6347218816301932, -0.6652701772003506, -0.39312049591396414]}, "obj": undefined});
        atStep(st, 614);
        cqCircle(c, st, 614, 613, {"radius": 0.2, "forConstruction": false});
        atStep(st, 615);
        cqExtrude(c, st, 615, 614, {"until": 0.6835629648217572, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 615, "extrude");
        atStep(st, 616);
        cqUnion(c, st, 616, 612, {"toUnion": {"wp": 615}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 616, "union");
        atStep(st, 617);
        cqWorkplaneNew(c, st, 617, {"plane": {"origin": [0.8083811016296659, 5.7181370273812835, 3.560792689049247], "xDir": [-0.5808888748220271, -1.3877787807814457e-17, -0.8139828715077483], "zDir": [-0.7899482124594519, 0.24121069980309373, 0.5637368357067138]}, "obj": undefined});
        atStep(st, 618);
        cqCircle(c, st, 618, 617, {"radius": 0.2, "forConstruction": false});
        atStep(st, 619);
        cqExtrude(c, st, 619, 618, {"until": 4.6284243031511565, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 619, "extrude");
        atStep(st, 620);
        cqUnion(c, st, 620, 616, {"toUnion": {"wp": 619}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 620, "union");
        atStep(st, 621);
        cqWorkplaneNew(c, st, 621, {"plane": {"origin": [-6.339959631114668, -2.4983621435127623, 0.7931740485055772], "xDir": [-0.7264509821006727, 0, 0.6872182845391762], "zDir": [-0.6751351191216315, -0.1866983780313272, -0.7136779992188999]}, "obj": undefined});
        atStep(st, 622);
        cqCircle(c, st, 622, 621, {"radius": 0.2, "forConstruction": false});
        atStep(st, 623);
        cqExtrude(c, st, 623, 622, {"until": 1.7543068478511743, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 623, "extrude");
        atStep(st, 624);
        cqUnion(c, st, 624, 620, {"toUnion": {"wp": 623}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 624, "union");
        atStep(st, 625);
        cqWorkplaneNew(c, st, 625, {"plane": {"origin": [-2.9631581027054734, 0.26320468486767745, 0.8121111650593615], "xDir": [-0.6821553782833787, 0.7312072482402376, 0], "zDir": [0.7220901433941985, 0.6736498798490199, -0.1574219304682382]}, "obj": undefined});
        atStep(st, 626);
        cqCircle(c, st, 626, 625, {"radius": 0.2, "forConstruction": false});
        atStep(st, 627);
        cqExtrude(c, st, 627, 626, {"until": 7.712622786873007, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 627, "extrude");
        atStep(st, 628);
        cqUnion(c, st, 628, 624, {"toUnion": {"wp": 627}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 628, "union");
        atStep(st, 629);
        cqWorkplaneNew(c, st, 629, {"plane": {"origin": [-6.468085469927464, -2.2715605724597214, 1.2003498812403657], "xDir": [0.29067596484955405, 3.4694469519536138e-18, -0.9568215525680748], "zDir": [-0.9534846547694684, 0.08344340100359166, -0.28966223769780786]}, "obj": undefined});
        atStep(st, 630);
        cqCircle(c, st, 630, 629, {"radius": 0.2, "forConstruction": false});
        atStep(st, 631);
        cqExtrude(c, st, 631, 630, {"until": 1.5109271377564064, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 631, "extrude");
        atStep(st, 632);
        cqUnion(c, st, 632, 628, {"toUnion": {"wp": 631}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 632, "union");
        atStep(st, 633);
        cqWorkplaneNew(c, st, 633, {"plane": {"origin": [-5.861836396311006, 0.5189688316304604, 2.574418359626798], "xDir": [-1.734723475976807e-18, -0.3752551784579934, 0.9269215452455832], "zDir": [-0.03702907922059085, 0.926285851769229, 0.37499782413265975]}, "obj": undefined});
        atStep(st, 634);
        cqCircle(c, st, 634, 633, {"radius": 0.2, "forConstruction": false});
        atStep(st, 635);
        cqExtrude(c, st, 635, 634, {"until": 6.161311539329474, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 635, "extrude");
        atStep(st, 636);
        cqUnion(c, st, 636, 632, {"toUnion": {"wp": 635}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 636, "union");
        atStep(st, 637);
        cqWorkplaneNew(c, st, 637, {"plane": {"origin": [-5.287656126537769, 0.4745608192735389, 2.602161688289202], "xDir": [0, -0.3881067130682924, 0.9216144417658212], "zDir": [0.1492584043216473, 0.9112907236752932, 0.38375922879159025]}, "obj": undefined});
        atStep(st, 638);
        cqCircle(c, st, 638, 637, {"radius": 0.2, "forConstruction": false});
        atStep(st, 639);
        cqExtrude(c, st, 639, 638, {"until": 6.1652330442369685, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 639, "extrude");
        atStep(st, 640);
        cqUnion(c, st, 640, 636, {"toUnion": {"wp": 639}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 640, "union");
        atStep(st, 641);
        cqWorkplaneNew(c, st, 641, {"plane": {"origin": [-4.934328849214886, 1.7819932492861563, 1.3099820684618697], "xDir": [0.9810310692759432, -0.19385056387666183, 0], "zDir": [0.19378496016123764, 0.9806990646544242, -0.026014107734944704]}, "obj": undefined});
        atStep(st, 642);
        cqCircle(c, st, 642, 641, {"radius": 0.2, "forConstruction": false});
        atStep(st, 643);
        cqExtrude(c, st, 643, 642, {"until": 8.395220143740993, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 643, "extrude");
        atStep(st, 644);
        cqUnion(c, st, 644, 640, {"toUnion": {"wp": 643}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 644, "union");
        atStep(st, 645);
        cqWorkplaneNew(c, st, 645, {"plane": {"origin": [-5.729823934681389, 2.1240222572983503, 0.3448137211744223], "xDir": [0.9698655218482894, -0.2436408617821435, 0], "zDir": [0.24348315366616463, 0.9692377303395716, 0.0359746572878044]}, "obj": undefined});
        atStep(st, 646);
        cqCircle(c, st, 646, 645, {"radius": 0.2, "forConstruction": false});
        atStep(st, 647);
        cqExtrude(c, st, 647, 646, {"until": 9.876106495907509, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 647, "extrude");
        atStep(st, 648);
        cqUnion(c, st, 648, 644, {"toUnion": {"wp": 647}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 648, "union");
        atStep(st, 649);
        cqWorkplaneNew(c, st, 649, {"plane": {"origin": [-3.5553551840554216, 0.09944156333614518, 0.18610606446414105], "xDir": [-0.6330632731675189, 0.7741000530722287, 0], "zDir": [0.7740927589709272, 0.6330573080114302, 0.004341115305932368]}, "obj": undefined});
        atStep(st, 650);
        cqCircle(c, st, 650, 649, {"radius": 0.2, "forConstruction": false});
        atStep(st, 651);
        cqExtrude(c, st, 651, 650, {"until": 8.724539764196447, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 651, "extrude");
        atStep(st, 652);
        cqUnion(c, st, 652, 648, {"toUnion": {"wp": 651}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 652, "union");
        atStep(st, 653);
        cqWorkplaneNew(c, st, 653, {"plane": {"origin": [-7.060282551277412, -2.435323693991254, 0.5743447806451449], "xDir": [-1.387778780781446e-17, 0.8736168082035181, -0.4866145008364401], "zDir": [-0.26506738845598193, 0.4692082463130719, 0.8423674383361083]}, "obj": undefined});
        atStep(st, 654);
        cqCircle(c, st, 654, 653, {"radius": 0.2, "forConstruction": false});
        atStep(st, 655);
        cqExtrude(c, st, 655, 654, {"until": 0.966741624151724, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 655, "extrude");
        atStep(st, 656);
        cqUnion(c, st, 656, 652, {"toUnion": {"wp": 655}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 656, "union");
        atStep(st, 657);
        cqWorkplaneNew(c, st, 657, {"plane": {"origin": [-5.5265259305648335, 1.6182301277546245, 0.6839769678666491], "xDir": [0.950082533507956, -0.3119986851304082, -6.938893903907228e-18], "zDir": [0.3099659361341262, 0.943892509612291, 0.1139651206918416]}, "obj": undefined});
        atStep(st, 658);
        cqCircle(c, st, 658, 657, {"radius": 0.2, "forConstruction": false});
        atStep(st, 659);
        cqExtrude(c, st, 659, 658, {"until": 9.06958228656163, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 659, "extrude");
        atStep(st, 660);
        cqUnion(c, st, 660, 656, {"toUnion": {"wp": 659}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 660, "union");
        atStep(st, 661);
        cqWorkplaneNew(c, st, 661, {"plane": {"origin": [-6.582159316473749, 0.5820072811519688, 2.3555890917663658], "xDir": [0, 0.44175373440000887, -0.8971363542648612], "zDir": [-0.1913052040835801, -0.8805667841538632, -0.43359480801257694]}, "obj": undefined});
        atStep(st, 662);
        cqCircle(c, st, 662, 661, {"radius": 0.2, "forConstruction": false});
        atStep(st, 663);
        cqExtrude(c, st, 663, 662, {"until": 6.3380301285645295, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 663, "extrude");
        atStep(st, 664);
        cqUnion(c, st, 664, 660, {"toUnion": {"wp": 663}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 664, "union");
        atStep(st, 665);
        cqWorkplaneNew(c, st, 665, {"plane": {"origin": [-5.401729973084055, 3.3281286728852293, 3.757400898815202], "xDir": [0.07711130010492459, 0.9970224909178972, -4.33680868994202e-19], "zDir": [0.9958675750292048, -0.0770219770791151, 0.048118479322142416]}, "obj": undefined});
        atStep(st, 666);
        cqCircle(c, st, 666, 665, {"radius": 0.2, "forConstruction": false});
        atStep(st, 667);
        cqExtrude(c, st, 667, 666, {"until": 1.153125745170282, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 667, "extrude");
        atStep(st, 668);
        cqUnion(c, st, 668, 664, {"toUnion": {"wp": 667}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 668, "union");
        atStep(st, 669);
        cqWorkplaneNew(c, st, 669, {"plane": {"origin": [-5.048402695761172, 4.635561102897847, 2.4652212789878702], "xDir": [0, -0.7075016701471608, -0.7067116715740432], "zDir": [0.4606377307050166, 0.6272689676708928, -0.6279701611127915]}, "obj": undefined});
        atStep(st, 670);
        cqCircle(c, st, 670, 669, {"radius": 0.2, "forConstruction": false});
        atStep(st, 671);
        cqExtrude(c, st, 671, 670, {"until": 4.0270585115837045, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 671, "extrude");
        atStep(st, 672);
        cqUnion(c, st, 672, 668, {"toUnion": {"wp": 671}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 672, "union");
        atStep(st, 673);
        cqWorkplaneNew(c, st, 673, {"plane": {"origin": [-7.7839575412589985, -2.018638247967024, 0.6943136120852221], "xDir": [0.4343816885880479, 0, -0.90072889851464], "zDir": [-0.865734847968679, 0.2760294214293912, -0.4175056065707427]}, "obj": undefined});
        atStep(st, 674);
        cqCircle(c, st, 674, 673, {"radius": 0.2, "forConstruction": false});
        atStep(st, 675);
        cqExtrude(c, st, 675, 674, {"until": 1.3758234465579386, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 675, "extrude");
        atStep(st, 676);
        cqUnion(c, st, 676, 672, {"toUnion": {"wp": 675}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 676, "union");
        atStep(st, 677);
        cqWorkplaneNew(c, st, 677, {"plane": {"origin": [-5.6546517693776295, 1.845031698807665, 1.0911528006014373], "xDir": [0.935287823117802, -0.35388795956850977, 1.734723475976807e-18], "zDir": [0.3537747919278929, 0.9349887332127118, 0.025287652355735567]}, "obj": undefined});
        atStep(st, 678);
        cqCircle(c, st, 678, 677, {"radius": 0.2, "forConstruction": false});
        atStep(st, 679);
        cqExtrude(c, st, 679, 678, {"until": 8.670807845603605, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 679, "extrude");
        atStep(st, 680);
        cqUnion(c, st, 680, 676, {"toUnion": {"wp": 679}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 680, "union");
        atStep(st, 681);
        cqWorkplaneNew(c, st, 681, {"plane": {"origin": [-3.721429302604476, 4.039868347261071, 4.822872909910593], "xDir": [-0.17391957994298415, 0.9847598589059446, -1.7347234759768069e-18], "zDir": [0.9815972078736196, 0.1733610204788938, -0.08008044750893091]}, "obj": undefined});
        atStep(st, 682);
        cqCircle(c, st, 682, 681, {"radius": 0.2, "forConstruction": false});
        atStep(st, 683);
        cqExtrude(c, st, 683, 682, {"until": 2.8826218683864964, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 683, "extrude");
        atStep(st, 684);
        cqUnion(c, st, 684, 680, {"toUnion": {"wp": 683}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 684, "union");
        atStep(st, 685);
        cqWorkplaneNew(c, st, 685, {"plane": {"origin": [-4.9818828972985525, 3.536860936705125, 4.361718980998955], "xDir": [0, -0.9156382452009744, 0.4020032386999894], "zDir": [0.23804575473059472, -0.39044723003666054, -0.8893172545337467]}, "obj": undefined});
        atStep(st, 686);
        cqCircle(c, st, 686, 685, {"radius": 0.2, "forConstruction": false});
        atStep(st, 687);
        cqExtrude(c, st, 687, 686, {"until": 1.2966683162436503, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 687, "extrude");
        atStep(st, 688);
        cqUnion(c, st, 688, 684, {"toUnion": {"wp": 687}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 688, "union");
        atStep(st, 689);
        cqWorkplaneNew(c, st, 689, {"plane": {"origin": [-4.628555619975669, 4.844293366717743, 3.069539361171622], "xDir": [-1.3877787807814457e-17, -0.8709539313140399, -0.4913646807907737], "zDir": [0.23024407933481095, 0.4781631425490935, -0.8475539351798449]}, "obj": undefined});
        atStep(st, 690);
        cqCircle(c, st, 690, 689, {"radius": 0.2, "forConstruction": false});
        atStep(st, 691);
        cqExtrude(c, st, 691, 690, {"until": 4.409759180581576, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 691, "extrude");
        atStep(st, 692);
        cqUnion(c, st, 692, 688, {"toUnion": {"wp": 691}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 692, "union");
        atStep(st, 693);
        cqWorkplaneNew(c, st, 693, {"plane": {"origin": [-6.250200920546421, 2.0349155737788545, 0.8039457993067263], "xDir": [0.875805220192294, -0.4826647037892117, 0], "zDir": [0.4807236671493639, 0.8722831685311357, 0.08959257636517654]}, "obj": undefined});
        atStep(st, 694);
        cqCircle(c, st, 694, 693, {"radius": 0.2, "forConstruction": false});
        atStep(st, 695);
        cqExtrude(c, st, 695, 694, {"until": 8.858751575548204, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 695, "extrude");
        atStep(st, 696);
        cqUnion(c, st, 696, 692, {"toUnion": {"wp": 695}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 696, "union");
        atStep(st, 697);
        cqWorkplaneNew(c, st, 697, {"plane": {"origin": [-0.15332086196220218, 6.160169523196374, 4.381388504883109], "xDir": [0, 0.6875064591409191, 0.7261782623016996], "zDir": [0.046752360764108915, 0.7253841940126273, -0.6867546780617034]}, "obj": undefined});
        atStep(st, 698);
        cqCircle(c, st, 698, 697, {"radius": 0.2, "forConstruction": false});
        atStep(st, 699);
        cqExtrude(c, st, 699, 698, {"until": 4.596618112121355, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 699, "extrude");
        atStep(st, 700);
        cqUnion(c, st, 700, 696, {"toUnion": {"wp": 699}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 700, "union");
        atStep(st, 701);
        cqWorkplaneNew(c, st, 701, {"plane": {"origin": [-1.2837073750106174, 4.391373971307038, 5.33360754303262], "xDir": [0.52207419255927, 6.938893903907228e-18, -0.8529000747236376], "zDir": [-0.8498539115350363, -0.08444119741330233, -0.5202095858670873]}, "obj": undefined});
        atStep(st, 702);
        cqCircle(c, st, 702, 701, {"radius": 0.2, "forConstruction": false});
        atStep(st, 703);
        cqExtrude(c, st, 703, 702, {"until": 2.407319952353661, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 703, "extrude");
        atStep(st, 704);
        cqUnion(c, st, 704, 700, {"toUnion": {"wp": 703}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 704, "union");
        atStep(st, 705);
        cqWorkplaneNew(c, st, 705, {"plane": {"origin": [0.14530337522908282, 3.714791369738305, 4.073541799660532], "xDir": [0, -0.9244118470635836, 0.38139577476486786], "zDir": [0.19518454454779005, -0.3740602119730299, -0.9066322029292604]}, "obj": undefined});
        atStep(st, 706);
        cqCircle(c, st, 706, 705, {"radius": 0.2, "forConstruction": false});
        atStep(st, 707);
        cqExtrude(c, st, 707, 706, {"until": 4.1609402247341025, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 707, "extrude");
        atStep(st, 708);
        cqUnion(c, st, 708, 704, {"toUnion": {"wp": 707}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 708, "union");
        atStep(st, 709);
        cqWorkplaneNew(c, st, 709, {"plane": {"origin": [0.9709221233378936, 4.599091350523112, 3.9736132213056066], "xDir": [-0.8498487446146429, -3.469446951953614e-18, -0.5270266703658512], "zDir": [0.5264846078460109, 0.0453431503823903, -0.8489746500424097]}, "obj": undefined});
        atStep(st, 710);
        cqCircle(c, st, 710, 709, {"radius": 0.2, "forConstruction": false});
        atStep(st, 711);
        cqExtrude(c, st, 711, 710, {"until": 4.67893777360616, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 711, "extrude");
        atStep(st, 712);
        cqUnion(c, st, 712, 708, {"toUnion": {"wp": 711}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 712, "union");
        atStep(st, 713);
        cqWorkplaneNew(c, st, 713, {"plane": {"origin": [-0.8175005981632097, 4.899823151454159, 5.663172377506157], "xDir": [-0.5899889202842384, -0.8074113412269105, 2.7755575615628914e-17], "zDir": [-0.7417064766860259, 0.5419772809765806, -0.39513558349917616]}, "obj": undefined});
        atStep(st, 714);
        cqCircle(c, st, 714, 713, {"radius": 0.2, "forConstruction": false});
        atStep(st, 715);
        cqExtrude(c, st, 715, 714, {"until": 1.501209385556736, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 715, "extrude");
        atStep(st, 716);
        cqUnion(c, st, 716, 712, {"toUnion": {"wp": 715}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 716, "union");
        atStep(st, 717);
        cqWorkplaneNew(c, st, 717, {"plane": {"origin": [-1.3691246078995425, 6.210212372702759, 3.735588503627646], "xDir": [0.9553588039303192, 0, -0.2954480593147133], "zDir": [0.2949871139020814, 0.0558379824153644, 0.9538682940277982]}, "obj": undefined});
        atStep(st, 718);
        cqCircle(c, st, 718, 717, {"radius": 0.2, "forConstruction": false});
        atStep(st, 719);
        cqExtrude(c, st, 719, 718, {"until": 2.368903187114254, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 719, "extrude");
        atStep(st, 720);
        cqUnion(c, st, 720, 716, {"toUnion": {"wp": 719}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 720, "union");
        atStep(st, 721);
        cqWorkplaneNew(c, st, 721, {"plane": {"origin": [-2.0125825394811727, 5.216905233545033, 3.656614884011842], "xDir": [-1.3877787807814457e-17, 0.749851853989001, 0.6616057716414346], "zDir": [-0.2053616707479243, -0.6475044068127631, 0.7338696255777813]}, "obj": undefined});
        atStep(st, 722);
        cqCircle(c, st, 722, 721, {"radius": 0.2, "forConstruction": false});
        atStep(st, 723);
        cqExtrude(c, st, 723, 722, {"until": 2.8638253026529275, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 723, "extrude");
        atStep(st, 724);
        cqUnion(c, st, 724, 720, {"toUnion": {"wp": 723}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 724, "union");
        atStep(st, 725);
        cqWorkplaneNew(c, st, 725, {"plane": {"origin": [-2.508639184840756, 5.824679735473914, 3.825589826202259], "xDir": [0.8393107135598334, -1.3877787807814457e-17, 0.5436520266711816], "zDir": [-0.5309809293612224, -0.2146427279412096, 0.8197485907256251]}, "obj": undefined});
        atStep(st, 726);
        cqCircle(c, st, 726, 725, {"radius": 0.2, "forConstruction": false});
        atStep(st, 727);
        cqExtrude(c, st, 727, 726, {"until": 2.976064020798628, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 727, "extrude");
        atStep(st, 728);
        cqUnion(c, st, 728, 724, {"toUnion": {"wp": 727}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 728, "union");
        atStep(st, 729);
        cqWorkplaneNew(c, st, 729, {"plane": {"origin": [-2.919708856852366, 6.021330253001704, 1.903281335272872], "xDir": [0.5048381685676006, 0, -0.863214007970973], "zDir": [-0.8598752489617743, -0.08786743412908328, -0.5028855438790203]}, "obj": undefined});
        atStep(st, 730);
        cqCircle(c, st, 730, 729, {"radius": 0.2, "forConstruction": false});
        atStep(st, 731);
        cqExtrude(c, st, 731, 730, {"until": 2.7938617683507356, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 731, "extrude");
        atStep(st, 732);
        cqUnion(c, st, 732, 728, {"toUnion": {"wp": 731}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 732, "union");
        atStep(st, 733);
        cqWorkplaneNew(c, st, 733, {"plane": {"origin": [-1.546375762633765, 5.725354413692153, 3.986179718485379], "xDir": [0, 0.9569441500563102, 0.29027210281562776], "zDir": [0.1184975150803812, -0.28822694684348366, 0.9502018554144476]}, "obj": undefined});
        atStep(st, 734);
        cqCircle(c, st, 734, 733, {"radius": 0.2, "forConstruction": false});
        atStep(st, 735);
        cqExtrude(c, st, 735, 734, {"until": 2.9054921917837975, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 735, "extrude");
        atStep(st, 736);
        cqUnion(c, st, 736, 732, {"toUnion": {"wp": 735}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 736, "union");
        atStep(st, 737);
        cqWorkplaneNew(c, st, 737, {"plane": {"origin": [-0.5835717892414725, 4.540322631976299, 2.3965491406397543], "xDir": [-0.8162741944682061, -0.5776646427169325, 0], "zDir": [0.5744166852498779, -0.8116846390946636, -0.1058939014500288]}, "obj": undefined});
        atStep(st, 738);
        cqCircle(c, st, 738, 737, {"radius": 0.2, "forConstruction": false});
        atStep(st, 739);
        cqExtrude(c, st, 739, 738, {"until": 3.9516636787961374, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 739, "extrude");
        atStep(st, 740);
        cqUnion(c, st, 740, 736, {"toUnion": {"wp": 739}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 740, "union");
        atStep(st, 741);
        cqWorkplaneNew(c, st, 741, {"plane": {"origin": [-0.6863364275394268, 5.668518861468229, 1.3097478397166422], "xDir": [-0.7822323854085261, -2.7755575615628914e-17, -0.6229867536457634], "zDir": [0.5988086213123508, -0.27588735850273854, -0.7518739259080495]}, "obj": undefined});
        atStep(st, 742);
        cqCircle(c, st, 742, 741, {"radius": 0.2, "forConstruction": false});
        atStep(st, 743);
        cqExtrude(c, st, 743, 742, {"until": 3.4474658539087537, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 743, "extrude");
        atStep(st, 744);
        cqUnion(c, st, 744, 740, {"toUnion": {"wp": 743}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 744, "union");
        atStep(st, 745);
        cqWorkplaneNew(c, st, 745, {"plane": {"origin": [-0.9485381103429537, 4.502541688583225, 1.405410431870364], "xDir": [0, 0.5902682692569259, -0.8072071421317042], "zDir": [0.35409953025699364, -0.7549063695716227, -0.5520234558893028]}, "obj": undefined});
        atStep(st, 746);
        cqCircle(c, st, 746, 745, {"radius": 0.2, "forConstruction": false});
        atStep(st, 747);
        cqExtrude(c, st, 747, 746, {"until": 4.348971907067471, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 747, "extrude");
        atStep(st, 748);
        cqUnion(c, st, 748, 744, {"toUnion": {"wp": 747}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 748, "union");
        atStep(st, 749);
        cqWorkplaneNew(c, st, 749, {"plane": {"origin": [-0.8821960264327575, 6.985700785434368, 2.704395845862332], "xDir": [0.7093364082536235, -0.704870101455473, 0], "zDir": [0.702447866852012, 0.7068988255131742, 0.08283142424921329]}, "obj": undefined});
        atStep(st, 750);
        cqCircle(c, st, 750, 749, {"radius": 0.2, "forConstruction": false});
        atStep(st, 751);
        cqExtrude(c, st, 751, 750, {"until": 2.3811775309529573, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 751, "extrude");
        atStep(st, 752);
        cqUnion(c, st, 752, 748, {"toUnion": {"wp": 751}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 752, "union");
        atStep(st, 753);
        cqWorkplaneNew(c, st, 753, {"plane": {"origin": [-0.6989362398081806, 7.260501308817807, 1.3080315282763286], "xDir": [-2.77555756156289e-17, -0.7580798866795649, -0.6521617018898748], "zDir": [0.5117086775142391, 0.5603106116031735, -0.6513111759224667]}, "obj": undefined});
        atStep(st, 754);
        cqCircle(c, st, 754, 753, {"radius": 0.2, "forConstruction": false});
        atStep(st, 755);
        cqExtrude(c, st, 755, 754, {"until": 3.9850265201067585, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 755, "extrude");
        atStep(st, 756);
        cqUnion(c, st, 756, 752, {"toUnion": {"wp": 755}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 756, "union");
        atStep(st, 757);
        cqWorkplaneNew(c, st, 757, {"plane": {"origin": [0.2420469588673383, 5.424622612761107, 2.2966205622848292], "xDir": [0.344498131160988, 0.9387870033328044, 0], "zDir": [0.928666596331347, -0.3407843374184103, -0.14643902563403763]}, "obj": undefined});
        atStep(st, 758);
        cqCircle(c, st, 758, 757, {"radius": 0.2, "forConstruction": false});
        atStep(st, 759);
        cqExtrude(c, st, 759, 758, {"until": 4.222332388506607, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 759, "extrude");
        atStep(st, 760);
        cqUnion(c, st, 760, 756, {"toUnion": {"wp": 759}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 760, "union");
        atStep(st, 761);
        cqWorkplaneNew(c, st, 761, {"plane": {"origin": [-3.273036134175249, 4.713897822989087, 3.195460955100204], "xDir": [0.6770628583866429, -0.7359251903511042, 0], "zDir": [-0.7088232966723848, -0.6521286859430415, 0.2688823368402886]}, "obj": undefined});
        atStep(st, 762);
        cqCircle(c, st, 762, 761, {"radius": 0.2, "forConstruction": false});
        atStep(st, 763);
        cqExtrude(c, st, 763, 762, {"until": 4.386180805380778, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 763, "extrude");
        atStep(st, 764);
        cqUnion(c, st, 764, 760, {"toUnion": {"wp": 763}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 764, "union");
        atStep(st, 765);
        cqWorkplaneNew(c, st, 765, {"plane": {"origin": [-3.1230068609689208, 6.52712238254543, 1.5641180885806452], "xDir": [0.5957078082389567, -1.3877787807814457e-17, -0.8032012246026137], "zDir": [-0.7845967361006754, 0.21398440580939104, -0.5819094738621886]}, "obj": undefined});
        atStep(st, 766);
        cqCircle(c, st, 766, 765, {"radius": 0.2, "forConstruction": false});
        atStep(st, 767);
        cqExtrude(c, st, 767, 766, {"until": 3.5801431010516587, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 767, "extrude");
        atStep(st, 768);
        cqUnion(c, st, 768, 764, {"toUnion": {"wp": 767}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 768, "union");
        atStep(st, 769);
        cqWorkplaneNew(c, st, 769, {"plane": {"origin": [0.5914449160074763, 5.490760000013999, 3.426431383189673], "xDir": [0.6661244344301104, 0, 0.7458406249696821], "zDir": [0.7009299401028504, -0.3417658272646187, -0.626013848394383]}, "obj": undefined});
        atStep(st, 770);
        cqCircle(c, st, 770, 769, {"radius": 0.2, "forConstruction": false});
        atStep(st, 771);
        cqExtrude(c, st, 771, 770, {"until": 4.597239965325113, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 771, "extrude");
        atStep(st, 772);
        cqUnion(c, st, 772, 768, {"toUnion": {"wp": 771}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 772, "union");
        atStep(st, 773);
        cqWorkplaneNew(c, st, 773, {"plane": {"origin": [-0.23417383210133452, 4.606460019229192, 3.526359961544598], "xDir": [0, 0.6255855977245026, -0.7801555357232791], "zDir": [0.3445323793087568, -0.7323900721769245, -0.5872837659807449]}, "obj": undefined});
        atStep(st, 774);
        cqCircle(c, st, 774, 773, {"radius": 0.2, "forConstruction": false});
        atStep(st, 775);
        cqExtrude(c, st, 775, 774, {"until": 4.5601102586302185, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 775, "extrude");
        atStep(st, 776);
        cqUnion(c, st, 776, 772, {"toUnion": {"wp": 775}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 776, "union");
        atStep(st, 777);
        cqWorkplaneNew(c, st, 777, {"plane": {"origin": [-1.196977805493627, 5.791491800945046, 5.115990539390223], "xDir": [1.3877787807814457e-17, -0.45913757720390314, -0.8883651755890312], "zDir": [-0.30888188344031114, -0.8449244790804482, 0.4366859360375594]}, "obj": undefined});
        atStep(st, 778);
        cqCircle(c, st, 778, 777, {"radius": 0.2, "forConstruction": false});
        atStep(st, 779);
        cqExtrude(c, st, 779, 778, {"until": 1.1476953763685214, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 779, "extrude");
        atStep(st, 780);
        cqUnion(c, st, 780, 776, {"toUnion": {"wp": 779}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 780, "union");
        atStep(st, 781);
        cqWorkplaneNew(c, st, 781, {"plane": {"origin": [-1.6631845823410347, 5.283042620797926, 4.786425704916686], "xDir": [-0.8392888032803592, 0.5436858511017392, 0], "zDir": [-0.5424794534206224, -0.8374264850610917, -0.06658021278644267]}, "obj": undefined});
        atStep(st, 782);
        cqCircle(c, st, 782, 781, {"radius": 0.2, "forConstruction": false});
        atStep(st, 783);
        cqExtrude(c, st, 783, 782, {"until": 2.372284987106092, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 783, "extrude");
        atStep(st, 784);
        cqUnion(c, st, 784, 780, {"toUnion": {"wp": 783}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 784, "union");
        atStep(st, 785);
        cqWorkplaneNew(c, st, 785, {"plane": {"origin": [-0.8776317636829651, 3.613152880071466, 3.4473863419287945], "xDir": [0.6613778091515674, 0, 0.750052927173725], "zDir": [0.7068076239083155, -0.33464670641021316, -0.6232451882476735]}, "obj": undefined});
        atStep(st, 786);
        cqCircle(c, st, 786, 785, {"radius": 0.2, "forConstruction": false});
        atStep(st, 787);
        cqExtrude(c, st, 787, 786, {"until": 4.043563487156348, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 787, "extrude");
        atStep(st, 788);
        cqUnion(c, st, 788, 784, {"toUnion": {"wp": 787}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 788, "union");
        atStep(st, 789);
        cqWorkplaneNew(c, st, 789, {"plane": {"origin": [-2.8026991592822488, 4.897509983569082, 4.8764270274912995], "xDir": [0.7747147770850091, 0.6323108524816924, 1.3877787807814457e-17], "zDir": [-0.618135128262128, 0.7573465111669474, 0.21055931523353552]}, "obj": undefined});
        atStep(st, 790);
        cqCircle(c, st, 790, 789, {"radius": 0.2, "forConstruction": false});
        atStep(st, 791);
        cqExtrude(c, st, 791, 790, {"until": 1.6050103696718698, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 791, "extrude");
        atStep(st, 792);
        cqUnion(c, st, 792, 788, {"toUnion": {"wp": 791}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 792, "union");
        atStep(st, 793);
        cqWorkplaneNew(c, st, 793, {"plane": {"origin": [-3.5670961086167416, 3.786728071084254, 4.246298156389244], "xDir": [0.37064478857716066, -0.928774698568276, -1.3877787807814457e-17], "zDir": [-0.8793908900109328, -0.3509372628337406, -0.3217370045868857]}, "obj": undefined});
        atStep(st, 794);
        cqCircle(c, st, 794, 793, {"radius": 0.2, "forConstruction": false});
        atStep(st, 795);
        cqExtrude(c, st, 795, 794, {"until": 2.8666514720851977, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 795, "extrude");
        atStep(st, 796);
        cqUnion(c, st, 796, 792, {"toUnion": {"wp": 795}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 796, "union");
        atStep(st, 797);
        cqWorkplaneNew(c, st, 797, {"plane": {"origin": [-1.2425980847844462, 3.5753719366783923, 2.4562476331594043], "xDir": [-0.9040972962613112, 0, -0.42732666531938646], "zDir": [0.41075671717011, -0.2757682071905269, -0.8690401689234837]}, "obj": undefined});
        atStep(st, 798);
        cqCircle(c, st, 798, 797, {"radius": 0.2, "forConstruction": false});
        atStep(st, 799);
        cqExtrude(c, st, 799, 798, {"until": 5.180898496165349, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 799, "extrude");
        atStep(st, 800);
        cqUnion(c, st, 800, 796, {"toUnion": {"wp": 799}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 800, "union");
        atStep(st, 801);
        cqWorkplaneNew(c, st, 801, {"plane": {"origin": [-3.2137688312938586, 5.094160501096871, 2.954118536561912], "xDir": [-0.8881701663473517, 0, 0.4595146957503292], "zDir": [-0.42554073201586146, 0.3773626727350157, -0.8225037985454476]}, "obj": undefined});
        atStep(st, 802);
        cqCircle(c, st, 802, 801, {"radius": 0.2, "forConstruction": false});
        atStep(st, 803);
        cqExtrude(c, st, 803, 802, {"until": 4.263405352874099, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 803, "extrude");
        atStep(st, 804);
        cqUnion(c, st, 804, 800, {"toUnion": {"wp": 803}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 804, "union");
        atStep(st, 805);
        cqWorkplaneNew(c, st, 805, {"plane": {"origin": [-1.8404357370752575, 4.79818466178732, 5.03701691977442], "xDir": [0.7370614460786871, -0.675825735455816, -2.7755575615628907e-17], "zDir": [0.6098076909218275, 0.6650615904669589, 0.43107732598473336]}, "obj": undefined});
        atStep(st, 806);
        cqCircle(c, st, 806, 805, {"radius": 0.2, "forConstruction": false});
        atStep(st, 807);
        cqExtrude(c, st, 807, 806, {"until": 1.5290288521702875, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 807, "extrude");
        atStep(st, 808);
        cqUnion(c, st, 808, 804, {"toUnion": {"wp": 807}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 808, "union");
        atStep(st, 809);
        cqWorkplaneNew(c, st, 809, {"plane": {"origin": [-4.0631527539763255, 4.394502573013136, 4.41527309857966], "xDir": [-0.8237875793230636, 0.5668986012974869, 0], "zDir": [-0.513584989405072, -0.7463150098277392, -0.4233726074790546]}, "obj": undefined});
        atStep(st, 810);
        cqCircle(c, st, 810, 809, {"radius": 0.2, "forConstruction": false});
        atStep(st, 811);
        cqExtrude(c, st, 811, 810, {"until": 2.976710632528248, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 811, "extrude");
        atStep(st, 812);
        cqUnion(c, st, 812, 808, {"toUnion": {"wp": 811}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 812, "union");
        atStep(st, 813);
        cqWorkplaneNew(c, st, 813, {"plane": {"origin": [-3.709825476653442, 5.701935003025753, 3.1230934787523283], "xDir": [-0.9778912880669688, 0, 0.2091139132691193], "zDir": [-0.20807534914819387, 0.09954060803009632, -0.973034591589557]}, "obj": undefined});
        atStep(st, 814);
        cqCircle(c, st, 814, 813, {"radius": 0.2, "forConstruction": false});
        atStep(st, 815);
        cqExtrude(c, st, 815, 814, {"until": 3.9511616699856247, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 815, "extrude");
        atStep(st, 816);
        cqUnion(c, st, 816, 812, {"toUnion": {"wp": 815}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 816, "union");
        atStep(st, 817);
        cqWorkplaneNew(c, st, 817, {"plane": {"origin": [-2.336492382434841, 5.405959163716202, 5.205991861964836], "xDir": [-0.16461107229793984, 0, 0.986358552898956], "zDir": [0.9812857727257582, -0.10128881851531564, 0.1637644878775102]}, "obj": undefined});
        atStep(st, 818);
        cqCircle(c, st, 818, 817, {"radius": 0.2, "forConstruction": false});
        atStep(st, 819);
        cqExtrude(c, st, 819, 818, {"until": 1.9612297435721957, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 819, "extrude");
        atStep(st, 820);
        cqUnion(c, st, 820, 816, {"toUnion": {"wp": 819}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 820, "union");
        atStep(st, 821);
        cqWorkplaneNew(c, st, 821, {"plane": {"origin": [-1.8875227193521125, 5.545774129020067, 0.6072514922667116], "xDir": [0.2568414258701372, 6.938893903907228e-18, 0.9664535591310092], "zDir": [0.9553831531197372, -0.15092425037391033, -0.2538993922483116]}, "obj": undefined});
        atStep(st, 822);
        cqCircle(c, st, 822, 821, {"radius": 0.2, "forConstruction": false});
        atStep(st, 823);
        cqExtrude(c, st, 823, 822, {"until": 4.6753439643979835, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 823, "extrude");
        atStep(st, 824);
        cqUnion(c, st, 824, 820, {"toUnion": {"wp": 823}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 824, "union");
        atStep(st, 825);
        cqWorkplaneNew(c, st, 825, {"plane": {"origin": [-4.324193152781607, 6.404377650097269, 0.8616217411307148], "xDir": [-2.7755575615628914e-17, 0.5569360334556381, 0.830555389265942], "zDir": [-0.3166545442951677, 0.7878157829671916, -0.5282766242083882]}, "obj": undefined});
        atStep(st, 826);
        cqCircle(c, st, 826, 825, {"radius": 0.2, "forConstruction": false});
        atStep(st, 827);
        cqExtrude(c, st, 827, 826, {"until": 1.2840365488458096, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 827, "extrude");
        atStep(st, 828);
        cqUnion(c, st, 828, 824, {"toUnion": {"wp": 827}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 828, "union");
        atStep(st, 829);
        cqWorkplaneNew(c, st, 829, {"plane": {"origin": [-2.149724402155639, 4.379796956135064, 0.7029140844204335], "xDir": [0.6103425955135603, 0.7921375613501551, 0], "zDir": [0.7767435606768508, -0.598481506247363, -0.19618697109373823]}, "obj": undefined});
        atStep(st, 830);
        cqCircle(c, st, 830, 829, {"radius": 0.2, "forConstruction": false});
        atStep(st, 831);
        cqExtrude(c, st, 831, 830, {"until": 5.075473673168899, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 831, "extrude");
        atStep(st, 832);
        cqUnion(c, st, 832, 828, {"toUnion": {"wp": 831}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 832, "union");
        atStep(st, 833);
        cqWorkplaneNew(c, st, 833, {"plane": {"origin": [-4.474222425987935, 4.591153090540925, 2.4929646076502734], "xDir": [0, -0.7029458450329563, -0.7112433753300645], "zDir": [-0.1887549489514903, -0.6984582210986057, 0.6903098453782125]}, "obj": undefined});
        atStep(st, 834);
        cqCircle(c, st, 834, 833, {"radius": 0.2, "forConstruction": false});
        atStep(st, 835);
        cqExtrude(c, st, 835, 834, {"until": 3.7437670300627457, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 835, "extrude");
        atStep(st, 836);
        cqUnion(c, st, 836, 832, {"toUnion": {"wp": 835}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 836, "union");
        atStep(st, 837);
        cqWorkplaneNew(c, st, 837, {"plane": {"origin": [-1.9001225316208663, 7.137756576369647, 0.605535180826398], "xDir": [-0.48726723348534046, 0.8732529090541541, 0], "zDir": [0.8502719284060609, 0.4744440539149394, -0.2279045578074221]}, "obj": undefined});
        atStep(st, 838);
        cqCircle(c, st, 838, 837, {"radius": 0.2, "forConstruction": false});
        atStep(st, 839);
        cqExtrude(c, st, 839, 838, {"until": 5.223676197818964, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 839, "extrude");
        atStep(st, 840);
        cqUnion(c, st, 840, 836, {"toUnion": {"wp": 839}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 840, "union");
        atStep(st, 841);
        cqWorkplaneNew(c, st, 841, {"plane": {"origin": [-0.41142498683555734, 4.121602060218586, 3.7769511764023314], "xDir": [0, -0.8017375399854805, 0.597676264358917], "zDir": [0.4368168782164846, -0.5376399534918889, -0.72120336612806]}, "obj": undefined});
        atStep(st, 842);
        cqCircle(c, st, 842, 841, {"radius": 0.2, "forConstruction": false});
        atStep(st, 843);
        cqExtrude(c, st, 843, 842, {"until": 4.408272763284256, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 843, "extrude");
        atStep(st, 844);
        cqUnion(c, st, 844, 840, {"toUnion": {"wp": 843}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 844, "union");
        atStep(st, 845);
        cqWorkplaneNew(c, st, 845, {"plane": {"origin": [0.1864126654552541, 2.8987893351096585, 1.1961818897873158], "xDir": [-0.9384014720230603, -1.734723475976807e-18, 0.34554692489581423], "zDir": [-0.34532606615627953, -0.03574780412128892, -0.9378016861434696]}, "obj": undefined});
        atStep(st, 846);
        cqCircle(c, st, 846, 845, {"radius": 0.2, "forConstruction": false});
        atStep(st, 847);
        cqExtrude(c, st, 847, 846, {"until": 2.1137490439908655, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 847, "extrude");
        atStep(st, 848);
        cqUnion(c, st, 848, 844, {"toUnion": {"wp": 847}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 848, "union");
        atStep(st, 849);
        cqWorkplaneNew(c, st, 849, {"plane": {"origin": [1.376997734665546, 3.82087025928754, 2.087392020201781], "xDir": [0.7309430697727867, -0.6824384431955277, -6.9388939039072276e-18], "zDir": [0.6801222997691055, 0.7284623053858689, -0.08231844865331436]}, "obj": undefined});
        atStep(st, 850);
        cqCircle(c, st, 850, 849, {"radius": 0.2, "forConstruction": false});
        atStep(st, 851);
        cqExtrude(c, st, 851, 850, {"until": 2.4278537797954867, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 851, "extrude");
        atStep(st, 852);
        cqUnion(c, st, 852, 848, {"toUnion": {"wp": 851}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 852, "union");
        atStep(st, 853);
        cqWorkplaneNew(c, st, 853, {"plane": {"origin": [0.448614348258781, 4.064766507994662, 1.100519297633594], "xDir": [0, 0.693770890618803, 0.7201957729187204], "zDir": [-0.06546017159793444, 0.7186510846713556, -0.6922828789122645]}, "obj": undefined});
        atStep(st, 854);
        cqCircle(c, st, 854, 853, {"radius": 0.2, "forConstruction": false});
        atStep(st, 855);
        cqExtrude(c, st, 855, 854, {"until": 3.139760736624678, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 855, "extrude");
        atStep(st, 856);
        cqUnion(c, st, 856, 852, {"toUnion": {"wp": 855}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 856, "union");
        atStep(st, 857);
        cqWorkplaneNew(c, st, 857, {"plane": {"origin": [-2.138085358377041, 3.11014546951552, 2.986232413017156], "xDir": [-0.284754558667516, -3.469446951953614e-18, -0.9586004596900984], "zDir": [-0.9567711716332654, 0.061749001872661774, 0.2842111642762096]}, "obj": undefined});
        atStep(st, 858);
        cqCircle(c, st, 858, 857, {"radius": 0.2, "forConstruction": false});
        atStep(st, 859);
        cqExtrude(c, st, 859, 858, {"until": 5.621959408209804, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 859, "extrude");
        atStep(st, 860);
        cqUnion(c, st, 860, 856, {"toUnion": {"wp": 859}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 860, "union");
        atStep(st, 861);
        cqWorkplaneNew(c, st, 861, {"plane": {"origin": [0.08364802715729987, 4.026985564601588, 0.10938058886420354], "xDir": [-0.9756354071479166, 0.21939815933439197, 0], "zDir": [-0.2186986373371324, -0.972524722761704, 0.07979078671220265]}, "obj": undefined});
        atStep(st, 862);
        cqCircle(c, st, 862, 861, {"radius": 0.2, "forConstruction": false});
        atStep(st, 863);
        cqExtrude(c, st, 863, 862, {"until": 2.3978355420599433, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 863, "extrude");
        atStep(st, 864);
        cqUnion(c, st, 864, 860, {"toUnion": {"wp": 863}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 864, "union");
        atStep(st, 865);
        cqWorkplaneNew(c, st, 865, {"plane": {"origin": [-2.0908207234686675, 6.051566258563794, 0.26808824557448485], "xDir": [-0.3323389574137756, -0.9431600168503352, 0], "zDir": [-0.9386214206228116, 0.33073970351048315, 0.0979850869766042]}, "obj": undefined});
        atStep(st, 866);
        cqCircle(c, st, 866, 865, {"radius": 0.2, "forConstruction": false});
        atStep(st, 867);
        cqExtrude(c, st, 867, 866, {"until": 5.192019657537048, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 867, "extrude");
        atStep(st, 868);
        cqUnion(c, st, 868, 864, {"toUnion": {"wp": 867}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 868, "union");
        atStep(st, 869);
        cqWorkplaneNew(c, st, 869, {"plane": {"origin": [0.149990111067496, 6.510144661452731, 1.4083660028561713], "xDir": [-6.938893903907228e-18, 0.7270094151385975, -0.6866274902010803], "zDir": [-0.10157069573903328, 0.683076475128403, 0.723249557824438]}, "obj": undefined});
        atStep(st, 870);
        cqCircle(c, st, 870, 869, {"radius": 0.2, "forConstruction": false});
        atStep(st, 871);
        cqExtrude(c, st, 871, 870, {"until": 3.8566162704360134, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 871, "extrude");
        atStep(st, 872);
        cqUnion(c, st, 872, 868, {"toUnion": {"wp": 871}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 872, "union");
        atStep(st, 873);
        cqWorkplaneNew(c, st, 873, {"plane": {"origin": [1.274233096367592, 4.9490664887794695, 1.000590719278669], "xDir": [0.7283613086997117, 0, -0.6851932603209429], "zDir": [0.6743548868512961, -0.17716024393427068, 0.7168401038927198]}, "obj": undefined});
        atStep(st, 874);
        cqCircle(c, st, 874, 873, {"radius": 0.2, "forConstruction": false});
        atStep(st, 875);
        cqExtrude(c, st, 875, 874, {"until": 2.75339707477047, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 875, "extrude");
        atStep(st, 876);
        cqUnion(c, st, 876, 872, {"toUnion": {"wp": 875}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 876, "union");
        atStep(st, 877);
        cqWorkplaneNew(c, st, 877, {"plane": {"origin": [-2.3530224062721943, 4.88558908567879, 0.3637508377282067], "xDir": [-0.6814321050024229, -0.7318813334632651, -3.4694469519536138e-18], "zDir": [-0.7308393682554408, 0.6804619633791257, 0.05334167416020728]}, "obj": undefined});
        atStep(st, 878);
        cqCircle(c, st, 878, 877, {"radius": 0.2, "forConstruction": false});
        atStep(st, 879);
        cqExtrude(c, st, 879, 878, {"until": 5.950606508285288, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 879, "extrude");
        atStep(st, 880);
        cqUnion(c, st, 880, 876, {"toUnion": {"wp": 879}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 880, "union");
        atStep(st, 881);
        cqWorkplaneNew(c, st, 881, {"plane": {"origin": [1.012031413564065, 3.7830893158944665, 1.096253311432391], "xDir": [-0.6123133724803738, 0.7906151616821621, 0], "zDir": [0.6803919211481653, 0.5269479919411382, 0.5093060449528467]}, "obj": undefined});
        atStep(st, 882);
        cqCircle(c, st, 882, 881, {"radius": 0.2, "forConstruction": false});
        atStep(st, 883);
        cqExtrude(c, st, 883, 882, {"until": 3.499703721352754, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 883, "extrude");
        atStep(st, 884);
        cqUnion(c, st, 884, 880, {"toUnion": {"wp": 883}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 884, "union");
        atStep(st, 885);
        cqWorkplaneNew(c, st, 885, {"plane": {"origin": [-2.5030516794785225, 3.0723645261224464, 1.9950937042477652], "xDir": [-0.6101336226963653, -6.93889390390723e-18, -0.7922985311455585], "zDir": [-0.790250571020712, 0.07185392467127026, 0.6085565286074697]}, "obj": undefined});
        atStep(st, 886);
        cqCircle(c, st, 886, 885, {"radius": 0.2, "forConstruction": false});
        atStep(st, 887);
        cqExtrude(c, st, 887, 886, {"until": 5.8829391817582675, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 887, "extrude");
        atStep(st, 888);
        cqUnion(c, st, 888, 884, {"toUnion": {"wp": 887}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 888, "union");
        atStep(st, 889);
        cqWorkplaneNew(c, st, 889, {"plane": {"origin": [1.0783734974742611, 6.266248412745609, 2.395238725424359], "xDir": [-0.8114685980905786, -0.5843960252370912, 0], "zDir": [0.5716940403607619, -0.7938311375065833, -0.20735971002340747]}, "obj": undefined});
        atStep(st, 890);
        cqCircle(c, st, 890, 889, {"radius": 0.2, "forConstruction": false});
        atStep(st, 891);
        cqExtrude(c, st, 891, 890, {"until": 3.933023281441427, "combine": true, "clean": true, "both": false, "taper": undefined});
        stepVol(c, st, 891, "extrude");
        atStep(st, 892);
        cqUnion(c, st, 892, 888, {"toUnion": {"wp": 891}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 892, "union");
        atStep(st, 893);
        cqCut(c, st, 893, 9, {"toCut": {"wp": 892}, "clean": true, "tol": undefined});
        stepVol(c, st, 893, "cut");
        return mergeMaps(cqMeasure(c, st, {"wp": 893}), {"i": 5587});
    } catch (e) {
        return {"i": 5587, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    { var r = case_5587(newContextWithDefaults(), makeId("q")); r.result_q = undefined; out["c5587"] = r; }
    return out;
}
