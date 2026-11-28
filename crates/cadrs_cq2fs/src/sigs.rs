//! CadQuery 2.8 call signatures (cq.py, occ_impl/shapes.py), for binding a tape entry's Python
//! arguments the way Python does: positionals in order, then keywords by name, then defaults.
//! A call Python itself would reject (unknown keyword, missing argument) is a generator error.

use serde_json::Value;

pub struct Sig {
    /// tape op name
    pub op: &'static str,
    /// FeatureScript runtime function
    pub fs: &'static str,
    /// (parameter, default as JSON; None = required)
    pub params: &'static [(&'static str, Option<&'static str>)],
    /// the last parameter collects extra positionals (`*args`)
    pub varargs: bool,
    /// the op makes or changes solids: log its volume after it
    pub solid: bool,
}

macro_rules! p {
    ($n:literal) => {
        ($n, None)
    };
    ($n:literal = $d:literal) => {
        ($n, Some($d))
    };
}

pub const WORKPLANE: &[Sig] = &[
    Sig {
        op: "workplane",
        fs: "cqWorkplane",
        params: &[
            p!("offset" = "0.0"),
            p!("invert" = "false"),
            p!("centerOption" = "\"ProjectedOrigin\""),
            p!("origin" = "null"),
        ],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "transformed",
        fs: "cqTransformed",
        params: &[p!("rotate" = "[0,0,0]"), p!("offset" = "[0,0,0]")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "center",
        fs: "cqCenter",
        params: &[p!("x"), p!("y")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "moveTo",
        fs: "cqMoveTo",
        params: &[p!("x" = "0"), p!("y" = "0")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "move",
        fs: "cqMove",
        params: &[p!("xDist" = "0"), p!("yDist" = "0")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "pushPoints",
        fs: "cqPushPoints",
        params: &[p!("pntList")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "rarray",
        fs: "cqRarray",
        params: &[
            p!("xSpacing"),
            p!("ySpacing"),
            p!("xCount"),
            p!("yCount"),
            p!("center" = "true"),
        ],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "polarArray",
        fs: "cqPolarArray",
        params: &[
            p!("radius"),
            p!("startAngle"),
            p!("angle"),
            p!("count"),
            p!("fill" = "true"),
            p!("rotate" = "true"),
        ],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "rect",
        fs: "cqRect",
        params: &[
            p!("xLen"),
            p!("yLen"),
            p!("centered" = "true"),
            p!("forConstruction" = "false"),
        ],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "circle",
        fs: "cqCircle",
        params: &[p!("radius"), p!("forConstruction" = "false")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "polygon",
        fs: "cqPolygon",
        params: &[
            p!("nSides"),
            p!("diameter"),
            p!("forConstruction" = "false"),
            p!("circumscribed" = "false"),
        ],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "ellipse",
        fs: "cqEllipse",
        params: &[
            p!("x_radius"),
            p!("y_radius"),
            p!("rotation_angle" = "0.0"),
            p!("forConstruction" = "false"),
        ],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "lineTo",
        fs: "cqLineTo",
        params: &[p!("x"), p!("y"), p!("forConstruction" = "false")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "line",
        fs: "cqLine",
        params: &[p!("xDist"), p!("yDist"), p!("forConstruction" = "false")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "hLine",
        fs: "cqHLine",
        params: &[p!("distance"), p!("forConstruction" = "false")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "vLine",
        fs: "cqVLine",
        params: &[p!("distance"), p!("forConstruction" = "false")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "hLineTo",
        fs: "cqHLineTo",
        params: &[p!("xCoord"), p!("forConstruction" = "false")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "vLineTo",
        fs: "cqVLineTo",
        params: &[p!("yCoord"), p!("forConstruction" = "false")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "polarLine",
        fs: "cqPolarLine",
        params: &[p!("distance"), p!("angle"), p!("forConstruction" = "false")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "polarLineTo",
        fs: "cqPolarLineTo",
        params: &[p!("distance"), p!("angle"), p!("forConstruction" = "false")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "threePointArc",
        fs: "cqThreePointArc",
        params: &[p!("point1"), p!("point2"), p!("forConstruction" = "false")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "polyline",
        fs: "cqPolyline",
        params: &[
            p!("listOfXYTuple"),
            p!("forConstruction" = "false"),
            p!("includeCurrent" = "false"),
        ],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "close",
        fs: "cqClose",
        params: &[],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "wire",
        fs: "cqWire",
        params: &[p!("forConstruction" = "false")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "extrude",
        fs: "cqExtrude",
        params: &[
            p!("until"),
            p!("combine" = "true"),
            p!("clean" = "true"),
            p!("both" = "false"),
            p!("taper" = "null"),
        ],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "cutBlind",
        fs: "cqCutBlind",
        params: &[
            p!("until"),
            p!("clean" = "true"),
            p!("both" = "false"),
            p!("taper" = "null"),
        ],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "cutThruAll",
        fs: "cqCutThruAll",
        params: &[p!("clean" = "true"), p!("taper" = "0")],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "hole",
        fs: "cqHole",
        params: &[p!("diameter"), p!("depth" = "null"), p!("clean" = "true")],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "cboreHole",
        fs: "cqCboreHole",
        params: &[
            p!("diameter"),
            p!("cboreDiameter"),
            p!("cboreDepth"),
            p!("depth" = "null"),
            p!("clean" = "true"),
        ],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "cskHole",
        fs: "cqCskHole",
        params: &[
            p!("diameter"),
            p!("cskDiameter"),
            p!("cskAngle"),
            p!("depth" = "null"),
            p!("clean" = "true"),
        ],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "box",
        fs: "cqBox",
        params: &[
            p!("length"),
            p!("width"),
            p!("height"),
            p!("centered" = "true"),
            p!("combine" = "true"),
            p!("clean" = "true"),
        ],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "sphere",
        fs: "cqSphere",
        params: &[
            p!("radius"),
            p!("direct" = "[0,0,1]"),
            p!("angle1" = "-90"),
            p!("angle2" = "90"),
            p!("angle3" = "360"),
            p!("centered" = "true"),
            p!("combine" = "true"),
            p!("clean" = "true"),
        ],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "cylinder",
        fs: "cqCylinder",
        params: &[
            p!("height"),
            p!("radius"),
            p!("direct" = "[0,0,1]"),
            p!("angle" = "360"),
            p!("centered" = "true"),
            p!("combine" = "true"),
            p!("clean" = "true"),
        ],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "revolve",
        fs: "cqRevolve",
        params: &[
            p!("angleDegrees" = "360.0"),
            p!("axisStart" = "null"),
            p!("axisEnd" = "null"),
            p!("combine" = "true"),
            p!("clean" = "true"),
        ],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "union",
        fs: "cqUnion",
        params: &[
            p!("toUnion" = "null"),
            p!("clean" = "true"),
            p!("glue" = "false"),
            p!("tol" = "null"),
        ],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "cut",
        fs: "cqCut",
        params: &[p!("toCut"), p!("clean" = "true"), p!("tol" = "null")],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "intersect",
        fs: "cqIntersect",
        params: &[p!("toIntersect"), p!("clean" = "true"), p!("tol" = "null")],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "combine",
        fs: "cqCombine",
        params: &[
            p!("clean" = "true"),
            p!("glue" = "false"),
            p!("tol" = "null"),
        ],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "fillet",
        fs: "cqFillet",
        params: &[p!("radius")],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "chamfer",
        fs: "cqChamfer",
        params: &[p!("length"), p!("length2" = "null")],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "shell",
        fs: "cqShell",
        params: &[p!("thickness"), p!("kind" = "\"arc\"")],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "rotate",
        fs: "cqRotate",
        params: &[p!("axisStartPoint"), p!("axisEndPoint"), p!("angleDegrees")],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "translate",
        fs: "cqTranslate",
        params: &[p!("vec")],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "mirror",
        fs: "cqMirror",
        params: &[
            p!("mirrorPlane" = "\"XY\""),
            p!("basePointVector" = "null"),
            p!("union" = "false"),
        ],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "split",
        fs: "cqSplit",
        params: &[p!("keepTop" = "false"), p!("keepBottom" = "false")],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "faces",
        fs: "cqSelect(\"Faces\")",
        params: &[p!("selector" = "null"), p!("tag" = "null")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "edges",
        fs: "cqSelect(\"Edges\")",
        params: &[p!("selector" = "null"), p!("tag" = "null")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "vertices",
        fs: "cqSelect(\"Vertices\")",
        params: &[p!("selector" = "null"), p!("tag" = "null")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "solids",
        fs: "cqSelect(\"Solids\")",
        params: &[p!("selector" = "null"), p!("tag" = "null")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "add",
        fs: "cqAdd",
        params: &[p!("obj")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "tag",
        fs: "cqTag",
        params: &[p!("name")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "end",
        fs: "cqEnd",
        params: &[p!("n" = "1")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "workplaneFromTagged",
        fs: "cqWorkplaneFromTagged",
        params: &[p!("name")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "copyWorkplane",
        fs: "cqCopyWorkplane",
        params: &[p!("obj")],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "toPending",
        fs: "cqToPending",
        params: &[],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "clean",
        fs: "cqClean",
        params: &[],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "first",
        fs: "cqSlice(\"first\")",
        params: &[],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "last",
        fs: "cqSlice(\"last\")",
        params: &[],
        varargs: false,
        solid: false,
    },
    Sig {
        op: "item",
        fs: "cqSlice(\"item\")",
        params: &[p!("i")],
        varargs: false,
        solid: false,
    },
];

pub const SHAPE: &[Sig] = &[
    Sig {
        op: "Solid.makeBox",
        fs: "shSolidMakeBox",
        params: &[
            p!("length"),
            p!("width"),
            p!("height"),
            p!("pnt" = "[0,0,0]"),
            p!("dir" = "[0,0,1]"),
        ],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "Solid.makeCylinder",
        fs: "shSolidMakeCylinder",
        params: &[
            p!("radius"),
            p!("height"),
            p!("pnt" = "[0,0,0]"),
            p!("dir" = "[0,0,1]"),
            p!("angleDegrees" = "360"),
        ],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "Solid.makeCone",
        fs: "shSolidMakeCone",
        params: &[
            p!("radius1"),
            p!("radius2"),
            p!("height"),
            p!("pnt" = "[0,0,0]"),
            p!("dir" = "[0,0,1]"),
            p!("angleDegrees" = "360"),
        ],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "Solid.makeSphere",
        fs: "shSolidMakeSphere",
        params: &[
            p!("radius"),
            p!("pnt" = "[0,0,0]"),
            p!("dir" = "[0,0,1]"),
            p!("angleDegrees1" = "0"),
            p!("angleDegrees2" = "90"),
            p!("angleDegrees3" = "360"),
        ],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "shape.translate",
        fs: "shTranslate",
        params: &[p!("vector")],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "shape.rotate",
        fs: "shRotate",
        params: &[p!("startVector"), p!("endVector"), p!("angleDegrees")],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "shape.mirror",
        fs: "shMirror",
        params: &[
            p!("mirrorPlane" = "\"XY\""),
            p!("basePointVector" = "[0,0,0]"),
        ],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "shape.moved",
        fs: "shMoved",
        params: &[p!("loc")],
        varargs: false,
        solid: true,
    },
    Sig {
        op: "shape.fuse",
        fs: "shBool(\"union\")",
        params: &[p!("toolShapes")],
        varargs: true,
        solid: true,
    },
    Sig {
        op: "shape.cut",
        fs: "shBool(\"cut\")",
        params: &[p!("toolShapes")],
        varargs: true,
        solid: true,
    },
    Sig {
        op: "shape.intersect",
        fs: "shBool(\"intersect\")",
        params: &[p!("toolShapes")],
        varargs: true,
        solid: true,
    },
];

/// Keyword arguments a signature tolerates and ignores (they change nothing Parasolid can show).
pub const IGNORED_KWARGS: &[&str] = &["clean", "glue", "tol"];

pub fn find(op: &str) -> Option<&'static Sig> {
    WORKPLANE.iter().chain(SHAPE.iter()).find(|s| s.op == op)
}

/// Bind positional and keyword arguments to the signature's parameters, as Python does.
pub fn bind(
    sig: &Sig,
    args: &[Value],
    kwargs: &serde_json::Map<String, Value>,
) -> Result<Vec<(String, Value)>, String> {
    let n = sig.params.len();
    let mut out: Vec<Option<Value>> = vec![None; n];
    if sig.varargs {
        // every positional goes into the last parameter, a list (shape.fuse(*toFuse))
        if n != 1 {
            return Err("varargs signature with more than one parameter".into());
        }
        out[0] = Some(Value::Array(args.to_vec()));
    } else {
        if args.len() > n {
            return Err(format!(
                "{}() takes {} positional arguments but {} were given",
                sig.op,
                n,
                args.len()
            ));
        }
        for (i, a) in args.iter().enumerate() {
            out[i] = Some(a.clone());
        }
    }
    for (k, v) in kwargs {
        match sig.params.iter().position(|(p, _)| p == k) {
            Some(i) => {
                if out[i].is_some() {
                    return Err(format!(
                        "{}() got multiple values for argument '{}'",
                        sig.op, k
                    ));
                }
                out[i] = Some(v.clone());
            }
            None => {
                if sig.varargs && IGNORED_KWARGS.contains(&k.as_str()) {
                    continue;
                }
                return Err(format!(
                    "{}() got an unexpected keyword argument '{}'",
                    sig.op, k
                ));
            }
        }
    }
    let mut bound = Vec::with_capacity(n);
    for (i, (name, default)) in sig.params.iter().enumerate() {
        let v = match out[i].take() {
            Some(v) => v,
            None => match default {
                Some(d) => serde_json::from_str(d).expect("default literal"),
                None => {
                    return Err(format!(
                        "{}() missing required argument: '{}'",
                        sig.op, name
                    ));
                }
            },
        };
        bound.push((name.to_string(), v));
    }
    Ok(bound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn binds_like_python() {
        let sig = find("rect").unwrap();
        let mut kw = serde_json::Map::new();
        kw.insert("centered".into(), json!(false));
        let b = bind(sig, &[json!(10), json!(5)], &kw).unwrap();
        assert_eq!(
            b,
            vec![
                ("xLen".into(), json!(10)),
                ("yLen".into(), json!(5)),
                ("centered".into(), json!(false)),
                ("forConstruction".into(), json!(false))
            ]
        );
        assert!(bind(sig, &[json!(1)], &serde_json::Map::new()).is_err());
        let mut bad = serde_json::Map::new();
        bad.insert("center".into(), json!(true));
        assert!(bind(sig, &[json!(1), json!(2)], &bad).is_err());
    }
}
