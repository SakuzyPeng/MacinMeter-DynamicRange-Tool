"""Regenerate the offline TTC fixture with fontTools. All glyphs are rectangles."""

from pathlib import Path

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTCollection


def face(family, chars, width, weight=400, mono=False):
    builder = FontBuilder(1000, isTTF=True)
    cmap = {ord(c): f"u{ord(c):04X}" for c in chars}
    names = [".notdef", *cmap.values()]
    pen = TTGlyphPen(None)
    pen.moveTo((40, 0))
    pen.lineTo((40, 600))
    pen.lineTo((width - 40, 600))
    pen.lineTo((width - 40, 0))
    pen.closePath()
    rectangle = pen.glyph()
    builder.setupGlyphOrder(names)
    builder.setupCharacterMap(cmap)
    builder.setupGlyf(dict.fromkeys(names, rectangle))
    builder.setupHorizontalMetrics({name: (width, 40) for name in names})
    builder.setupHorizontalHeader(ascent=800, descent=-200)
    style = "Semibold" if weight == 600 else "Regular"
    builder.setupNameTable({
        "familyName": family, "styleName": style,
        "psName": f"{family.replace(' ', '')}-{style}", "fullName": f"{family} {style}",
    })
    builder.setupOS2(usWeightClass=weight, sTypoAscender=800, sTypoDescender=-200,
                     usWinAscent=800, usWinDescent=200)
    builder.setupPost(isFixedPitch=int(mono))
    builder.setupMaxp()
    builder.font.recalcTimestamp = False
    builder.font["head"].created = builder.font["head"].modified = 2082844800
    return builder.font


if __name__ == "__main__":
    shared = " ADa1原顺序欅坂あセゾン한국"
    collection = TTCollection()
    collection.fonts = [
        face("Fixture Sans", shared, 500),
        face("Fixture Japanese", shared + "ㇰ", 900),
        face("Fixture Serif", " AD原", 500, weight=600),
        face("Fixture Mono", " 原顺序ㇱ", 1000, mono=True),
    ]
    collection.save(Path(__file__).with_name("fallback.ttc"))
