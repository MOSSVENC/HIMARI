#!/usr/bin/env python3
"""Build the multi-resolution Windows icon from assets/himari_logo.png.

The PNG is the source of truth: it carries the finished artwork with its rounded
corners and alpha. This script renders each size through librsvg and packs an ICO
whose small entries are 32-bit DIBs, not PNGs.

PNG-compressed entries are only universally reliable at 256x256; the shell and
CreateIconFromResourceEx are most predictable when 16-64 are classic DIBs. The
DIBs are built from the rendered PNG via netpbm's pngtopam, so no image library
is required.

Requirements: rsvg-convert and pngtopam (netpbm).
"""

import base64
import struct
import subprocess
import tempfile
from pathlib import Path

DIB_SIZES = (16, 20, 24, 32, 40, 48, 64)
PNG_SIZES = (128, 256)


def render(png_bytes: bytes, size: int) -> bytes:
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
        return subprocess.check_output(
            ["rsvg-convert", "-w", str(size), "-h", str(size), str(source)]
        )


def decode_png(data: bytes):
    """Minimal RGBA8 PNG decoder (non-interlaced), enough for rsvg output."""
    import zlib

    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit("not a PNG")
    pos = 8
    width = height = None
    idat = bytearray()
    while pos < len(data):
        length = int.from_bytes(data[pos : pos + 4], "big")
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            width = int.from_bytes(body[0:4], "big")
            height = int.from_bytes(body[4:8], "big")
            depth = body[8]
            color = body[9]
            interlace = body[12]
            if depth != 8 or color != 6 or interlace != 0:
                raise SystemExit(
                    f"unsupported PNG: depth={depth} color={color} interlace={interlace}"
                )
        elif kind == b"IDAT":
            idat += body
        elif kind == b"IEND":
            break
    raw = zlib.decompress(bytes(idat))
    stride = width * 4
    out = bytearray(height * stride)
    previous = bytearray(stride)
    offset = 0
    for row in range(height):
        filter_type = raw[offset]
        offset += 1
        line = bytearray(raw[offset : offset + stride])
        offset += stride
        if filter_type == 1:
            for index in range(4, stride):
                line[index] = (line[index] + line[index - 4]) & 0xFF
        elif filter_type == 2:
            for index in range(stride):
                line[index] = (line[index] + previous[index]) & 0xFF
        elif filter_type == 3:
            for index in range(stride):
                left = line[index - 4] if index >= 4 else 0
                line[index] = (line[index] + ((left + previous[index]) >> 1)) & 0xFF
        elif filter_type == 4:
            for index in range(stride):
                left = line[index - 4] if index >= 4 else 0
                up = previous[index]
                up_left = previous[index - 4] if index >= 4 else 0
                estimate = left + up - up_left
                pa, pb, pc = abs(estimate - left), abs(estimate - up), abs(estimate - up_left)
                if pa <= pb and pa <= pc:
                    predictor = left
                elif pb <= pc:
                    predictor = up
                else:
                    predictor = up_left
                line[index] = (line[index] + predictor) & 0xFF
        elif filter_type != 0:
            raise SystemExit(f"unknown PNG filter {filter_type}")
        out[row * stride : (row + 1) * stride] = line
        previous = line
    return width, height, bytes(out)

def to_rgba(png: bytes) -> bytes:
    """Decode a PNG to raw RGBA bytes through netpbm's PAM output."""
    _, _, rgba = decode_png(png)
    return rgba


def dib_entry(png: bytes, size: int) -> bytes:
    rgba = to_rgba(png)
    header = struct.pack(
        "<IiiHHIIiiII",
        40,          # biSize
        size,        # biWidth
        size * 2,    # biHeight: XOR image plus AND mask
        1,           # biPlanes
        32,          # biBitCount
        0,           # biCompression: BI_RGB
        size * size * 4,
        0, 0, 0, 0,
    )
    xor = bytearray(size * size * 4)
    for row in range(size):
        src = row * size * 4
        # DIB rows are bottom-up.
        dst = (size - 1 - row) * size * 4
        for column in range(size):
            r, g, b, a = rgba[src + column * 4 : src + column * 4 + 4]
            # ICO DIBs store straight (non-premultiplied) BGRA.
            xor[dst + column * 4 : dst + column * 4 + 4] = bytes((b, g, r, a))
    mask_stride = ((size + 31) // 32) * 4
    mask = bytearray(mask_stride * size)
    for row in range(size):
        dst_row = size - 1 - row
        for column in range(size):
            alpha = rgba[(row * size + column) * 4 + 3]
            if alpha == 0:
                # A set bit means "transparent" for the AND mask.
                mask[dst_row * mask_stride + column // 8] |= 0x80 >> (column % 8)
    return header + bytes(xor) + bytes(mask)


def main():
    assets = Path(__file__).resolve().parent.parent / "assets"
    source_png = assets / "himari_logo.png"
    png_bytes = source_png.read_bytes()

    entries = []
    for size in DIB_SIZES:
        entries.append((size, dib_entry(render(png_bytes, size), size)))
    for size in PNG_SIZES:
        entries.append((size, render(png_bytes, size)))

    offset = 6 + 16 * len(entries)
    directory = bytearray(struct.pack("<HHH", 0, 1, len(entries)))
    for size, payload in entries:
        directory += struct.pack(
            "<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(payload), offset
        )
        offset += len(payload)
    (assets / "himari_logo.ico").write_bytes(
        bytes(directory) + b"".join(payload for _, payload in entries)
    )
    print(
        f"himari_logo.ico: {len(entries)} entries "
        f"({len(DIB_SIZES)} DIB + {len(PNG_SIZES)} PNG), {offset} bytes"
    )


if __name__ == "__main__":
    main()
