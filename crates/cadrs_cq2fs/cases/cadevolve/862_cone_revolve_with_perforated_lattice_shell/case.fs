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

const case_862 = function(c, base) {
    var st = newState(base);
    try {
        atStep(st, 1);
        cqWorkplaneNew(c, st, 1, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 2);
        cqMoveTo(c, st, 2, 1, {"x": 0, "y": 0});
        atStep(st, 3);
        cqLineTo(c, st, 3, 2, {"x": 0, "y": 20, "forConstruction": false});
        atStep(st, 4);
        cqLineTo(c, st, 4, 3, {"x": 10, "y": 0, "forConstruction": false});
        atStep(st, 5);
        cqClose(c, st, 5, 4, {});
        atStep(st, 6);
        cqRevolve(c, st, 6, 5, {"angleDegrees": 360, "axisStart": undefined, "axisEnd": undefined, "combine": true, "clean": true});
        stepVol(c, st, 6, "revolve");
        atStep(st, 7);
        cqWorkplaneNew(c, st, 7, {"plane": {"origin": [0, 0, 0], "xDir": [1, 0, 0], "zDir": [0, -1, 0]}, "obj": undefined});
        atStep(st, 8);
        cqMoveTo(c, st, 8, 7, {"x": 0, "y": 0});
        atStep(st, 9);
        cqLineTo(c, st, 9, 8, {"x": 0, "y": 20, "forConstruction": false});
        atStep(st, 10);
        cqLineTo(c, st, 10, 9, {"x": 9, "y": 0, "forConstruction": false});
        atStep(st, 11);
        cqClose(c, st, 11, 10, {});
        atStep(st, 12);
        cqRevolve(c, st, 12, 11, {"angleDegrees": 360, "axisStart": undefined, "axisEnd": undefined, "combine": true, "clean": true});
        stepVol(c, st, 12, "revolve");
        atStep(st, 13);
        cqCut(c, st, 13, 6, {"toCut": {"wp": 12}, "clean": true, "tol": undefined});
        stepVol(c, st, 13, "cut");
        atStep(st, 14);
        cqWorkplaneNew(c, st, 14, {"plane": {"origin": [6.601129300589238, 0, 6.102885682970026], "xDir": [0, 1, 0], "zDir": [0.894427190999916, 0, 0.447213595499958]}, "obj": undefined});
        atStep(st, 15);
        cqPolyline(c, st, 15, 14, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 16);
        cqClose(c, st, 16, 15, {});
        atStep(st, 17);
        cqExtrude(c, st, 17, 16, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 17, "extrude");
        atStep(st, 18);
        cqWorkplaneNew(c, st, 18, {"plane": {"origin": [5.716745667976085, 3.3005646502946187, 6.102885682970026], "xDir": [-0.49999999999999994, 0.8660254037844388, 0], "zDir": [0.7745966692414834, 0.4472135954999578, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 19);
        cqPolyline(c, st, 19, 18, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 20);
        cqClose(c, st, 20, 19, {});
        atStep(st, 21);
        cqExtrude(c, st, 21, 20, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 21, "extrude");
        atStep(st, 22);
        cqUnion(c, st, 22, 17, {"toUnion": {"wp": 21}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 22, "union");
        atStep(st, 23);
        cqWorkplaneNew(c, st, 23, {"plane": {"origin": [3.30056465029462, 5.716745667976084, 6.102885682970026], "xDir": [-0.8660254037844386, 0.5000000000000001, 0], "zDir": [0.4472135954999581, 0.7745966692414834, 0.447213595499958]}, "obj": undefined});
        atStep(st, 24);
        cqPolyline(c, st, 24, 23, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 25);
        cqClose(c, st, 25, 24, {});
        atStep(st, 26);
        cqExtrude(c, st, 26, 25, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 26, "extrude");
        atStep(st, 27);
        cqUnion(c, st, 27, 22, {"toUnion": {"wp": 26}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 27, "union");
        atStep(st, 28);
        cqWorkplaneNew(c, st, 28, {"plane": {"origin": [4.0420259343622083e-16, 6.601129300589238, 6.102885682970026], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [5.476786982642027e-17, 0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 29);
        cqPolyline(c, st, 29, 28, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 30);
        cqClose(c, st, 30, 29, {});
        atStep(st, 31);
        cqExtrude(c, st, 31, 30, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 31, "extrude");
        atStep(st, 32);
        cqUnion(c, st, 32, 27, {"toUnion": {"wp": 31}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 32, "union");
        atStep(st, 33);
        cqWorkplaneNew(c, st, 33, {"plane": {"origin": [-3.3005646502946178, 5.716745667976085, 6.102885682970026], "xDir": [-0.8660254037844389, -0.49999999999999983, 0], "zDir": [-0.44721359549995776, 0.7745966692414835, 0.447213595499958]}, "obj": undefined});
        atStep(st, 34);
        cqPolyline(c, st, 34, 33, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 35);
        cqClose(c, st, 35, 34, {});
        atStep(st, 36);
        cqExtrude(c, st, 36, 35, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 36, "extrude");
        atStep(st, 37);
        cqUnion(c, st, 37, 32, {"toUnion": {"wp": 36}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 37, "union");
        atStep(st, 38);
        cqWorkplaneNew(c, st, 38, {"plane": {"origin": [-5.716745667976083, 3.3005646502946213, 6.102885682970026], "xDir": [-0.5000000000000003, -0.8660254037844386, 0], "zDir": [-0.7745966692414833, 0.44721359549995826, 0.447213595499958]}, "obj": undefined});
        atStep(st, 39);
        cqPolyline(c, st, 39, 38, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 40);
        cqClose(c, st, 40, 39, {});
        atStep(st, 41);
        cqExtrude(c, st, 41, 40, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 41, "extrude");
        atStep(st, 42);
        cqUnion(c, st, 42, 37, {"toUnion": {"wp": 41}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 42, "union");
        atStep(st, 43);
        cqWorkplaneNew(c, st, 43, {"plane": {"origin": [-6.601129300589238, 8.084051868724419e-16, 6.102885682970026], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [-0.894427190999916, 1.0953573965284054e-16, 0.447213595499958]}, "obj": undefined});
        atStep(st, 44);
        cqPolyline(c, st, 44, 43, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 45);
        cqClose(c, st, 45, 44, {});
        atStep(st, 46);
        cqExtrude(c, st, 46, 45, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 46, "extrude");
        atStep(st, 47);
        cqUnion(c, st, 47, 42, {"toUnion": {"wp": 46}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 47, "union");
        atStep(st, 48);
        cqWorkplaneNew(c, st, 48, {"plane": {"origin": [-5.7167456679760855, -3.3005646502946173, 6.102885682970026], "xDir": [0.4999999999999997, -0.8660254037844388, 0], "zDir": [-0.7745966692414836, -0.4472135954999577, 0.447213595499958]}, "obj": undefined});
        atStep(st, 49);
        cqPolyline(c, st, 49, 48, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 50);
        cqClose(c, st, 50, 49, {});
        atStep(st, 51);
        cqExtrude(c, st, 51, 50, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 51, "extrude");
        atStep(st, 52);
        cqUnion(c, st, 52, 47, {"toUnion": {"wp": 51}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 52, "union");
        atStep(st, 53);
        cqWorkplaneNew(c, st, 53, {"plane": {"origin": [-3.300564650294622, -5.716745667976082, 6.102885682970026], "xDir": [0.8660254037844384, -0.5000000000000004, 0], "zDir": [-0.4472135954999584, -0.7745966692414832, 0.447213595499958]}, "obj": undefined});
        atStep(st, 54);
        cqPolyline(c, st, 54, 53, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 55);
        cqClose(c, st, 55, 54, {});
        atStep(st, 56);
        cqExtrude(c, st, 56, 55, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 56, "extrude");
        atStep(st, 57);
        cqUnion(c, st, 57, 52, {"toUnion": {"wp": 56}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 57, "union");
        atStep(st, 58);
        cqWorkplaneNew(c, st, 58, {"plane": {"origin": [-1.2126077803086625e-15, -6.601129300589238, 6.102885682970026], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [-1.643036094792608e-16, -0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 59);
        cqPolyline(c, st, 59, 58, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 60);
        cqClose(c, st, 60, 59, {});
        atStep(st, 61);
        cqExtrude(c, st, 61, 60, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 61, "extrude");
        atStep(st, 62);
        cqUnion(c, st, 62, 57, {"toUnion": {"wp": 61}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 62, "union");
        atStep(st, 63);
        cqWorkplaneNew(c, st, 63, {"plane": {"origin": [3.3005646502946147, -5.716745667976086, 6.102885682970026], "xDir": [0.866025403784439, 0.4999999999999993, 0], "zDir": [0.4472135954999573, -0.7745966692414837, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 64);
        cqPolyline(c, st, 64, 63, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 65);
        cqClose(c, st, 65, 64, {});
        atStep(st, 66);
        cqExtrude(c, st, 66, 65, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 66, "extrude");
        atStep(st, 67);
        cqUnion(c, st, 67, 62, {"toUnion": {"wp": 66}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 67, "union");
        atStep(st, 68);
        cqWorkplaneNew(c, st, 68, {"plane": {"origin": [5.716745667976082, -3.300564650294622, 6.102885682970026], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [0.7745966692414832, -0.4472135954999584, 0.447213595499958]}, "obj": undefined});
        atStep(st, 69);
        cqPolyline(c, st, 69, 68, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 70);
        cqClose(c, st, 70, 69, {});
        atStep(st, 71);
        cqExtrude(c, st, 71, 70, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 71, "extrude");
        atStep(st, 72);
        cqUnion(c, st, 72, 67, {"toUnion": {"wp": 71}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 72, "union");
        atStep(st, 73);
        cqWorkplaneNew(c, st, 73, {"plane": {"origin": [5.97885602983718, 1.6020296448568208, 6.968911086754464], "xDir": [-0.25881904510252074, 0.9659258262890684, 0], "zDir": [0.8639503235220041, 0.23149479148832816, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 74);
        cqPolyline(c, st, 74, 73, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 75);
        cqClose(c, st, 75, 74, {});
        atStep(st, 76);
        cqExtrude(c, st, 76, 75, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 76, "extrude");
        atStep(st, 77);
        cqUnion(c, st, 77, 72, {"toUnion": {"wp": 76}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 77, "union");
        atStep(st, 78);
        cqWorkplaneNew(c, st, 78, {"plane": {"origin": [4.37682638498036, 4.376826384980359, 6.968911086754464], "xDir": [-0.7071067811865476, 0.7071067811865477, 0], "zDir": [0.632455532033676, 0.6324555320336759, 0.447213595499958]}, "obj": undefined});
        atStep(st, 79);
        cqPolyline(c, st, 79, 78, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 80);
        cqClose(c, st, 80, 79, {});
        atStep(st, 81);
        cqExtrude(c, st, 81, 80, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 81, "extrude");
        atStep(st, 82);
        cqUnion(c, st, 82, 77, {"toUnion": {"wp": 81}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 82, "union");
        atStep(st, 83);
        cqWorkplaneNew(c, st, 83, {"plane": {"origin": [1.602029644856822, 5.978856029837179, 6.968911086754464], "xDir": [-0.9659258262890682, 0.25881904510252096, 0], "zDir": [0.2314947914883284, 0.8639503235220041, 0.447213595499958]}, "obj": undefined});
        atStep(st, 84);
        cqPolyline(c, st, 84, 83, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 85);
        cqClose(c, st, 85, 84, {});
        atStep(st, 86);
        cqExtrude(c, st, 86, 85, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 86, "extrude");
        atStep(st, 87);
        cqUnion(c, st, 87, 82, {"toUnion": {"wp": 86}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 87, "union");
        atStep(st, 88);
        cqWorkplaneNew(c, st, 88, {"plane": {"origin": [-1.60202964485682, 5.97885602983718, 6.968911086754464], "xDir": [-0.9659258262890684, -0.25881904510252063, 0], "zDir": [-0.23149479148832808, 0.8639503235220042, 0.447213595499958]}, "obj": undefined});
        atStep(st, 89);
        cqPolyline(c, st, 89, 88, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 90);
        cqClose(c, st, 90, 89, {});
        atStep(st, 91);
        cqExtrude(c, st, 91, 90, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 91, "extrude");
        atStep(st, 92);
        cqUnion(c, st, 92, 87, {"toUnion": {"wp": 91}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 92, "union");
        atStep(st, 93);
        cqWorkplaneNew(c, st, 93, {"plane": {"origin": [-4.376826384980359, 4.37682638498036, 6.968911086754464], "xDir": [-0.7071067811865477, -0.7071067811865476, 0], "zDir": [-0.6324555320336759, 0.632455532033676, 0.447213595499958]}, "obj": undefined});
        atStep(st, 94);
        cqPolyline(c, st, 94, 93, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 95);
        cqClose(c, st, 95, 94, {});
        atStep(st, 96);
        cqExtrude(c, st, 96, 95, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 96, "extrude");
        atStep(st, 97);
        cqUnion(c, st, 97, 92, {"toUnion": {"wp": 96}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 97, "union");
        atStep(st, 98);
        cqWorkplaneNew(c, st, 98, {"plane": {"origin": [-5.978856029837179, 1.6020296448568223, 6.968911086754464], "xDir": [-0.258819045102521, -0.9659258262890682, -1.3877787807814457e-17], "zDir": [-0.8639503235220041, 0.2314947914883284, 0.447213595499958]}, "obj": undefined});
        atStep(st, 99);
        cqPolyline(c, st, 99, 98, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 100);
        cqClose(c, st, 100, 99, {});
        atStep(st, 101);
        cqExtrude(c, st, 101, 100, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 101, "extrude");
        atStep(st, 102);
        cqUnion(c, st, 102, 97, {"toUnion": {"wp": 101}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 102, "union");
        atStep(st, 103);
        cqWorkplaneNew(c, st, 103, {"plane": {"origin": [-5.97885602983718, -1.6020296448568212, 6.968911086754464], "xDir": [0.2588190451025208, -0.9659258262890684, 1.3877787807814457e-17], "zDir": [-0.8639503235220041, -0.2314947914883282, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 104);
        cqPolyline(c, st, 104, 103, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 105);
        cqClose(c, st, 105, 104, {});
        atStep(st, 106);
        cqExtrude(c, st, 106, 105, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 106, "extrude");
        atStep(st, 107);
        cqUnion(c, st, 107, 102, {"toUnion": {"wp": 106}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 107, "union");
        atStep(st, 108);
        cqWorkplaneNew(c, st, 108, {"plane": {"origin": [-4.37682638498036, -4.376826384980359, 6.968911086754464], "xDir": [0.7071067811865475, -0.7071067811865478, 0], "zDir": [-0.632455532033676, -0.6324555320336758, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 109);
        cqPolyline(c, st, 109, 108, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 110);
        cqClose(c, st, 110, 109, {});
        atStep(st, 111);
        cqExtrude(c, st, 111, 110, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 111, "extrude");
        atStep(st, 112);
        cqUnion(c, st, 112, 107, {"toUnion": {"wp": 111}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 112, "union");
        atStep(st, 113);
        cqWorkplaneNew(c, st, 113, {"plane": {"origin": [-1.6020296448568256, -5.978856029837179, 6.968911086754464], "xDir": [0.965925826289068, -0.25881904510252157, 0], "zDir": [-0.23149479148832883, -0.8639503235220038, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 114);
        cqPolyline(c, st, 114, 113, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 115);
        cqClose(c, st, 115, 114, {});
        atStep(st, 116);
        cqExtrude(c, st, 116, 115, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 116, "extrude");
        atStep(st, 117);
        cqUnion(c, st, 117, 112, {"toUnion": {"wp": 116}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 117, "union");
        atStep(st, 118);
        cqWorkplaneNew(c, st, 118, {"plane": {"origin": [1.602029644856818, -5.978856029837181, 6.968911086754464], "xDir": [0.9659258262890684, 0.2588190451025203, 0], "zDir": [0.23149479148832777, -0.8639503235220042, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 119);
        cqPolyline(c, st, 119, 118, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 120);
        cqClose(c, st, 120, 119, {});
        atStep(st, 121);
        cqExtrude(c, st, 121, 120, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 121, "extrude");
        atStep(st, 122);
        cqUnion(c, st, 122, 117, {"toUnion": {"wp": 121}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 122, "union");
        atStep(st, 123);
        cqWorkplaneNew(c, st, 123, {"plane": {"origin": [4.376826384980355, -4.376826384980364, 6.968911086754464], "xDir": [0.7071067811865483, 0.7071067811865468, 0], "zDir": [0.6324555320336752, -0.6324555320336765, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 124);
        cqPolyline(c, st, 124, 123, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 125);
        cqClose(c, st, 125, 124, {});
        atStep(st, 126);
        cqExtrude(c, st, 126, 125, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 126, "extrude");
        atStep(st, 127);
        cqUnion(c, st, 127, 122, {"toUnion": {"wp": 126}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 127, "union");
        atStep(st, 128);
        cqWorkplaneNew(c, st, 128, {"plane": {"origin": [5.978856029837179, -1.602029644856826, 6.968911086754464], "xDir": [0.25881904510252157, 0.965925826289068, -1.387778780781446e-17], "zDir": [0.8639503235220038, -0.23149479148832888, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 129);
        cqPolyline(c, st, 129, 128, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 130);
        cqClose(c, st, 130, 129, {});
        atStep(st, 131);
        cqExtrude(c, st, 131, 130, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 131, "extrude");
        atStep(st, 132);
        cqUnion(c, st, 132, 127, {"toUnion": {"wp": 131}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 132, "union");
        atStep(st, 133);
        cqWorkplaneNew(c, st, 133, {"plane": {"origin": [5.7784051669940215, 0, 7.834936490538903], "xDir": [0, 1, 0], "zDir": [0.894427190999916, 0, 0.447213595499958]}, "obj": undefined});
        atStep(st, 134);
        cqPolyline(c, st, 134, 133, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 135);
        cqClose(c, st, 135, 134, {});
        atStep(st, 136);
        cqExtrude(c, st, 136, 135, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 136, "extrude");
        atStep(st, 137);
        cqUnion(c, st, 137, 132, {"toUnion": {"wp": 136}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 137, "union");
        atStep(st, 138);
        cqWorkplaneNew(c, st, 138, {"plane": {"origin": [5.004245667976084, 2.8892025834970103, 7.834936490538903], "xDir": [-0.49999999999999994, 0.8660254037844388, 0], "zDir": [0.7745966692414834, 0.4472135954999578, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 139);
        cqPolyline(c, st, 139, 138, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 140);
        cqClose(c, st, 140, 139, {});
        atStep(st, 141);
        cqExtrude(c, st, 141, 140, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 141, "extrude");
        atStep(st, 142);
        cqUnion(c, st, 142, 137, {"toUnion": {"wp": 141}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 142, "union");
        atStep(st, 143);
        cqWorkplaneNew(c, st, 143, {"plane": {"origin": [2.889202583497011, 5.004245667976083, 7.834936490538903], "xDir": [-0.8660254037844386, 0.5000000000000001, 0], "zDir": [0.4472135954999581, 0.7745966692414834, 0.447213595499958]}, "obj": undefined});
        atStep(st, 144);
        cqPolyline(c, st, 144, 143, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 145);
        cqClose(c, st, 145, 144, {});
        atStep(st, 146);
        cqExtrude(c, st, 146, 145, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 146, "extrude");
        atStep(st, 147);
        cqUnion(c, st, 147, 142, {"toUnion": {"wp": 146}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 147, "union");
        atStep(st, 148);
        cqWorkplaneNew(c, st, 148, {"plane": {"origin": [3.5382526959678784e-16, 5.7784051669940215, 7.834936490538903], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [5.476786982642027e-17, 0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 149);
        cqPolyline(c, st, 149, 148, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 150);
        cqClose(c, st, 150, 149, {});
        atStep(st, 151);
        cqExtrude(c, st, 151, 150, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 151, "extrude");
        atStep(st, 152);
        cqUnion(c, st, 152, 147, {"toUnion": {"wp": 151}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 152, "union");
        atStep(st, 153);
        cqWorkplaneNew(c, st, 153, {"plane": {"origin": [-2.8892025834970094, 5.004245667976084, 7.834936490538903], "xDir": [-0.8660254037844389, -0.49999999999999983, 0], "zDir": [-0.44721359549995776, 0.7745966692414835, 0.447213595499958]}, "obj": undefined});
        atStep(st, 154);
        cqPolyline(c, st, 154, 153, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 155);
        cqClose(c, st, 155, 154, {});
        atStep(st, 156);
        cqExtrude(c, st, 156, 155, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 156, "extrude");
        atStep(st, 157);
        cqUnion(c, st, 157, 152, {"toUnion": {"wp": 156}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 157, "union");
        atStep(st, 158);
        cqWorkplaneNew(c, st, 158, {"plane": {"origin": [-5.004245667976083, 2.8892025834970125, 7.834936490538903], "xDir": [-0.5000000000000003, -0.8660254037844386, 0], "zDir": [-0.7745966692414833, 0.44721359549995826, 0.447213595499958]}, "obj": undefined});
        atStep(st, 159);
        cqPolyline(c, st, 159, 158, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 160);
        cqClose(c, st, 160, 159, {});
        atStep(st, 161);
        cqExtrude(c, st, 161, 160, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 161, "extrude");
        atStep(st, 162);
        cqUnion(c, st, 162, 157, {"toUnion": {"wp": 161}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 162, "union");
        atStep(st, 163);
        cqWorkplaneNew(c, st, 163, {"plane": {"origin": [-5.7784051669940215, 7.076505391935758e-16, 7.834936490538903], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [-0.894427190999916, 1.0953573965284054e-16, 0.447213595499958]}, "obj": undefined});
        atStep(st, 164);
        cqPolyline(c, st, 164, 163, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 165);
        cqClose(c, st, 165, 164, {});
        atStep(st, 166);
        cqExtrude(c, st, 166, 165, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 166, "extrude");
        atStep(st, 167);
        cqUnion(c, st, 167, 162, {"toUnion": {"wp": 166}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 167, "union");
        atStep(st, 168);
        cqWorkplaneNew(c, st, 168, {"plane": {"origin": [-5.004245667976085, -2.889202583497009, 7.834936490538903], "xDir": [0.4999999999999997, -0.8660254037844388, 0], "zDir": [-0.7745966692414836, -0.4472135954999577, 0.447213595499958]}, "obj": undefined});
        atStep(st, 169);
        cqPolyline(c, st, 169, 168, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 170);
        cqClose(c, st, 170, 169, {});
        atStep(st, 171);
        cqExtrude(c, st, 171, 170, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 171, "extrude");
        atStep(st, 172);
        cqUnion(c, st, 172, 167, {"toUnion": {"wp": 171}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 172, "union");
        atStep(st, 173);
        cqWorkplaneNew(c, st, 173, {"plane": {"origin": [-2.8892025834970134, -5.0042456679760825, 7.834936490538903], "xDir": [0.8660254037844384, -0.5000000000000004, 0], "zDir": [-0.4472135954999584, -0.7745966692414832, 0.447213595499958]}, "obj": undefined});
        atStep(st, 174);
        cqPolyline(c, st, 174, 173, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 175);
        cqClose(c, st, 175, 174, {});
        atStep(st, 176);
        cqExtrude(c, st, 176, 175, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 176, "extrude");
        atStep(st, 177);
        cqUnion(c, st, 177, 172, {"toUnion": {"wp": 176}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 177, "union");
        atStep(st, 178);
        cqWorkplaneNew(c, st, 178, {"plane": {"origin": [-1.0614758087903633e-15, -5.7784051669940215, 7.834936490538903], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [-1.643036094792608e-16, -0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 179);
        cqPolyline(c, st, 179, 178, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 180);
        cqClose(c, st, 180, 179, {});
        atStep(st, 181);
        cqExtrude(c, st, 181, 180, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 181, "extrude");
        atStep(st, 182);
        cqUnion(c, st, 182, 177, {"toUnion": {"wp": 181}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 182, "union");
        atStep(st, 183);
        cqWorkplaneNew(c, st, 183, {"plane": {"origin": [2.8892025834970068, -5.004245667976086, 7.834936490538903], "xDir": [0.866025403784439, 0.4999999999999993, 0], "zDir": [0.4472135954999573, -0.7745966692414837, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 184);
        cqPolyline(c, st, 184, 183, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 185);
        cqClose(c, st, 185, 184, {});
        atStep(st, 186);
        cqExtrude(c, st, 186, 185, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 186, "extrude");
        atStep(st, 187);
        cqUnion(c, st, 187, 182, {"toUnion": {"wp": 186}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 187, "union");
        atStep(st, 188);
        cqWorkplaneNew(c, st, 188, {"plane": {"origin": [5.0042456679760825, -2.8892025834970134, 7.834936490538903], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [0.7745966692414832, -0.4472135954999584, 0.447213595499958]}, "obj": undefined});
        atStep(st, 189);
        cqPolyline(c, st, 189, 188, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 190);
        cqClose(c, st, 190, 189, {});
        atStep(st, 191);
        cqExtrude(c, st, 191, 190, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 191, "extrude");
        atStep(st, 192);
        cqUnion(c, st, 192, 187, {"toUnion": {"wp": 191}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 192, "union");
        atStep(st, 193);
        cqWorkplaneNew(c, st, 193, {"plane": {"origin": [5.184165541286263, 1.389092970216908, 8.700961894323342], "xDir": [-0.25881904510252074, 0.9659258262890684, 0], "zDir": [0.8639503235220041, 0.23149479148832816, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 194);
        cqPolyline(c, st, 194, 193, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 195);
        cqClose(c, st, 195, 194, {});
        atStep(st, 196);
        cqExtrude(c, st, 196, 195, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 196, "extrude");
        atStep(st, 197);
        cqUnion(c, st, 197, 192, {"toUnion": {"wp": 196}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 197, "union");
        atStep(st, 198);
        cqWorkplaneNew(c, st, 198, {"plane": {"origin": [3.7950725710693543, 3.7950725710693543, 8.700961894323342], "xDir": [-0.7071067811865476, 0.7071067811865477, 0], "zDir": [0.632455532033676, 0.6324555320336759, 0.447213595499958]}, "obj": undefined});
        atStep(st, 199);
        cqPolyline(c, st, 199, 198, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 200);
        cqClose(c, st, 200, 199, {});
        atStep(st, 201);
        cqExtrude(c, st, 201, 200, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 201, "extrude");
        atStep(st, 202);
        cqUnion(c, st, 202, 197, {"toUnion": {"wp": 201}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 202, "union");
        atStep(st, 203);
        cqWorkplaneNew(c, st, 203, {"plane": {"origin": [1.3890929702169093, 5.184165541286262, 8.700961894323342], "xDir": [-0.9659258262890682, 0.25881904510252096, 0], "zDir": [0.2314947914883284, 0.8639503235220041, 0.447213595499958]}, "obj": undefined});
        atStep(st, 204);
        cqPolyline(c, st, 204, 203, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 205);
        cqClose(c, st, 205, 204, {});
        atStep(st, 206);
        cqExtrude(c, st, 206, 205, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 206, "extrude");
        atStep(st, 207);
        cqUnion(c, st, 207, 202, {"toUnion": {"wp": 206}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 207, "union");
        atStep(st, 208);
        cqWorkplaneNew(c, st, 208, {"plane": {"origin": [-1.3890929702169077, 5.184165541286263, 8.700961894323342], "xDir": [-0.9659258262890684, -0.25881904510252063, 0], "zDir": [-0.23149479148832808, 0.8639503235220042, 0.447213595499958]}, "obj": undefined});
        atStep(st, 209);
        cqPolyline(c, st, 209, 208, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 210);
        cqClose(c, st, 210, 209, {});
        atStep(st, 211);
        cqExtrude(c, st, 211, 210, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 211, "extrude");
        atStep(st, 212);
        cqUnion(c, st, 212, 207, {"toUnion": {"wp": 211}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 212, "union");
        atStep(st, 213);
        cqWorkplaneNew(c, st, 213, {"plane": {"origin": [-3.7950725710693543, 3.7950725710693543, 8.700961894323342], "xDir": [-0.7071067811865477, -0.7071067811865476, 0], "zDir": [-0.6324555320336759, 0.632455532033676, 0.447213595499958]}, "obj": undefined});
        atStep(st, 214);
        cqPolyline(c, st, 214, 213, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 215);
        cqClose(c, st, 215, 214, {});
        atStep(st, 216);
        cqExtrude(c, st, 216, 215, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 216, "extrude");
        atStep(st, 217);
        cqUnion(c, st, 217, 212, {"toUnion": {"wp": 216}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 217, "union");
        atStep(st, 218);
        cqWorkplaneNew(c, st, 218, {"plane": {"origin": [-5.184165541286262, 1.3890929702169097, 8.700961894323342], "xDir": [-0.258819045102521, -0.9659258262890682, -1.3877787807814457e-17], "zDir": [-0.8639503235220041, 0.2314947914883284, 0.447213595499958]}, "obj": undefined});
        atStep(st, 219);
        cqPolyline(c, st, 219, 218, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 220);
        cqClose(c, st, 220, 219, {});
        atStep(st, 221);
        cqExtrude(c, st, 221, 220, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 221, "extrude");
        atStep(st, 222);
        cqUnion(c, st, 222, 217, {"toUnion": {"wp": 221}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 222, "union");
        atStep(st, 223);
        cqWorkplaneNew(c, st, 223, {"plane": {"origin": [-5.184165541286263, -1.3890929702169084, 8.700961894323342], "xDir": [0.2588190451025208, -0.9659258262890684, 1.3877787807814457e-17], "zDir": [-0.8639503235220041, -0.2314947914883282, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 224);
        cqPolyline(c, st, 224, 223, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 225);
        cqClose(c, st, 225, 224, {});
        atStep(st, 226);
        cqExtrude(c, st, 226, 225, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 226, "extrude");
        atStep(st, 227);
        cqUnion(c, st, 227, 222, {"toUnion": {"wp": 226}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 227, "union");
        atStep(st, 228);
        cqWorkplaneNew(c, st, 228, {"plane": {"origin": [-3.795072571069355, -3.7950725710693543, 8.700961894323342], "xDir": [0.7071067811865475, -0.7071067811865478, 0], "zDir": [-0.632455532033676, -0.6324555320336758, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 229);
        cqPolyline(c, st, 229, 228, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 230);
        cqClose(c, st, 230, 229, {});
        atStep(st, 231);
        cqExtrude(c, st, 231, 230, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 231, "extrude");
        atStep(st, 232);
        cqUnion(c, st, 232, 227, {"toUnion": {"wp": 231}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 232, "union");
        atStep(st, 233);
        cqWorkplaneNew(c, st, 233, {"plane": {"origin": [-1.3890929702169124, -5.184165541286261, 8.700961894323342], "xDir": [0.965925826289068, -0.25881904510252157, 0], "zDir": [-0.23149479148832883, -0.8639503235220038, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 234);
        cqPolyline(c, st, 234, 233, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 235);
        cqClose(c, st, 235, 234, {});
        atStep(st, 236);
        cqExtrude(c, st, 236, 235, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 236, "extrude");
        atStep(st, 237);
        cqUnion(c, st, 237, 232, {"toUnion": {"wp": 236}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 237, "union");
        atStep(st, 238);
        cqWorkplaneNew(c, st, 238, {"plane": {"origin": [1.3890929702169057, -5.184165541286263, 8.700961894323342], "xDir": [0.9659258262890684, 0.2588190451025203, 0], "zDir": [0.23149479148832777, -0.8639503235220042, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 239);
        cqPolyline(c, st, 239, 238, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 240);
        cqClose(c, st, 240, 239, {});
        atStep(st, 241);
        cqExtrude(c, st, 241, 240, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 241, "extrude");
        atStep(st, 242);
        cqUnion(c, st, 242, 237, {"toUnion": {"wp": 241}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 242, "union");
        atStep(st, 243);
        cqWorkplaneNew(c, st, 243, {"plane": {"origin": [3.7950725710693503, -3.7950725710693582, 8.700961894323342], "xDir": [0.7071067811865483, 0.7071067811865468, 0], "zDir": [0.6324555320336752, -0.6324555320336765, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 244);
        cqPolyline(c, st, 244, 243, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 245);
        cqClose(c, st, 245, 244, {});
        atStep(st, 246);
        cqExtrude(c, st, 246, 245, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 246, "extrude");
        atStep(st, 247);
        cqUnion(c, st, 247, 242, {"toUnion": {"wp": 246}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 247, "union");
        atStep(st, 248);
        cqWorkplaneNew(c, st, 248, {"plane": {"origin": [5.184165541286261, -1.3890929702169126, 8.700961894323342], "xDir": [0.25881904510252157, 0.965925826289068, -1.387778780781446e-17], "zDir": [0.8639503235220038, -0.23149479148832888, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 249);
        cqPolyline(c, st, 249, 248, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 250);
        cqClose(c, st, 250, 249, {});
        atStep(st, 251);
        cqExtrude(c, st, 251, 250, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 251, "extrude");
        atStep(st, 252);
        cqUnion(c, st, 252, 247, {"toUnion": {"wp": 251}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 252, "union");
        atStep(st, 253);
        cqWorkplaneNew(c, st, 253, {"plane": {"origin": [4.955681033398805, 0, 9.56698729810778], "xDir": [0, 1, 0], "zDir": [0.894427190999916, 0, 0.447213595499958]}, "obj": undefined});
        atStep(st, 254);
        cqPolyline(c, st, 254, 253, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 255);
        cqClose(c, st, 255, 254, {});
        atStep(st, 256);
        cqExtrude(c, st, 256, 255, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 256, "extrude");
        atStep(st, 257);
        cqUnion(c, st, 257, 252, {"toUnion": {"wp": 256}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 257, "union");
        atStep(st, 258);
        cqWorkplaneNew(c, st, 258, {"plane": {"origin": [4.291745667976085, 2.477840516699402, 9.56698729810778], "xDir": [-0.49999999999999994, 0.8660254037844388, 0], "zDir": [0.7745966692414834, 0.4472135954999578, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 259);
        cqPolyline(c, st, 259, 258, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 260);
        cqClose(c, st, 260, 259, {});
        atStep(st, 261);
        cqExtrude(c, st, 261, 260, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 261, "extrude");
        atStep(st, 262);
        cqUnion(c, st, 262, 257, {"toUnion": {"wp": 261}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 262, "union");
        atStep(st, 263);
        cqWorkplaneNew(c, st, 263, {"plane": {"origin": [2.477840516699403, 4.291745667976084, 9.56698729810778], "xDir": [-0.8660254037844386, 0.5000000000000001, 0], "zDir": [0.4472135954999581, 0.7745966692414834, 0.447213595499958]}, "obj": undefined});
        atStep(st, 264);
        cqPolyline(c, st, 264, 263, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 265);
        cqClose(c, st, 265, 264, {});
        atStep(st, 266);
        cqExtrude(c, st, 266, 265, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 266, "extrude");
        atStep(st, 267);
        cqUnion(c, st, 267, 262, {"toUnion": {"wp": 266}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 267, "union");
        atStep(st, 268);
        cqWorkplaneNew(c, st, 268, {"plane": {"origin": [3.034479457573547e-16, 4.955681033398805, 9.56698729810778], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [5.476786982642027e-17, 0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 269);
        cqPolyline(c, st, 269, 268, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 270);
        cqClose(c, st, 270, 269, {});
        atStep(st, 271);
        cqExtrude(c, st, 271, 270, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 271, "extrude");
        atStep(st, 272);
        cqUnion(c, st, 272, 267, {"toUnion": {"wp": 271}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 272, "union");
        atStep(st, 273);
        cqWorkplaneNew(c, st, 273, {"plane": {"origin": [-2.4778405166994015, 4.291745667976085, 9.56698729810778], "xDir": [-0.8660254037844389, -0.49999999999999983, 0], "zDir": [-0.44721359549995776, 0.7745966692414835, 0.447213595499958]}, "obj": undefined});
        atStep(st, 274);
        cqPolyline(c, st, 274, 273, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 275);
        cqClose(c, st, 275, 274, {});
        atStep(st, 276);
        cqExtrude(c, st, 276, 275, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 276, "extrude");
        atStep(st, 277);
        cqUnion(c, st, 277, 272, {"toUnion": {"wp": 276}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 277, "union");
        atStep(st, 278);
        cqWorkplaneNew(c, st, 278, {"plane": {"origin": [-4.291745667976083, 2.477840516699404, 9.56698729810778], "xDir": [-0.5000000000000003, -0.8660254037844386, 0], "zDir": [-0.7745966692414833, 0.44721359549995826, 0.447213595499958]}, "obj": undefined});
        atStep(st, 279);
        cqPolyline(c, st, 279, 278, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 280);
        cqClose(c, st, 280, 279, {});
        atStep(st, 281);
        cqExtrude(c, st, 281, 280, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 281, "extrude");
        atStep(st, 282);
        cqUnion(c, st, 282, 277, {"toUnion": {"wp": 281}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 282, "union");
        atStep(st, 283);
        cqWorkplaneNew(c, st, 283, {"plane": {"origin": [-4.955681033398805, 6.068958915147094e-16, 9.56698729810778], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [-0.894427190999916, 1.0953573965284054e-16, 0.447213595499958]}, "obj": undefined});
        atStep(st, 284);
        cqPolyline(c, st, 284, 283, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 285);
        cqClose(c, st, 285, 284, {});
        atStep(st, 286);
        cqExtrude(c, st, 286, 285, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 286, "extrude");
        atStep(st, 287);
        cqUnion(c, st, 287, 282, {"toUnion": {"wp": 286}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 287, "union");
        atStep(st, 288);
        cqWorkplaneNew(c, st, 288, {"plane": {"origin": [-4.291745667976085, -2.477840516699401, 9.56698729810778], "xDir": [0.4999999999999997, -0.8660254037844388, 0], "zDir": [-0.7745966692414836, -0.4472135954999577, 0.447213595499958]}, "obj": undefined});
        atStep(st, 289);
        cqPolyline(c, st, 289, 288, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 290);
        cqClose(c, st, 290, 289, {});
        atStep(st, 291);
        cqExtrude(c, st, 291, 290, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 291, "extrude");
        atStep(st, 292);
        cqUnion(c, st, 292, 287, {"toUnion": {"wp": 291}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 292, "union");
        atStep(st, 293);
        cqWorkplaneNew(c, st, 293, {"plane": {"origin": [-2.4778405166994046, -4.291745667976083, 9.56698729810778], "xDir": [0.8660254037844384, -0.5000000000000004, 0], "zDir": [-0.4472135954999584, -0.7745966692414832, 0.447213595499958]}, "obj": undefined});
        atStep(st, 294);
        cqPolyline(c, st, 294, 293, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 295);
        cqClose(c, st, 295, 294, {});
        atStep(st, 296);
        cqExtrude(c, st, 296, 295, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 296, "extrude");
        atStep(st, 297);
        cqUnion(c, st, 297, 292, {"toUnion": {"wp": 296}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 297, "union");
        atStep(st, 298);
        cqWorkplaneNew(c, st, 298, {"plane": {"origin": [-9.10343837272064e-16, -4.955681033398805, 9.56698729810778], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [-1.643036094792608e-16, -0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 299);
        cqPolyline(c, st, 299, 298, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 300);
        cqClose(c, st, 300, 299, {});
        atStep(st, 301);
        cqExtrude(c, st, 301, 300, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 301, "extrude");
        atStep(st, 302);
        cqUnion(c, st, 302, 297, {"toUnion": {"wp": 301}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 302, "union");
        atStep(st, 303);
        cqWorkplaneNew(c, st, 303, {"plane": {"origin": [2.4778405166993993, -4.291745667976086, 9.56698729810778], "xDir": [0.866025403784439, 0.4999999999999993, 0], "zDir": [0.4472135954999573, -0.7745966692414837, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 304);
        cqPolyline(c, st, 304, 303, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 305);
        cqClose(c, st, 305, 304, {});
        atStep(st, 306);
        cqExtrude(c, st, 306, 305, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 306, "extrude");
        atStep(st, 307);
        cqUnion(c, st, 307, 302, {"toUnion": {"wp": 306}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 307, "union");
        atStep(st, 308);
        cqWorkplaneNew(c, st, 308, {"plane": {"origin": [4.291745667976083, -2.4778405166994046, 9.56698729810778], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [0.7745966692414832, -0.4472135954999584, 0.447213595499958]}, "obj": undefined});
        atStep(st, 309);
        cqPolyline(c, st, 309, 308, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 310);
        cqClose(c, st, 310, 309, {});
        atStep(st, 311);
        cqExtrude(c, st, 311, 310, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 311, "extrude");
        atStep(st, 312);
        cqUnion(c, st, 312, 307, {"toUnion": {"wp": 311}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 312, "union");
        atStep(st, 313);
        cqWorkplaneNew(c, st, 313, {"plane": {"origin": [4.389475052735345, 1.1761562955769953, 10.43301270189222], "xDir": [-0.25881904510252074, 0.9659258262890684, 0], "zDir": [0.8639503235220041, 0.23149479148832816, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 314);
        cqPolyline(c, st, 314, 313, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 315);
        cqClose(c, st, 315, 314, {});
        atStep(st, 316);
        cqExtrude(c, st, 316, 315, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 316, "extrude");
        atStep(st, 317);
        cqUnion(c, st, 317, 312, {"toUnion": {"wp": 316}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 317, "union");
        atStep(st, 318);
        cqWorkplaneNew(c, st, 318, {"plane": {"origin": [3.21331875715835, 3.2133187571583495, 10.43301270189222], "xDir": [-0.7071067811865476, 0.7071067811865477, 0], "zDir": [0.632455532033676, 0.6324555320336759, 0.447213595499958]}, "obj": undefined});
        atStep(st, 319);
        cqPolyline(c, st, 319, 318, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 320);
        cqClose(c, st, 320, 319, {});
        atStep(st, 321);
        cqExtrude(c, st, 321, 320, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 321, "extrude");
        atStep(st, 322);
        cqUnion(c, st, 322, 317, {"toUnion": {"wp": 321}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 322, "union");
        atStep(st, 323);
        cqWorkplaneNew(c, st, 323, {"plane": {"origin": [1.1761562955769964, 4.389475052735345, 10.43301270189222], "xDir": [-0.9659258262890682, 0.25881904510252096, 0], "zDir": [0.2314947914883284, 0.8639503235220041, 0.447213595499958]}, "obj": undefined});
        atStep(st, 324);
        cqPolyline(c, st, 324, 323, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 325);
        cqClose(c, st, 325, 324, {});
        atStep(st, 326);
        cqExtrude(c, st, 326, 325, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 326, "extrude");
        atStep(st, 327);
        cqUnion(c, st, 327, 322, {"toUnion": {"wp": 326}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 327, "union");
        atStep(st, 328);
        cqWorkplaneNew(c, st, 328, {"plane": {"origin": [-1.1761562955769949, 4.389475052735345, 10.43301270189222], "xDir": [-0.9659258262890684, -0.25881904510252063, 0], "zDir": [-0.23149479148832808, 0.8639503235220042, 0.447213595499958]}, "obj": undefined});
        atStep(st, 329);
        cqPolyline(c, st, 329, 328, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 330);
        cqClose(c, st, 330, 329, {});
        atStep(st, 331);
        cqExtrude(c, st, 331, 330, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 331, "extrude");
        atStep(st, 332);
        cqUnion(c, st, 332, 327, {"toUnion": {"wp": 331}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 332, "union");
        atStep(st, 333);
        cqWorkplaneNew(c, st, 333, {"plane": {"origin": [-3.2133187571583495, 3.21331875715835, 10.43301270189222], "xDir": [-0.7071067811865477, -0.7071067811865476, 0], "zDir": [-0.6324555320336759, 0.632455532033676, 0.447213595499958]}, "obj": undefined});
        atStep(st, 334);
        cqPolyline(c, st, 334, 333, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 335);
        cqClose(c, st, 335, 334, {});
        atStep(st, 336);
        cqExtrude(c, st, 336, 335, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 336, "extrude");
        atStep(st, 337);
        cqUnion(c, st, 337, 332, {"toUnion": {"wp": 336}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 337, "union");
        atStep(st, 338);
        cqWorkplaneNew(c, st, 338, {"plane": {"origin": [-4.389475052735345, 1.1761562955769966, 10.43301270189222], "xDir": [-0.258819045102521, -0.9659258262890682, -1.3877787807814457e-17], "zDir": [-0.8639503235220041, 0.2314947914883284, 0.447213595499958]}, "obj": undefined});
        atStep(st, 339);
        cqPolyline(c, st, 339, 338, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 340);
        cqClose(c, st, 340, 339, {});
        atStep(st, 341);
        cqExtrude(c, st, 341, 340, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 341, "extrude");
        atStep(st, 342);
        cqUnion(c, st, 342, 337, {"toUnion": {"wp": 341}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 342, "union");
        atStep(st, 343);
        cqWorkplaneNew(c, st, 343, {"plane": {"origin": [-4.389475052735345, -1.1761562955769955, 10.43301270189222], "xDir": [0.2588190451025208, -0.9659258262890684, 1.3877787807814457e-17], "zDir": [-0.8639503235220041, -0.2314947914883282, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 344);
        cqPolyline(c, st, 344, 343, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 345);
        cqClose(c, st, 345, 344, {});
        atStep(st, 346);
        cqExtrude(c, st, 346, 345, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 346, "extrude");
        atStep(st, 347);
        cqUnion(c, st, 347, 342, {"toUnion": {"wp": 346}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 347, "union");
        atStep(st, 348);
        cqWorkplaneNew(c, st, 348, {"plane": {"origin": [-3.2133187571583504, -3.2133187571583495, 10.43301270189222], "xDir": [0.7071067811865475, -0.7071067811865478, 0], "zDir": [-0.632455532033676, -0.6324555320336758, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 349);
        cqPolyline(c, st, 349, 348, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 350);
        cqClose(c, st, 350, 349, {});
        atStep(st, 351);
        cqExtrude(c, st, 351, 350, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 351, "extrude");
        atStep(st, 352);
        cqUnion(c, st, 352, 347, {"toUnion": {"wp": 351}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 352, "union");
        atStep(st, 353);
        cqWorkplaneNew(c, st, 353, {"plane": {"origin": [-1.1761562955769989, -4.389475052735344, 10.43301270189222], "xDir": [0.965925826289068, -0.25881904510252157, 0], "zDir": [-0.23149479148832883, -0.8639503235220038, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 354);
        cqPolyline(c, st, 354, 353, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 355);
        cqClose(c, st, 355, 354, {});
        atStep(st, 356);
        cqExtrude(c, st, 356, 355, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 356, "extrude");
        atStep(st, 357);
        cqUnion(c, st, 357, 352, {"toUnion": {"wp": 356}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 357, "union");
        atStep(st, 358);
        cqWorkplaneNew(c, st, 358, {"plane": {"origin": [1.1761562955769933, -4.389475052735346, 10.43301270189222], "xDir": [0.9659258262890684, 0.2588190451025203, 0], "zDir": [0.23149479148832777, -0.8639503235220042, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 359);
        cqPolyline(c, st, 359, 358, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 360);
        cqClose(c, st, 360, 359, {});
        atStep(st, 361);
        cqExtrude(c, st, 361, 360, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 361, "extrude");
        atStep(st, 362);
        cqUnion(c, st, 362, 357, {"toUnion": {"wp": 361}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 362, "union");
        atStep(st, 363);
        cqWorkplaneNew(c, st, 363, {"plane": {"origin": [3.2133187571583464, -3.2133187571583535, 10.43301270189222], "xDir": [0.7071067811865483, 0.7071067811865468, 0], "zDir": [0.6324555320336752, -0.6324555320336765, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 364);
        cqPolyline(c, st, 364, 363, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 365);
        cqClose(c, st, 365, 364, {});
        atStep(st, 366);
        cqExtrude(c, st, 366, 365, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 366, "extrude");
        atStep(st, 367);
        cqUnion(c, st, 367, 362, {"toUnion": {"wp": 366}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 367, "union");
        atStep(st, 368);
        cqWorkplaneNew(c, st, 368, {"plane": {"origin": [4.389475052735344, -1.176156295576999, 10.43301270189222], "xDir": [0.25881904510252157, 0.965925826289068, -1.387778780781446e-17], "zDir": [0.8639503235220038, -0.23149479148832888, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 369);
        cqPolyline(c, st, 369, 368, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 370);
        cqClose(c, st, 370, 369, {});
        atStep(st, 371);
        cqExtrude(c, st, 371, 370, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 371, "extrude");
        atStep(st, 372);
        cqUnion(c, st, 372, 367, {"toUnion": {"wp": 371}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 372, "union");
        atStep(st, 373);
        cqWorkplaneNew(c, st, 373, {"plane": {"origin": [4.132956899803587, 0, 11.299038105676658], "xDir": [0, 1, 0], "zDir": [0.894427190999916, 0, 0.447213595499958]}, "obj": undefined});
        atStep(st, 374);
        cqPolyline(c, st, 374, 373, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 375);
        cqClose(c, st, 375, 374, {});
        atStep(st, 376);
        cqExtrude(c, st, 376, 375, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 376, "extrude");
        atStep(st, 377);
        cqUnion(c, st, 377, 372, {"toUnion": {"wp": 376}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 377, "union");
        atStep(st, 378);
        cqWorkplaneNew(c, st, 378, {"plane": {"origin": [3.5792456679760836, 2.066478449901793, 11.299038105676658], "xDir": [-0.49999999999999994, 0.8660254037844388, 0], "zDir": [0.7745966692414834, 0.4472135954999578, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 379);
        cqPolyline(c, st, 379, 378, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 380);
        cqClose(c, st, 380, 379, {});
        atStep(st, 381);
        cqExtrude(c, st, 381, 380, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 381, "extrude");
        atStep(st, 382);
        cqUnion(c, st, 382, 377, {"toUnion": {"wp": 381}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 382, "union");
        atStep(st, 383);
        cqWorkplaneNew(c, st, 383, {"plane": {"origin": [2.066478449901794, 3.579245667976083, 11.299038105676658], "xDir": [-0.8660254037844386, 0.5000000000000001, 0], "zDir": [0.4472135954999581, 0.7745966692414834, 0.447213595499958]}, "obj": undefined});
        atStep(st, 384);
        cqPolyline(c, st, 384, 383, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 385);
        cqClose(c, st, 385, 384, {});
        atStep(st, 386);
        cqExtrude(c, st, 386, 385, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 386, "extrude");
        atStep(st, 387);
        cqUnion(c, st, 387, 382, {"toUnion": {"wp": 386}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 387, "union");
        atStep(st, 388);
        cqWorkplaneNew(c, st, 388, {"plane": {"origin": [2.5307062191792155e-16, 4.132956899803587, 11.299038105676658], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [5.476786982642027e-17, 0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 389);
        cqPolyline(c, st, 389, 388, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 390);
        cqClose(c, st, 390, 389, {});
        atStep(st, 391);
        cqExtrude(c, st, 391, 390, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 391, "extrude");
        atStep(st, 392);
        cqUnion(c, st, 392, 387, {"toUnion": {"wp": 391}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 392, "union");
        atStep(st, 393);
        cqWorkplaneNew(c, st, 393, {"plane": {"origin": [-2.0664784499017927, 3.5792456679760836, 11.299038105676658], "xDir": [-0.8660254037844389, -0.49999999999999983, 0], "zDir": [-0.44721359549995776, 0.7745966692414835, 0.447213595499958]}, "obj": undefined});
        atStep(st, 394);
        cqPolyline(c, st, 394, 393, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 395);
        cqClose(c, st, 395, 394, {});
        atStep(st, 396);
        cqExtrude(c, st, 396, 395, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 396, "extrude");
        atStep(st, 397);
        cqUnion(c, st, 397, 392, {"toUnion": {"wp": 396}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 397, "union");
        atStep(st, 398);
        cqWorkplaneNew(c, st, 398, {"plane": {"origin": [-3.5792456679760827, 2.066478449901795, 11.299038105676658], "xDir": [-0.5000000000000003, -0.8660254037844386, 0], "zDir": [-0.7745966692414833, 0.44721359549995826, 0.447213595499958]}, "obj": undefined});
        atStep(st, 399);
        cqPolyline(c, st, 399, 398, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 400);
        cqClose(c, st, 400, 399, {});
        atStep(st, 401);
        cqExtrude(c, st, 401, 400, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 401, "extrude");
        atStep(st, 402);
        cqUnion(c, st, 402, 397, {"toUnion": {"wp": 401}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 402, "union");
        atStep(st, 403);
        cqWorkplaneNew(c, st, 403, {"plane": {"origin": [-4.132956899803587, 5.061412438358431e-16, 11.299038105676658], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [-0.894427190999916, 1.0953573965284054e-16, 0.447213595499958]}, "obj": undefined});
        atStep(st, 404);
        cqPolyline(c, st, 404, 403, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 405);
        cqClose(c, st, 405, 404, {});
        atStep(st, 406);
        cqExtrude(c, st, 406, 405, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 406, "extrude");
        atStep(st, 407);
        cqUnion(c, st, 407, 402, {"toUnion": {"wp": 406}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 407, "union");
        atStep(st, 408);
        cqWorkplaneNew(c, st, 408, {"plane": {"origin": [-3.579245667976084, -2.0664784499017923, 11.299038105676658], "xDir": [0.4999999999999997, -0.8660254037844388, 0], "zDir": [-0.7745966692414836, -0.4472135954999577, 0.447213595499958]}, "obj": undefined});
        atStep(st, 409);
        cqPolyline(c, st, 409, 408, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 410);
        cqClose(c, st, 410, 409, {});
        atStep(st, 411);
        cqExtrude(c, st, 411, 410, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 411, "extrude");
        atStep(st, 412);
        cqUnion(c, st, 412, 407, {"toUnion": {"wp": 411}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 412, "union");
        atStep(st, 413);
        cqWorkplaneNew(c, st, 413, {"plane": {"origin": [-2.0664784499017954, -3.5792456679760822, 11.299038105676658], "xDir": [0.8660254037844384, -0.5000000000000004, 0], "zDir": [-0.4472135954999584, -0.7745966692414832, 0.447213595499958]}, "obj": undefined});
        atStep(st, 414);
        cqPolyline(c, st, 414, 413, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 415);
        cqClose(c, st, 415, 414, {});
        atStep(st, 416);
        cqExtrude(c, st, 416, 415, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 416, "extrude");
        atStep(st, 417);
        cqUnion(c, st, 417, 412, {"toUnion": {"wp": 416}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 417, "union");
        atStep(st, 418);
        cqWorkplaneNew(c, st, 418, {"plane": {"origin": [-7.592118657537646e-16, -4.132956899803587, 11.299038105676658], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [-1.643036094792608e-16, -0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 419);
        cqPolyline(c, st, 419, 418, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 420);
        cqClose(c, st, 420, 419, {});
        atStep(st, 421);
        cqExtrude(c, st, 421, 420, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 421, "extrude");
        atStep(st, 422);
        cqUnion(c, st, 422, 417, {"toUnion": {"wp": 421}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 422, "union");
        atStep(st, 423);
        cqWorkplaneNew(c, st, 423, {"plane": {"origin": [2.066478449901791, -3.579245667976085, 11.299038105676658], "xDir": [0.866025403784439, 0.4999999999999993, 0], "zDir": [0.4472135954999573, -0.7745966692414837, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 424);
        cqPolyline(c, st, 424, 423, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 425);
        cqClose(c, st, 425, 424, {});
        atStep(st, 426);
        cqExtrude(c, st, 426, 425, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 426, "extrude");
        atStep(st, 427);
        cqUnion(c, st, 427, 422, {"toUnion": {"wp": 426}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 427, "union");
        atStep(st, 428);
        cqWorkplaneNew(c, st, 428, {"plane": {"origin": [3.5792456679760822, -2.0664784499017954, 11.299038105676658], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [0.7745966692414832, -0.4472135954999584, 0.447213595499958]}, "obj": undefined});
        atStep(st, 429);
        cqPolyline(c, st, 429, 428, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 430);
        cqClose(c, st, 430, 429, {});
        atStep(st, 431);
        cqExtrude(c, st, 431, 430, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 431, "extrude");
        atStep(st, 432);
        cqUnion(c, st, 432, 427, {"toUnion": {"wp": 431}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 432, "union");
        atStep(st, 433);
        cqWorkplaneNew(c, st, 433, {"plane": {"origin": [3.594784564184428, 0.9632196209370828, 12.165063509461095], "xDir": [-0.25881904510252074, 0.9659258262890684, 0], "zDir": [0.8639503235220041, 0.23149479148832816, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 434);
        cqPolyline(c, st, 434, 433, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 435);
        cqClose(c, st, 435, 434, {});
        atStep(st, 436);
        cqExtrude(c, st, 436, 435, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 436, "extrude");
        atStep(st, 437);
        cqUnion(c, st, 437, 432, {"toUnion": {"wp": 436}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 437, "union");
        atStep(st, 438);
        cqWorkplaneNew(c, st, 438, {"plane": {"origin": [2.631564943247345, 2.6315649432473447, 12.165063509461095], "xDir": [-0.7071067811865476, 0.7071067811865477, 0], "zDir": [0.632455532033676, 0.6324555320336759, 0.447213595499958]}, "obj": undefined});
        atStep(st, 439);
        cqPolyline(c, st, 439, 438, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 440);
        cqClose(c, st, 440, 439, {});
        atStep(st, 441);
        cqExtrude(c, st, 441, 440, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 441, "extrude");
        atStep(st, 442);
        cqUnion(c, st, 442, 437, {"toUnion": {"wp": 441}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 442, "union");
        atStep(st, 443);
        cqWorkplaneNew(c, st, 443, {"plane": {"origin": [0.9632196209370836, 3.5947845641844274, 12.165063509461095], "xDir": [-0.9659258262890682, 0.25881904510252096, 0], "zDir": [0.2314947914883284, 0.8639503235220041, 0.447213595499958]}, "obj": undefined});
        atStep(st, 444);
        cqPolyline(c, st, 444, 443, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 445);
        cqClose(c, st, 445, 444, {});
        atStep(st, 446);
        cqExtrude(c, st, 446, 445, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 446, "extrude");
        atStep(st, 447);
        cqUnion(c, st, 447, 442, {"toUnion": {"wp": 446}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 447, "union");
        atStep(st, 448);
        cqWorkplaneNew(c, st, 448, {"plane": {"origin": [-0.9632196209370824, 3.594784564184428, 12.165063509461095], "xDir": [-0.9659258262890684, -0.25881904510252063, 0], "zDir": [-0.23149479148832808, 0.8639503235220042, 0.447213595499958]}, "obj": undefined});
        atStep(st, 449);
        cqPolyline(c, st, 449, 448, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 450);
        cqClose(c, st, 450, 449, {});
        atStep(st, 451);
        cqExtrude(c, st, 451, 450, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 451, "extrude");
        atStep(st, 452);
        cqUnion(c, st, 452, 447, {"toUnion": {"wp": 451}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 452, "union");
        atStep(st, 453);
        cqWorkplaneNew(c, st, 453, {"plane": {"origin": [-2.6315649432473447, 2.631564943247345, 12.165063509461095], "xDir": [-0.7071067811865477, -0.7071067811865476, 0], "zDir": [-0.6324555320336759, 0.632455532033676, 0.447213595499958]}, "obj": undefined});
        atStep(st, 454);
        cqPolyline(c, st, 454, 453, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 455);
        cqClose(c, st, 455, 454, {});
        atStep(st, 456);
        cqExtrude(c, st, 456, 455, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 456, "extrude");
        atStep(st, 457);
        cqUnion(c, st, 457, 452, {"toUnion": {"wp": 456}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 457, "union");
        atStep(st, 458);
        cqWorkplaneNew(c, st, 458, {"plane": {"origin": [-3.5947845641844274, 0.9632196209370838, 12.165063509461095], "xDir": [-0.258819045102521, -0.9659258262890682, -1.3877787807814457e-17], "zDir": [-0.8639503235220041, 0.2314947914883284, 0.447213595499958]}, "obj": undefined});
        atStep(st, 459);
        cqPolyline(c, st, 459, 458, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 460);
        cqClose(c, st, 460, 459, {});
        atStep(st, 461);
        cqExtrude(c, st, 461, 460, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 461, "extrude");
        atStep(st, 462);
        cqUnion(c, st, 462, 457, {"toUnion": {"wp": 461}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 462, "union");
        atStep(st, 463);
        cqWorkplaneNew(c, st, 463, {"plane": {"origin": [-3.594784564184428, -0.9632196209370828, 12.165063509461095], "xDir": [0.2588190451025208, -0.9659258262890684, 1.3877787807814457e-17], "zDir": [-0.8639503235220041, -0.2314947914883282, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 464);
        cqPolyline(c, st, 464, 463, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 465);
        cqClose(c, st, 465, 464, {});
        atStep(st, 466);
        cqExtrude(c, st, 466, 465, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 466, "extrude");
        atStep(st, 467);
        cqUnion(c, st, 467, 462, {"toUnion": {"wp": 466}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 467, "union");
        atStep(st, 468);
        cqWorkplaneNew(c, st, 468, {"plane": {"origin": [-2.6315649432473456, -2.6315649432473447, 12.165063509461095], "xDir": [0.7071067811865475, -0.7071067811865478, 0], "zDir": [-0.632455532033676, -0.6324555320336758, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 469);
        cqPolyline(c, st, 469, 468, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 470);
        cqClose(c, st, 470, 469, {});
        atStep(st, 471);
        cqExtrude(c, st, 471, 470, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 471, "extrude");
        atStep(st, 472);
        cqUnion(c, st, 472, 467, {"toUnion": {"wp": 471}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 472, "union");
        atStep(st, 473);
        cqWorkplaneNew(c, st, 473, {"plane": {"origin": [-0.9632196209370856, -3.594784564184427, 12.165063509461095], "xDir": [0.965925826289068, -0.25881904510252157, 0], "zDir": [-0.23149479148832883, -0.8639503235220038, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 474);
        cqPolyline(c, st, 474, 473, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 475);
        cqClose(c, st, 475, 474, {});
        atStep(st, 476);
        cqExtrude(c, st, 476, 475, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 476, "extrude");
        atStep(st, 477);
        cqUnion(c, st, 477, 472, {"toUnion": {"wp": 476}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 477, "union");
        atStep(st, 478);
        cqWorkplaneNew(c, st, 478, {"plane": {"origin": [0.963219620937081, -3.5947845641844283, 12.165063509461095], "xDir": [0.9659258262890684, 0.2588190451025203, 0], "zDir": [0.23149479148832777, -0.8639503235220042, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 479);
        cqPolyline(c, st, 479, 478, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 480);
        cqClose(c, st, 480, 479, {});
        atStep(st, 481);
        cqExtrude(c, st, 481, 480, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 481, "extrude");
        atStep(st, 482);
        cqUnion(c, st, 482, 477, {"toUnion": {"wp": 481}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 482, "union");
        atStep(st, 483);
        cqWorkplaneNew(c, st, 483, {"plane": {"origin": [2.631564943247342, -2.631564943247348, 12.165063509461095], "xDir": [0.7071067811865483, 0.7071067811865468, 0], "zDir": [0.6324555320336752, -0.6324555320336765, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 484);
        cqPolyline(c, st, 484, 483, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 485);
        cqClose(c, st, 485, 484, {});
        atStep(st, 486);
        cqExtrude(c, st, 486, 485, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 486, "extrude");
        atStep(st, 487);
        cqUnion(c, st, 487, 482, {"toUnion": {"wp": 486}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 487, "union");
        atStep(st, 488);
        cqWorkplaneNew(c, st, 488, {"plane": {"origin": [3.594784564184427, -0.9632196209370858, 12.165063509461095], "xDir": [0.25881904510252157, 0.965925826289068, -1.387778780781446e-17], "zDir": [0.8639503235220038, -0.23149479148832888, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 489);
        cqPolyline(c, st, 489, 488, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 490);
        cqClose(c, st, 490, 489, {});
        atStep(st, 491);
        cqExtrude(c, st, 491, 490, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 491, "extrude");
        atStep(st, 492);
        cqUnion(c, st, 492, 487, {"toUnion": {"wp": 491}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 492, "union");
        atStep(st, 493);
        cqWorkplaneNew(c, st, 493, {"plane": {"origin": [3.310232766208371, 0, 13.031088913245537], "xDir": [0, 1, 0], "zDir": [0.894427190999916, 0, 0.447213595499958]}, "obj": undefined});
        atStep(st, 494);
        cqPolyline(c, st, 494, 493, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 495);
        cqClose(c, st, 495, 494, {});
        atStep(st, 496);
        cqExtrude(c, st, 496, 495, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 496, "extrude");
        atStep(st, 497);
        cqUnion(c, st, 497, 492, {"toUnion": {"wp": 496}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 497, "union");
        atStep(st, 498);
        cqWorkplaneNew(c, st, 498, {"plane": {"origin": [2.866745667976084, 1.6551163831041853, 13.031088913245537], "xDir": [-0.49999999999999994, 0.8660254037844388, 0], "zDir": [0.7745966692414834, 0.4472135954999578, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 499);
        cqPolyline(c, st, 499, 498, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 500);
        cqClose(c, st, 500, 499, {});
        atStep(st, 501);
        cqExtrude(c, st, 501, 500, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 501, "extrude");
        atStep(st, 502);
        cqUnion(c, st, 502, 497, {"toUnion": {"wp": 501}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 502, "union");
        atStep(st, 503);
        cqWorkplaneNew(c, st, 503, {"plane": {"origin": [1.655116383104186, 2.8667456679760837, 13.031088913245537], "xDir": [-0.8660254037844386, 0.5000000000000001, 0], "zDir": [0.4472135954999581, 0.7745966692414834, 0.447213595499958]}, "obj": undefined});
        atStep(st, 504);
        cqPolyline(c, st, 504, 503, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 505);
        cqClose(c, st, 505, 504, {});
        atStep(st, 506);
        cqExtrude(c, st, 506, 505, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 506, "extrude");
        atStep(st, 507);
        cqUnion(c, st, 507, 502, {"toUnion": {"wp": 506}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 507, "union");
        atStep(st, 508);
        cqWorkplaneNew(c, st, 508, {"plane": {"origin": [2.026932980784885e-16, 3.310232766208371, 13.031088913245537], "xDir": [-1, 6.123233995736766e-17, 0], "zDir": [5.476786982642027e-17, 0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 509);
        cqPolyline(c, st, 509, 508, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 510);
        cqClose(c, st, 510, 509, {});
        atStep(st, 511);
        cqExtrude(c, st, 511, 510, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 511, "extrude");
        atStep(st, 512);
        cqUnion(c, st, 512, 507, {"toUnion": {"wp": 511}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 512, "union");
        atStep(st, 513);
        cqWorkplaneNew(c, st, 513, {"plane": {"origin": [-1.6551163831041849, 2.866745667976084, 13.031088913245537], "xDir": [-0.8660254037844389, -0.49999999999999983, 0], "zDir": [-0.44721359549995776, 0.7745966692414835, 0.447213595499958]}, "obj": undefined});
        atStep(st, 514);
        cqPolyline(c, st, 514, 513, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 515);
        cqClose(c, st, 515, 514, {});
        atStep(st, 516);
        cqExtrude(c, st, 516, 515, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 516, "extrude");
        atStep(st, 517);
        cqUnion(c, st, 517, 512, {"toUnion": {"wp": 516}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 517, "union");
        atStep(st, 518);
        cqWorkplaneNew(c, st, 518, {"plane": {"origin": [-2.866745667976083, 1.6551163831041866, 13.031088913245537], "xDir": [-0.5000000000000003, -0.8660254037844386, 0], "zDir": [-0.7745966692414833, 0.44721359549995826, 0.447213595499958]}, "obj": undefined});
        atStep(st, 519);
        cqPolyline(c, st, 519, 518, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 520);
        cqClose(c, st, 520, 519, {});
        atStep(st, 521);
        cqExtrude(c, st, 521, 520, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 521, "extrude");
        atStep(st, 522);
        cqUnion(c, st, 522, 517, {"toUnion": {"wp": 521}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 522, "union");
        atStep(st, 523);
        cqWorkplaneNew(c, st, 523, {"plane": {"origin": [-3.310232766208371, 4.053865961569769e-16, 13.031088913245537], "xDir": [-1.2246467991473532e-16, -1, 0], "zDir": [-0.894427190999916, 1.0953573965284054e-16, 0.447213595499958]}, "obj": undefined});
        atStep(st, 524);
        cqPolyline(c, st, 524, 523, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 525);
        cqClose(c, st, 525, 524, {});
        atStep(st, 526);
        cqExtrude(c, st, 526, 525, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 526, "extrude");
        atStep(st, 527);
        cqUnion(c, st, 527, 522, {"toUnion": {"wp": 526}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 527, "union");
        atStep(st, 528);
        cqWorkplaneNew(c, st, 528, {"plane": {"origin": [-2.8667456679760845, -1.6551163831041846, 13.031088913245537], "xDir": [0.4999999999999997, -0.8660254037844388, 0], "zDir": [-0.7745966692414836, -0.4472135954999577, 0.447213595499958]}, "obj": undefined});
        atStep(st, 529);
        cqPolyline(c, st, 529, 528, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 530);
        cqClose(c, st, 530, 529, {});
        atStep(st, 531);
        cqExtrude(c, st, 531, 530, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 531, "extrude");
        atStep(st, 532);
        cqUnion(c, st, 532, 527, {"toUnion": {"wp": 531}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 532, "union");
        atStep(st, 533);
        cqWorkplaneNew(c, st, 533, {"plane": {"origin": [-1.655116383104187, -2.8667456679760828, 13.031088913245537], "xDir": [0.8660254037844384, -0.5000000000000004, 0], "zDir": [-0.4472135954999584, -0.7745966692414832, 0.447213595499958]}, "obj": undefined});
        atStep(st, 534);
        cqPolyline(c, st, 534, 533, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 535);
        cqClose(c, st, 535, 534, {});
        atStep(st, 536);
        cqExtrude(c, st, 536, 535, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 536, "extrude");
        atStep(st, 537);
        cqUnion(c, st, 537, 532, {"toUnion": {"wp": 536}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 537, "union");
        atStep(st, 538);
        cqWorkplaneNew(c, st, 538, {"plane": {"origin": [-6.080798942354655e-16, -3.310232766208371, 13.031088913245537], "xDir": [1, -1.8369701987210294e-16, 0], "zDir": [-1.643036094792608e-16, -0.894427190999916, 0.447213595499958]}, "obj": undefined});
        atStep(st, 539);
        cqPolyline(c, st, 539, 538, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 540);
        cqClose(c, st, 540, 539, {});
        atStep(st, 541);
        cqExtrude(c, st, 541, 540, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 541, "extrude");
        atStep(st, 542);
        cqUnion(c, st, 542, 537, {"toUnion": {"wp": 541}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 542, "union");
        atStep(st, 543);
        cqWorkplaneNew(c, st, 543, {"plane": {"origin": [1.6551163831041833, -2.866745667976085, 13.031088913245537], "xDir": [0.866025403784439, 0.4999999999999993, 0], "zDir": [0.4472135954999573, -0.7745966692414837, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 544);
        cqPolyline(c, st, 544, 543, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 545);
        cqClose(c, st, 545, 544, {});
        atStep(st, 546);
        cqExtrude(c, st, 546, 545, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 546, "extrude");
        atStep(st, 547);
        cqUnion(c, st, 547, 542, {"toUnion": {"wp": 546}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 547, "union");
        atStep(st, 548);
        cqWorkplaneNew(c, st, 548, {"plane": {"origin": [2.8667456679760828, -1.655116383104187, 13.031088913245537], "xDir": [0.5000000000000004, 0.8660254037844384, 0], "zDir": [0.7745966692414832, -0.4472135954999584, 0.447213595499958]}, "obj": undefined});
        atStep(st, 549);
        cqPolyline(c, st, 549, 548, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 550);
        cqClose(c, st, 550, 549, {});
        atStep(st, 551);
        cqExtrude(c, st, 551, 550, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 551, "extrude");
        atStep(st, 552);
        cqUnion(c, st, 552, 547, {"toUnion": {"wp": 551}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 552, "union");
        atStep(st, 553);
        cqWorkplaneNew(c, st, 553, {"plane": {"origin": [2.8000940756335093, 0.7502829462971699, 13.897114317029974], "xDir": [-0.25881904510252074, 0.9659258262890684, 0], "zDir": [0.8639503235220041, 0.23149479148832816, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 554);
        cqPolyline(c, st, 554, 553, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 555);
        cqClose(c, st, 555, 554, {});
        atStep(st, 556);
        cqExtrude(c, st, 556, 555, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 556, "extrude");
        atStep(st, 557);
        cqUnion(c, st, 557, 552, {"toUnion": {"wp": 556}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 557, "union");
        atStep(st, 558);
        cqWorkplaneNew(c, st, 558, {"plane": {"origin": [2.0498111293363395, 2.0498111293363395, 13.897114317029974], "xDir": [-0.7071067811865476, 0.7071067811865477, 0], "zDir": [0.632455532033676, 0.6324555320336759, 0.447213595499958]}, "obj": undefined});
        atStep(st, 559);
        cqPolyline(c, st, 559, 558, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 560);
        cqClose(c, st, 560, 559, {});
        atStep(st, 561);
        cqExtrude(c, st, 561, 560, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 561, "extrude");
        atStep(st, 562);
        cqUnion(c, st, 562, 557, {"toUnion": {"wp": 561}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 562, "union");
        atStep(st, 563);
        cqWorkplaneNew(c, st, 563, {"plane": {"origin": [0.7502829462971704, 2.8000940756335093, 13.897114317029974], "xDir": [-0.9659258262890682, 0.25881904510252096, 0], "zDir": [0.2314947914883284, 0.8639503235220041, 0.447213595499958]}, "obj": undefined});
        atStep(st, 564);
        cqPolyline(c, st, 564, 563, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 565);
        cqClose(c, st, 565, 564, {});
        atStep(st, 566);
        cqExtrude(c, st, 566, 565, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 566, "extrude");
        atStep(st, 567);
        cqUnion(c, st, 567, 562, {"toUnion": {"wp": 566}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 567, "union");
        atStep(st, 568);
        cqWorkplaneNew(c, st, 568, {"plane": {"origin": [-0.7502829462971695, 2.8000940756335093, 13.897114317029974], "xDir": [-0.9659258262890684, -0.25881904510252063, 0], "zDir": [-0.23149479148832808, 0.8639503235220042, 0.447213595499958]}, "obj": undefined});
        atStep(st, 569);
        cqPolyline(c, st, 569, 568, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 570);
        cqClose(c, st, 570, 569, {});
        atStep(st, 571);
        cqExtrude(c, st, 571, 570, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 571, "extrude");
        atStep(st, 572);
        cqUnion(c, st, 572, 567, {"toUnion": {"wp": 571}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 572, "union");
        atStep(st, 573);
        cqWorkplaneNew(c, st, 573, {"plane": {"origin": [-2.0498111293363395, 2.0498111293363395, 13.897114317029974], "xDir": [-0.7071067811865477, -0.7071067811865476, 0], "zDir": [-0.6324555320336759, 0.632455532033676, 0.447213595499958]}, "obj": undefined});
        atStep(st, 574);
        cqPolyline(c, st, 574, 573, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 575);
        cqClose(c, st, 575, 574, {});
        atStep(st, 576);
        cqExtrude(c, st, 576, 575, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 576, "extrude");
        atStep(st, 577);
        cqUnion(c, st, 577, 572, {"toUnion": {"wp": 576}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 577, "union");
        atStep(st, 578);
        cqWorkplaneNew(c, st, 578, {"plane": {"origin": [-2.8000940756335093, 0.7502829462971706, 13.897114317029974], "xDir": [-0.258819045102521, -0.9659258262890682, -1.3877787807814457e-17], "zDir": [-0.8639503235220041, 0.2314947914883284, 0.447213595499958]}, "obj": undefined});
        atStep(st, 579);
        cqPolyline(c, st, 579, 578, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 580);
        cqClose(c, st, 580, 579, {});
        atStep(st, 581);
        cqExtrude(c, st, 581, 580, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 581, "extrude");
        atStep(st, 582);
        cqUnion(c, st, 582, 577, {"toUnion": {"wp": 581}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 582, "union");
        atStep(st, 583);
        cqWorkplaneNew(c, st, 583, {"plane": {"origin": [-2.8000940756335093, -0.75028294629717, 13.897114317029974], "xDir": [0.2588190451025208, -0.9659258262890684, 1.3877787807814457e-17], "zDir": [-0.8639503235220041, -0.2314947914883282, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 584);
        cqPolyline(c, st, 584, 583, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 585);
        cqClose(c, st, 585, 584, {});
        atStep(st, 586);
        cqExtrude(c, st, 586, 585, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 586, "extrude");
        atStep(st, 587);
        cqUnion(c, st, 587, 582, {"toUnion": {"wp": 586}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 587, "union");
        atStep(st, 588);
        cqWorkplaneNew(c, st, 588, {"plane": {"origin": [-2.04981112933634, -2.0498111293363395, 13.897114317029974], "xDir": [0.7071067811865475, -0.7071067811865478, 0], "zDir": [-0.632455532033676, -0.6324555320336758, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 589);
        cqPolyline(c, st, 589, 588, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 590);
        cqClose(c, st, 590, 589, {});
        atStep(st, 591);
        cqExtrude(c, st, 591, 590, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 591, "extrude");
        atStep(st, 592);
        cqUnion(c, st, 592, 587, {"toUnion": {"wp": 591}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 592, "union");
        atStep(st, 593);
        cqWorkplaneNew(c, st, 593, {"plane": {"origin": [-0.7502829462971721, -2.800094075633509, 13.897114317029974], "xDir": [0.965925826289068, -0.25881904510252157, 0], "zDir": [-0.23149479148832883, -0.8639503235220038, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 594);
        cqPolyline(c, st, 594, 593, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 595);
        cqClose(c, st, 595, 594, {});
        atStep(st, 596);
        cqExtrude(c, st, 596, 595, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 596, "extrude");
        atStep(st, 597);
        cqUnion(c, st, 597, 592, {"toUnion": {"wp": 596}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 597, "union");
        atStep(st, 598);
        cqWorkplaneNew(c, st, 598, {"plane": {"origin": [0.7502829462971685, -2.8000940756335098, 13.897114317029974], "xDir": [0.9659258262890684, 0.2588190451025203, 0], "zDir": [0.23149479148832777, -0.8639503235220042, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 599);
        cqPolyline(c, st, 599, 598, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 600);
        cqClose(c, st, 600, 599, {});
        atStep(st, 601);
        cqExtrude(c, st, 601, 600, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 601, "extrude");
        atStep(st, 602);
        cqUnion(c, st, 602, 597, {"toUnion": {"wp": 601}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 602, "union");
        atStep(st, 603);
        cqWorkplaneNew(c, st, 603, {"plane": {"origin": [2.0498111293363372, -2.049811129336342, 13.897114317029974], "xDir": [0.7071067811865483, 0.7071067811865468, 0], "zDir": [0.6324555320336752, -0.6324555320336765, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 604);
        cqPolyline(c, st, 604, 603, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 605);
        cqClose(c, st, 605, 604, {});
        atStep(st, 606);
        cqExtrude(c, st, 606, 605, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 606, "extrude");
        atStep(st, 607);
        cqUnion(c, st, 607, 602, {"toUnion": {"wp": 606}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 607, "union");
        atStep(st, 608);
        cqWorkplaneNew(c, st, 608, {"plane": {"origin": [2.800094075633509, -0.7502829462971722, 13.897114317029974], "xDir": [0.25881904510252157, 0.965925826289068, -1.387778780781446e-17], "zDir": [0.8639503235220038, -0.23149479148832888, 0.4472135954999579]}, "obj": undefined});
        atStep(st, 609);
        cqPolyline(c, st, 609, 608, {"listOfXYTuple": [[-0.5, -0.28867513459481287], [0.5, -0.28867513459481287], [0, 0.5773502691896257]], "forConstruction": false, "includeCurrent": false});
        atStep(st, 610);
        cqClose(c, st, 610, 609, {});
        atStep(st, 611);
        cqExtrude(c, st, 611, 610, {"until": 0.5600169943749475, "combine": true, "clean": true, "both": true, "taper": undefined});
        stepVol(c, st, 611, "extrude");
        atStep(st, 612);
        cqUnion(c, st, 612, 607, {"toUnion": {"wp": 611}, "clean": true, "glue": false, "tol": undefined});
        stepVol(c, st, 612, "union");
        atStep(st, 613);
        cqCut(c, st, 613, 13, {"toCut": {"wp": 612}, "clean": true, "tol": undefined});
        stepVol(c, st, 613, "cut");
        return mergeMaps(cqMeasure(c, st, {"wp": 613}), {"i": 862});
    } catch (e) {
        return {"i": 862, "status": "error", "at": st[].cur, "err": toString(e), "log": st[].log, "warn": st[].warn};
    }
};
    var out = {};
    { var r = case_862(newContextWithDefaults(), makeId("q")); r.result_q = undefined; out["c862"] = r; }
    return out;
}
