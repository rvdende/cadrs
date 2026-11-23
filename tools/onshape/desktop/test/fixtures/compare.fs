features.extrude = function(id)
{
    "Feature Name": "Extrude 1",
    extrude(context, id + "extrude1", {"operationType": NewBodyOperationType.NEW, "depth": 10});
};
features.fillet = function(id)
{
    "Feature Name": "Fillet 1",
    fillet(context, id + "fillet1", {"operationType": NewBodyOperationType.NEW});
};
