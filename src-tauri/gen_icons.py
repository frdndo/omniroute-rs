#!/usr/bin/env python3
"""Generate placeholder Tauri icons (32/128/256 + RGBA square).

Real branding icons should be generated from a 1024x1024 source PNG with:
    npm run tauri icon <source.png>   (or: cargo tauri icon <source.png>)
This script only produces valid minimal PNGs so the scaffold builds.
"""
import struct, zlib, os

def png(size: int, path: str) -> None:
    # simple solid-color square with a "O" letterform-ish pattern
    rows = bytearray()
    for y in range(size):
        row = bytearray([0])  # filter: None
        for x in range(size):
            r, g, b = 24, 60, 120  # dark blue
            # rough ring: border 8% + diagonal accent
            edge = min(x, y, size - 1 - x, size - 1 - y)
            if edge < size * 0.04:
                r, g, b = 70, 140, 220
            elif abs(x - y) < size * 0.02:
                r, g, b = 40, 90, 170
            row += bytes((r, g, b, 255))
        rows += row
    def chunk(tag: bytes, data: bytes) -> bytes:
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    png_bytes = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(bytes(rows), 9))
        + chunk(b"IEND", b"")
    )
    with open(path, "wb") as f:
        f.write(png_bytes)
    print(f"  {path} ({size}x{size}, {len(png_bytes)//1024}KB)")

os.makedirs("icons", exist_ok=True)
png(32, "icons/32x32.png")
png(128, "icons/128x128.png")
png(256, "icons/128x128@2x.png")
png(256, "icons/icon.png")

# .icns and .ico are required by tauri.conf.json on macOS/windows bundles;
# they are produced by `cargo tauri icon` — note in README until then.
open("icons/.placeholder", "w").write("real icons: npm run tauri icon <1024px.png>\n")
print("done (icns/ico generated on macOS via `cargo tauri icon`)")
