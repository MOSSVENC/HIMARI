#!/usr/bin/env python3
"""Export the SVG portrait to PNG and a multi-resolution Windows icon.

Requires librsvg's rsvg-convert; uses the Python standard library for ICO output.
"""

from pathlib import Path
import struct
import subprocess


def main():
    assets = Path(__file__).resolve().parent.parent / "assets"
    source = assets / "ram_control_center_logo.svg"
    sizes = (16, 20, 24, 32, 40, 48, 64, 128, 256)
    images = []
    for size in (*sizes, 512):
        png = subprocess.check_output(
            ["rsvg-convert", "-w", str(size), "-h", str(size), str(source)]
        )
        if size == 512:
            (assets / "ram_control_center_logo.png").write_bytes(png)
        else:
            images.append((size, png))
    directory = bytearray(struct.pack("<HHH", 0, 1, len(images)))
    offset = 6 + 16 * len(images)
    for size, png in images:
        directory += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(png), offset)
        offset += len(png)
    (assets / "ram_control_center_logo.ico").write_bytes(
        directory + b"".join(png for _, png in images)
    )


if __name__ == "__main__":
    main()
