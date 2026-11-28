"""A stand-in for the parts of OCP (OCCT's Python binding) CadQuery programs reach for, on
cadrs's shapes. Only names the real OCP 7.9 has are given, so programs that fall back when an
import fails behave as they do there."""


def __getattr__(name):
    # (`import OCP; OCP.Standard.Standard_Failure`: the submodules as attributes, as OCP's.)
    import importlib
    try:
        return importlib.import_module("OCP." + name)
    except ImportError:
        raise AttributeError(name) from None
