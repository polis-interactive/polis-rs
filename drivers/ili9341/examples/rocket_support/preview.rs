//! Native preview uses the exact firmware scene and top-of-file settings.
use super::*;
use std::{fs, io::Write, path::PathBuf};

pub fn run() {
    let output = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "/tmp/polis-rockets".into()),
    );
    fs::create_dir_all(&output).unwrap();
    let mut scene = Scene::empty();
    scene.init(SETTINGS, RANDOM_SEED);
    for _ in 0..WARMUP_SECONDS * 20 {
        scene.advance(50_000);
    }
    let mut frame = vec![0; SCREEN_WIDTH * SCREEN_HEIGHT];
    let (mut total, mut min, mut max, mut images) = (0usize, usize::MAX, 0, 0);
    for i in 0..7200 {
        scene.advance(16_667);
        let visible = scene.visible();
        total += visible;
        min = min.min(visible);
        max = max.max(visible);
        // Six seconds at 15 fps after the statistics run settles.
        if i >= 6840 && i % 4 == 0 {
            scene.render(&mut frame);
            let path = output.join(format!("frame-{images:03}.ppm"));
            let mut f = std::io::BufWriter::new(fs::File::create(path).unwrap());
            write!(f, "P6\n{SCREEN_WIDTH} {SCREEN_HEIGHT}\n255\n").unwrap();
            for &p in &frame {
                let (r, g, b) = (p >> 5, (p >> 2) & 7, p & 3);
                f.write_all(&[
                    (r << 5) | (r << 2) | (r >> 1),
                    (g << 5) | (g << 2) | (g >> 1),
                    b * 85,
                ])
                .unwrap();
            }
            images += 1;
        }
    }
    println!(
        "target={N}; visible avg={:.2}, min={min}, max={max}; capacity_drops={}; preview_frames={images}",
        total as f64 / 7200.0,
        scene.dropped
    );
    // Also reference firmware-only tuning here so native builds remain warning-free.
    println!(
        "firmware FPS cap={TARGET_FPS}, bus clock limit={BUS_MAX_CLOCK_HZ}, HUD={SHOW_HUD}, staging rows={STAGING_ROWS}"
    );
    println!("{}", output.display());
}
