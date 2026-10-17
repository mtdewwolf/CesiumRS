"""Stitch CesiumJS's bundled Natural Earth II level-2 TMS tiles into one
2048x1024 equirectangular image: row 0 is the north edge, column 0 is -180°.

    python3 scripts/stitch_natural_earth.py <tile-dir> assets/natural-earth-ii-2048.jpg

<tile-dir> holds {x}_{y}.jpg for x in 0..8, y in 0..4, from
cesium@1.139.0/Build/Cesium/Assets/Textures/NaturalEarthII/2/{x}/{y}.jpg.
Natural Earth imagery is public domain.
"""

import sys

from PIL import Image

TILE = 256


def main(tile_dir: str, out: str) -> None:
    image = Image.new("RGB", (8 * TILE, 4 * TILE))
    for x in range(8):
        for y in range(4):
            tile = Image.open(f"{tile_dir}/{x}_{y}.jpg").convert("RGB")
            # TMS rows count up from the south, image rows count down from the north.
            image.paste(tile, (x * TILE, (3 - y) * TILE))
    image.save(out, quality=78, optimize=True, progressive=True)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
