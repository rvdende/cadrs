features.extrude = function(id)
{
    "Feature Name": "Extrude 1",
    var sketch1 = newSketch(context, id + "sketch1", {});
    skPolyline(sketch, "polyline1", {});
    extrude(context, id + "extrude1", {"operationType": NewBodyOperationType.NEW, "depth": 10});
    features.inner = function(id)
    {
        "Feature Name": "Inner 1",
        chamfer(context, id + "chamfer1", {"operationType": NewBodyOperationType.NEW});
    };
};
features.fillet = function(id)
{
    "Feature Name": "Fillet 1",
    fillet(context, id + "fillet1", {"operationType": NewBodyOperationType.NEW});
};
