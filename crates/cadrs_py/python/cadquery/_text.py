"""Text (`Workplane.text`, `Compound.makeText`): the font found as OCCT's font manager finds it
(fontconfig's match for the name, "Arial" a metric-compatible sans on Linux), its glyphs made
natively."""
import functools
import os
import subprocess


@functools.lru_cache(maxsize=None)
def font_file(font="Arial", fontPath=None, kind="regular"):
    if fontPath and os.path.isfile(fontPath):
        return fontPath
    style = {"bold": ":bold", "italic": ":italic"}.get(kind, "")
    for name in (font, "Arial", "Liberation Sans", "DejaVu Sans", "FreeSans", "Noto Sans"):
        try:
            p = subprocess.run(["fc-match", "-f", "%{file}", f"{name}{style}"], capture_output=True, text=True, timeout=10)
            path = p.stdout.strip()
            if path and os.path.isfile(path) and path.lower().endswith((".ttf", ".otf")):
                return path
        except (OSError, subprocess.SubprocessError):
            pass
    raise ValueError(f"no font found for {font!r}")


def make_text(txt, fontsize, distance, font, fontPath, kind, halign, valign, plane):
    from . import _cadrs
    return _cadrs.text(font_file(font, fontPath, kind), txt, fontsize, distance, halign, valign, plane)


def compound_make_text(text, size, height, font="Arial", fontPath=None, kind="regular", halign="center", valign="center", position=None):
    from . import Plane
    return make_text(text, size, height, font, fontPath, kind, halign, valign, position if position is not None else Plane.named("XY"))
