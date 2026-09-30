"""Builds `assets/fonts/cadrs-symbols.ttf`: our own drawings of the three hole-callout symbols
Inter lacks (P3.6, PS15.10), so callouts such as `Ø 5.3 mm THRU | ⌴Ø 9.75 mm ↧ 5 mm` render:

- U+2334 ⌴ counterbore (an open-topped box),
- U+2335 ⌵ countersink (a V),
- U+21A7 ↧ depth (an arrow down onto a bar).

Run from the repository root: `python3 tools/make_symbols_font.py` (needs fontTools).
"""

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

UPM = 1000


def polygon(pen, pts):
    pen.moveTo(pts[0])
    for p in pts[1:]:
        pen.lineTo(p)
    pen.closePath()


def glyph(contours):
    pen = TTGlyphPen(None)
    for c in contours:
        polygon(pen, c)
    return pen.glyph()


# TrueType fills clockwise contours.
counterbore = glyph([[(140, 700), (140, 0), (860, 0), (860, 700), (745, 700), (745, 115), (255, 115), (255, 700)]])
countersink = glyph([[(110, 720), (440, 0), (560, 0), (890, 720), (765, 720), (500, 150), (235, 720)]])
depth = glyph([
    # the bar
    [(200, 0), (200, 100), (800, 100), (800, 0)],
    # the stem
    [(440, 330), (440, 780), (560, 780), (560, 330)],
    # the head
    [(250, 400), (500, 150), (750, 400)],
])
empty = TTGlyphPen(None).glyph()

order = [".notdef", "space", "counterbore", "countersink", "depth"]
fb = FontBuilder(UPM, isTTF=True)
fb.setupGlyphOrder(order)
fb.setupCharacterMap({0x20: "space", 0x2334: "counterbore", 0x2335: "countersink", 0x21A7: "depth"})
fb.setupGlyf({".notdef": empty, "space": empty, "counterbore": counterbore, "countersink": countersink, "depth": depth})
fb.setupHorizontalMetrics({n: (1000 if n in ("counterbore", "countersink", "depth") else 280, 0) for n in order})
fb.setupHorizontalHeader(ascent=950, descent=-250)
fb.setupNameTable({"familyName": "cadrs Symbols", "styleName": "Regular"})
fb.setupOS2(sTypoAscender=950, sTypoDescender=-250, usWinAscent=950, usWinDescent=250)
fb.setupPost()
fb.save("assets/fonts/cadrs-symbols.ttf")
