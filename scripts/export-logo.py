#!/usr/bin/env python3
"""Build the multi-resolution Windows icon from assets/himari_logo.png.

The PNG is the source of truth: it carries the finished artwork with its rounded
corners and alpha. This script embeds it once, renders each ICO size through
librsvg so the downscaling is properly filtered, and packs the result. It never
rewrites the PNG, so repeated runs cannot degrade the artwork.

Requires librsvg's rsvg-convert.
"""

import base64
import struct
import subprocess
import tempfile
from pathlib import Path

SIZES = (16, 20, 24, 32, 40, 48, 64, 128, 256)


def render_sizes(png_bytes: bytes, sizes):
    wrapper = (
        '<svg xmlns="http://www.w3.org/2000/svg" '
        'xmlns:xlink="http://www.w3.org/1999/xlink" width="512" height="512" '
        'viewBox="0 0 512 512">'
        '<defs><clipPath id="t"><rect width="512" height="512" rx="112" ry="112"/></clipPath></defs>'
        '<g clip-path="url(#t)">'
        f'<image x="0" y="0" width="512" height="512" xlink:href="data:image/png;base64,'
        f'{base64.b64encode(png_bytes).decode()}"/>'
        '</g></svg>'
    )
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "wrapper.svg"
        source.write_text(wrapper)
        for size in sizes:
            yield size, subprocess.check_output(
                ["rsvg-convert", "-w", str(size), "-h", str(size), str(source)]
            )


def main():
    assets = Path(__file__).resolve().parent.parent / "assets"
    source_png = assets / "himari_logo.png"
    images = list(render_sizes(source_png.read_bytes(), SIZES))
    directory = bytearray(struct.pack("<HHH", 0, 1, len(images)))
    offset = 6 + 16 * len(images)
    for size, png in images:
        directory += struct.pack(
            "<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(png), offset
        )
        offset += len(png)
    (assets / "himari_logo.ico").write_bytes(
        bytes(directory) + b"".join(png for _, png in images)
    )
    print(f"himari_logo.ico: {len(images)} sizes, {offset} bytes")


if __name__ == "__main__":
    main()
