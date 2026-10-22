#!/usr/bin/env python3
"""Rebuild the reference's native pixel grid; Python standard library only."""
from pathlib import Path
import struct
import zlib

OUT = Path(__file__).resolve().parent / "output"
WIDTH, HEIGHT = 11, 21
PALETTE = [(255, 255, 255), (0, 0, 0), (243, 183, 183),
           (226, 152, 153), (255, 204, 203)]
SYMBOLS = '.#psl'  # white, outline, pink, shadow, light pink
# Left half including center; reflect without duplicating the center pixel.
# Reconstructed cells intentionally omit photographic/JPEG/watermark artifacts.
RUNS = [('....##', 1), ('...#ps', 1), ('..#ppp', 2),
        ('.#pppl', 1), ('.#ppll', 1), ('.#plll', 1),
        ('..#lll', 9), ('.#llll', 1), ('#lllll', 2),
        ('.#lll#', 1), ('..###.', 1)]


def pack():
    bits = ''
    rows = []
    for half, count in RUNS:
        digits = [SYMBOLS.index(c) for c in half]
        value = 0
        for digit in digits:
            value = value * 5 + digit
        bits += f'{value:014b}' + ('1' if count > 1 else '0')
        if count > 1:
            bits += f'{count - 2:04b}'
        rows.extend([digits + digits[-2::-1]] * count)
    bits += '0' * (-len(bits) % 8)
    payload = bytes(int(bits[i:i+8], 2) for i in range(0, len(bits), 8))
    return bytes([WIDTH, HEIGHT]) + bytes(c for rgb in PALETTE for c in rgb) + payload, rows


def unpack(data):
    width, height = data[:2]
    assert width == 11 and height == 21
    bits = ''.join(f'{byte:08b}' for byte in data[17:])
    offset = 0

    def read(n):
        nonlocal offset
        value = int(bits[offset:offset+n], 2)
        offset += n
        return value

    rows = []
    while len(rows) < height:
        value = read(14)
        count = read(4) + 2 if read(1) else 1
        half = [0] * 6
        for i in range(5, -1, -1):
            half[i] = value % 5
            value //= 5
        assert value == 0
        rows.extend([half + half[-2::-1]] * count)
    assert len(rows) == height
    assert set(bits[offset:]) <= {'0'}
    return rows


def png(rows, scale=1):
    """Indexed PNG, four bits per pixel, only the five needed palette entries."""
    def chunk(kind, data):
        return (struct.pack('>I', len(data)) + kind + data +
                struct.pack('>I', zlib.crc32(kind + data)))

    scanlines = bytearray()
    for row in rows:
        expanded = [pixel for pixel in row for _ in range(scale)]
        if len(expanded) % 2:
            expanded.append(0)
        packed = bytes((expanded[i] << 4) | expanded[i+1]
                       for i in range(0, len(expanded), 2))
        scanlines.extend((b'\0' + packed) * scale)
    return (b'\x89PNG\r\n\x1a\n' +
            chunk(b'IHDR', struct.pack('>IIBBBBB', WIDTH*scale, HEIGHT*scale, 4, 3, 0, 0, 0)) +
            chunk(b'PLTE', bytes(c for rgb in PALETTE for c in rgb)) +
            chunk(b'IDAT', zlib.compress(bytes(scanlines), 9)) +
            chunk(b'IEND', b''))


def main():
    data, rows = pack()
    assert len(rows) == HEIGHT and all(len(row) == WIDTH for row in rows)
    assert unpack(data) == rows
    assert all(row == row[::-1] for row in rows)
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / 'rocket.bin').write_bytes(data)
    (OUT / 'rocket.png').write_bytes(png(rows))
    (OUT / 'preview.png').write_bytes(png(rows, 20))
    for name in ['rocket.bin', 'rocket.png', 'preview.png']:
        print(f'{name}: {(OUT / name).stat().st_size} bytes')
    print(f'Round-trip passed: {WIDTH} x {HEIGHT}, five colors, {len(data)-17} compressed pixel bytes')


if __name__ == '__main__':
    main()
