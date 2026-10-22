ROCKET SPRITE — 11 x 21 pixels, five colors, no watermarks

Folder layout:
    rocket_pixel_art.jpg  Original reference, preserved unchanged.
    recreate.py           Python generator with the reconstructed pixel grid.
    output/               Sprite PNG, enlarged preview, binary, and Rust decoder.

The generator writes to output/ regardless of the current working directory.
It rebuilds this rocket's manually reconstructed grid, rather than converting
arbitrary images automatically.

output/rocket.png is the actual small image. output/preview.png is a 20x nearest-neighbor
enlargement for inspection. White background is opaque, matching the reference.
The original black outline and three pink shades are retained; colors are
representative JPEG cell medians, without JPEG noise or watermark overlays.

Regenerate using Python 3 (no packages required):
    python3 assets/rocket_sprite/recreate.py

Embedded asset: rocket.bin is 40 bytes TOTAL:
    2 bytes: width=11, height=21
   15 bytes: five RGB888 palette entries
   23 bytes: compressed pixels
This compares with 693 bytes for raw RGB888 pixels (94% smaller), or 87 bytes
for tightly packed 3-bit palette indices plus a 15-byte RGB888 palette.
The 40-byte count excludes decoder machine code. For just one sprite, decoder
code can outweigh data savings; measure your firmware's linked size.

The custom format uses symmetry, base-5 palette indices, and repeated rows.
Bits are MSB-first. Each record contains a 14-bit base-5 integer for the six
leftmost pixels including the center, then a one-bit repetition flag. If set,
the next four bits specify repeat count minus two; otherwise count is one.
The leftmost pixel is the most significant base-5 digit. Mirror the first five
pixels around the center. Stop at 21 rows; the final seven padding bits are zero.
This is a sprite-specific format, not a standard image format or an untrusted
image parser. Dimensions, five-color palette, and symmetry are fixed here.

Rust integration (no_std compatible, no heap, no framebuffer required):
    mod rocket;
    rocket::draw(|x, y, rgb| {
        // Send rgb to your display at the desired origin plus (x, y).
    });
Copy rocket.rs and rocket.bin together. The callback emits all 231 RGB888 pixels
in raster order, including white. RGB565 displays can convert each RGB888 color:
    ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3)

This redraw preserves the reference's apparent native grid. Smaller dimensions
would discard shape or shading details. No claim of a mathematically minimal
encoding or hardware qualification is made.
