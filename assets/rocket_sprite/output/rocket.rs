//! Allocation-free decoder for this trusted, generated rocket.bin asset.
//! RGB888 output; white background is opaque. No external dependencies.
pub const WIDTH: u8 = 11;
pub const HEIGHT: u8 = 21;
pub static DATA: &[u8; 40] = include_bytes!("rocket.bin");

/// Calls `pixel(x, y, [r, g, b])` once for each pixel, in raster order.
/// Coordinates are local to the sprite; apply position/scaling in the callback.
pub fn draw(mut pixel: impl FnMut(u8, u8, [u8; 3])) {
    let mut bit = 17 * 8;
    let mut read = |count: usize| -> u16 {
        let mut value = 0;
        for _ in 0..count {
            value = (value << 1) | ((DATA[bit / 8] >> (7 - bit % 8)) & 1) as u16;
            bit += 1;
        }
        value
    };
    let mut y = 0;
    while y < HEIGHT {
        let mut value = read(14);
        let count = if read(1) != 0 { read(4) as u8 + 2 } else { 1 };
        let mut half = [0u8; 6];
        for i in (0..6).rev() {
            half[i] = (value % 5) as u8;
            value /= 5;
        }
        for _ in 0..count {
            for x in 0..WIDTH {
                let index = half[if x < 6 { x } else { WIDTH - 1 - x } as usize];
                let p = 2 + index as usize * 3;
                pixel(x, y, [DATA[p], DATA[p + 1], DATA[p + 2]]);
            }
            y += 1;
        }
    }
}
