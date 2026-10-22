//! Portable simulation/rendering shared verbatim by firmware and host preview.
#[path = "rocket.rs"]
mod asset;

const SIDE: usize = 25;
const CENTER: i32 = 12;
const CLEAR: u8 = 255; // Original asset's white background becomes transparent.
// The supplied sprite needs at most 70 runs per heading; checked during init.
const MAX_SPANS: usize = 96;
// Sine/cosine in Q14, clockwise from the native upward-facing sprite.
const ANGLES: [(i32, i32); 8] = [
    (0, 16384),
    (11585, 11585),
    (16384, 0),
    (11585, -11585),
    (0, -16384),
    (-11585, -11585),
    (-16384, 0),
    (-11585, 11585),
];

#[derive(Clone, Copy)]
pub struct Settings {
    pub width: usize,
    pub height: usize,
    pub scale: usize,
    pub target: usize,
    pub min_speed: f32,
    pub max_speed: f32,
    pub headings: &'static [u8],
    pub background: u8,
    pub margin: f32,
}

#[derive(Clone, Copy)]
struct Bounds {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}
impl Bounds {
    const EMPTY: Self = Self {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };
    fn visible(self, x: f32, y: f32, s: Settings) -> bool {
        x + self.right > 0.0
            && x + self.left < s.width as f32
            && y + self.bottom > 0.0
            && y + self.top < s.height as f32
    }
}

#[derive(Clone, Copy)]
struct Rocket {
    alive: bool,
    born: u64,
    duration: u64,
    heading: usize,
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
}
impl Rocket {
    const EMPTY: Self = Self {
        alive: false,
        born: 0,
        duration: 0,
        heading: 0,
        x: 0.0,
        y: 0.0,
        vx: 0.0,
        vy: 0.0,
    };
    fn position(self, now: u64) -> (f32, f32) {
        let seconds = now.saturating_sub(self.born) as f32 / 1_000_000.0;
        (self.x + self.vx * seconds, self.y + self.vy * seconds)
    }
}

#[derive(Clone, Copy)]
struct Star {
    x: usize,
    y: usize,
    phase: u32,
}
impl Star {
    const EMPTY: Self = Self {
        x: 0,
        y: 0,
        phase: 0,
    };
}

#[derive(Clone, Copy)]
struct Span {
    x: i16,
    y: i16,
    width: u16,
    color: u8,
}
impl Span {
    const EMPTY: Self = Self {
        x: 0,
        y: 0,
        width: 0,
        color: 0,
    };
}

struct Sprite {
    spans: [Span; MAX_SPANS],
    len: usize,
    exhaust: [Span; 10],
}
impl Sprite {
    const EMPTY: Self = Self {
        spans: [Span::EMPTY; MAX_SPANS],
        len: 0,
        exhaust: [Span::EMPTY; 10],
    };

    fn prepare(&mut self, pixels: &[u8; SIDE * SIDE], scale: usize, heading: usize) {
        self.len = 0;
        for y in 0..SIDE {
            let mut x = 0;
            while x < SIDE {
                let start = x;
                let color = pixels[y * SIDE + x];
                x += 1;
                while x < SIDE && pixels[y * SIDE + x] == color {
                    x += 1;
                }
                if color == CLEAR {
                    continue;
                }
                assert!(self.len < MAX_SPANS, "sprite run cache too small");
                self.spans[self.len] = Span {
                    x: ((start as i32 - CENTER) * scale as i32) as i16,
                    y: ((y as i32 - CENTER) * scale as i32) as i16,
                    width: ((x - start) * scale) as u16,
                    color,
                };
                self.len += 1;
            }
        }
        // The three flame lengths share the first 8/9/10 blocks in this order.
        let (sin, cos) = ANGLES[heading];
        let mut i = 0;
        for dy in 0..6 {
            let radius = if dy < 2 { 1 } else { 0 };
            for dx in -radius..=radius {
                let y = 11 + dy;
                self.exhaust[i] = Span {
                    x: (((cos * dx - sin * y + 8192).div_euclid(16384)) * scale as i32) as i16,
                    y: (((sin * dx + cos * y + 8192).div_euclid(16384)) * scale as i32) as i16,
                    width: scale as u16,
                    color: if dy < 2 { 0xfc } else { 0xf0 },
                };
                i += 1;
            }
        }
    }

    fn draw(&self, frame: &mut [u8], s: Settings, x: i32, y: i32, flame: usize) {
        for span in self.exhaust[..8 + flame]
            .iter()
            .chain(&self.spans[..self.len])
        {
            rectangle(
                frame,
                s,
                x + i32::from(span.x),
                y + i32::from(span.y),
                usize::from(span.width),
                span.color,
            );
        }
    }
}

pub struct Scene<const CAP: usize, const STARS: usize> {
    settings: Settings,
    rockets: [Rocket; CAP],
    stars: [Star; STARS],
    sprites: [Sprite; 8],
    bounds: [Bounds; 8],
    rng: u32,
    now: u64,
    next_spawn: u64,
    pub dropped: u32,
}

impl<const CAP: usize, const STARS: usize> Scene<CAP, STARS> {
    pub const fn empty() -> Self {
        Self {
            settings: Settings {
                width: 1,
                height: 1,
                scale: 1,
                target: 0,
                min_speed: 1.0,
                max_speed: 1.0,
                headings: &[0],
                background: 0,
                margin: 1.0,
            },
            rockets: [Rocket::EMPTY; CAP],
            stars: [Star::EMPTY; STARS],
            sprites: [const { Sprite::EMPTY }; 8],
            bounds: [Bounds::EMPTY; 8],
            rng: 1,
            now: 0,
            next_spawn: 0,
            dropped: 0,
        }
    }

    pub fn init(&mut self, settings: Settings, seed: u32) {
        assert!(settings.width > 0 && settings.height > 0 && settings.scale > 0);
        assert!(
            settings.scale <= i16::MAX as usize / SIDE,
            "scaled sprite offsets must fit i16"
        );
        assert!(settings.min_speed.is_finite() && settings.min_speed > 0.0);
        assert!(settings.max_speed.is_finite() && settings.max_speed >= settings.min_speed);
        assert!(settings.margin.is_finite() && settings.margin >= settings.scale as f32 * 7.0);
        assert!(!settings.headings.is_empty() && settings.headings.iter().all(|h| *h < 8));
        assert!(settings.target <= CAP);
        self.settings = settings;
        self.rng = seed.max(1);
        self.now = 0;
        self.next_spawn = 0;
        self.dropped = 0;
        self.rockets.fill(Rocket::EMPTY);
        for i in 0..STARS {
            self.stars[i] = Star {
                x: self.random() as usize % settings.width,
                y: self.random() as usize % settings.height,
                phase: self.random(),
            };
        }
        let mut native = [CLEAR; 11 * 21];
        asset::draw(|x, y, [r, g, b]| {
            native[y as usize * 11 + x as usize] = if [r, g, b] == [255; 3] {
                CLEAR
            } else {
                (r & 0xe0) | ((g >> 3) & 0x1c) | (b >> 6)
            };
        });
        for (h, &(sin, cos)) in ANGLES.iter().enumerate() {
            let (mut left, mut top, mut right, mut bottom) = (SIDE as i32, SIDE as i32, 0, 0);
            let mut pixels = [CLEAR; SIDE * SIDE];
            for y in 0..SIDE {
                for x in 0..SIDE {
                    let dx = x as i32 - CENTER;
                    let dy = y as i32 - CENTER;
                    // Inverse nearest-neighbor rotation avoids holes in diagonal sprites.
                    let sx = ((cos * dx + sin * dy + 8192).div_euclid(16384)) + 5;
                    let sy = ((-sin * dx + cos * dy + 8192).div_euclid(16384)) + 10;
                    if (0..11).contains(&sx) && (0..21).contains(&sy) {
                        let color = native[sy as usize * 11 + sx as usize];
                        pixels[y * SIDE + x] = color;
                        if color != CLEAR {
                            left = left.min(x as i32);
                            top = top.min(y as i32);
                            right = right.max(x as i32 + 1);
                            bottom = bottom.max(y as i32 + 1);
                        }
                    }
                }
            }
            self.sprites[h].prepare(&pixels, settings.scale, h);
            let scale = settings.scale as f32;
            self.bounds[h] = Bounds {
                left: (left - CENTER) as f32 * scale,
                top: (top - CENTER) as f32 * scale,
                right: (right - CENTER) as f32 * scale,
                bottom: (bottom - CENTER) as f32 * scale,
            };
        }
    }

    fn random(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng
    }
    fn unit(&mut self) -> f32 {
        (self.random() >> 8) as f32 / 16_777_216.0
    }

    fn launch(&mut self, born: u64) -> (Rocket, u64) {
        let s = self.settings;
        let heading = s.headings[self.random() as usize % s.headings.len()] as usize;
        let speed = s.min_speed + self.unit() * (s.max_speed - s.min_speed);
        let (sin, cos) = ANGLES[heading];
        let (vx, vy) = (sin as f32 / 16384.0 * speed, -cos as f32 / 16384.0 * speed);
        // Every trajectory crosses a random interior point, so corner misses cannot occur.
        let (x, y) = (self.unit() * s.width as f32, self.unit() * s.height as f32);
        let b = self.bounds[heading];
        let (x0, x1) = interval(x, vx, -b.right, s.width as f32 - b.left);
        let (y0, y1) = interval(y, vy, -b.bottom, s.height as f32 - b.top);
        let enter = x0.max(y0);
        let exit = x1.min(y1);
        let padding = s.margin / speed;
        let start = enter - padding;
        let duration = ((exit - enter + 2.0 * padding) * 1_000_000.0) as u64 + 1;
        let visible_us = ((exit - enter) * 1_000_000.0) as u64;
        (
            Rocket {
                alive: true,
                born,
                duration,
                heading,
                x: x + vx * start,
                y: y + vy * start,
                vx,
                vy,
            },
            visible_us,
        )
    }

    pub fn advance(&mut self, elapsed_us: u64) {
        self.now += elapsed_us;
        for r in &mut self.rockets {
            if r.alive && self.now - r.born >= r.duration {
                r.alive = false;
            }
        }
        if self.settings.target == 0 {
            return;
        }
        while self.next_spawn <= self.now {
            let (r, visible_us) = self.launch(self.next_spawn);
            // Renewal reward: E[visible lifetime] / E[launch interval] = target N.
            // Speed and trajectory vary, so instantaneous population fluctuates.
            self.next_spawn += (visible_us / self.settings.target as u64).max(1);
            if self.now - r.born < r.duration {
                if let Some(slot) = self.rockets.iter_mut().find(|r| !r.alive) {
                    *slot = r;
                } else {
                    self.dropped += 1;
                }
            }
        }
    }

    pub fn visible(&self) -> usize {
        self.rockets
            .iter()
            .filter(|r| {
                if !r.alive {
                    return false;
                }
                let (x, y) = r.position(self.now);
                self.bounds[r.heading].visible(x, y, self.settings)
            })
            .count()
    }

    pub fn render(&self, frame: &mut [u8]) {
        let s = self.settings;
        assert_eq!(frame.len(), s.width * s.height);
        frame.fill(s.background);
        let star_tick = (self.now / 250_000) as u32;
        for star in &self.stars {
            let phase = star_tick.wrapping_add(star.phase) % 16;
            frame[star.y * s.width + star.x] = if phase < 3 { 0xff } else { 0x6e };
        }
        let flame_tick = self.now / 90_000;
        for r in self.rockets.iter().filter(|r| r.alive) {
            let (cx, cy) = r.position(self.now);
            let (cx, cy) = (cx as i32, cy as i32);
            let flame = ((flame_tick + r.born / 1000) % 3) as usize;
            self.sprites[r.heading].draw(frame, s, cx, cy, flame);
        }
    }
}

fn interval(p: f32, v: f32, lo: f32, hi: f32) -> (f32, f32) {
    if v == 0.0 {
        return (f32::NEG_INFINITY, f32::INFINITY);
    }
    let (a, b) = ((lo - p) / v, (hi - p) / v);
    (a.min(b), a.max(b))
}

fn rectangle(frame: &mut [u8], s: Settings, x: i32, y: i32, width: usize, color: u8) {
    let x0 = x.max(0).min(s.width as i32) as usize;
    let y0 = y.max(0).min(s.height as i32) as usize;
    let x1 = (x + width as i32).max(0).min(s.width as i32) as usize;
    let y1 = (y + s.scale as i32).max(0).min(s.height as i32) as usize;
    for y in y0..y1 {
        frame[y * s.width + x0..y * s.width + x1].fill(color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_spans_match_pixel_renderer_at_all_headings_scales_and_edges() {
        let mut native = [CLEAR; 11 * 21];
        asset::draw(|x, y, [r, g, b]| {
            native[y as usize * 11 + x as usize] = (r & 0xe0) | ((g >> 3) & 0x1c) | (b >> 6);
        });
        for scale in [1, 2, 3] {
            let s = Settings {
                scale,
                margin: 10.0 * scale as f32,
                ..super::super::SETTINGS
            };
            let mut scene = super::super::Scene::empty();
            scene.init(s, 42);
            for (h, &(sin, cos)) in ANGLES.iter().enumerate() {
                for (cx, cy) in [
                    (-50, -50),
                    (0, 0),
                    (3, 5),
                    (120, 160),
                    (239, 319),
                    (240, 320),
                    (260, 30),
                    (5, 330),
                ] {
                    for flame in 0..3 {
                        let mut expected = vec![s.background; s.width * s.height];
                        // Original pixel-at-a-time rasterizer, with independent clipping.
                        let mut pixel = |x: i32, y: i32, color| {
                            for dy in 0..scale as i32 {
                                for dx in 0..scale as i32 {
                                    let (px, py) =
                                        (cx + x * scale as i32 + dx, cy + y * scale as i32 + dy);
                                    if px >= 0
                                        && py >= 0
                                        && px < s.width as i32
                                        && py < s.height as i32
                                    {
                                        expected[py as usize * s.width + px as usize] = color;
                                    }
                                }
                            }
                        };
                        for dy in 0..4 + flame as i32 {
                            let radius = if dy < 2 { 1 } else { 0 };
                            for dx in -radius..=radius {
                                let y = 11 + dy;
                                pixel(
                                    (cos * dx - sin * y + 8192).div_euclid(16384),
                                    (sin * dx + cos * y + 8192).div_euclid(16384),
                                    if dy < 2 { 0xfc } else { 0xf0 },
                                );
                            }
                        }
                        for y in -CENTER..=CENTER {
                            for x in -CENTER..=CENTER {
                                let sx = (cos * x + sin * y + 8192).div_euclid(16384) + 5;
                                let sy = (-sin * x + cos * y + 8192).div_euclid(16384) + 10;
                                if (0..11).contains(&sx) && (0..21).contains(&sy) {
                                    let color = native[sy as usize * 11 + sx as usize];
                                    if color != CLEAR {
                                        pixel(x, y, color);
                                    }
                                }
                            }
                        }
                        let mut actual = vec![s.background; s.width * s.height];
                        scene.sprites[h].draw(&mut actual, s, cx, cy, flame);
                        assert!(
                            actual == expected,
                            "heading={h} scale={scale} x={cx} y={cy} flame={flame}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn launches_start_and_finish_offscreen_with_bounded_speeds_and_valid_headings() {
        let mut scene = super::super::Scene::empty();
        scene.init(super::super::SETTINGS, 123);
        for _ in 0..2000 {
            let (r, visible) = scene.launch(0);
            assert!(visible > 0 && visible < r.duration);
            let b = scene.bounds[r.heading];
            assert!(!b.visible(r.x, r.y, scene.settings));
            let (x, y) = r.position(r.duration);
            assert!(!b.visible(x, y, scene.settings));
            let speed2 = r.vx * r.vx + r.vy * r.vy;
            assert!(speed2 >= scene.settings.min_speed.powi(2) * 0.999);
            assert!(speed2 <= scene.settings.max_speed.powi(2) * 1.001);
            assert!(scene.settings.headings.contains(&(r.heading as u8)));
        }
    }

    #[test]
    fn steady_population_matches_target_without_capacity_loss() {
        let mut scene = super::super::Scene::empty();
        scene.init(super::super::SETTINGS, super::super::RANDOM_SEED);
        let mut total = 0;
        for i in 0..36_000 {
            scene.advance(16_667);
            if i >= 3600 {
                total += scene.visible();
            }
        }
        let average = total as f32 / 32_400.0;
        assert!(
            (average - scene.settings.target as f32).abs() < 0.5,
            "average={average}"
        );
        assert_eq!(scene.dropped, 0);
    }

    #[test]
    fn timing_is_independent_of_frame_rate_and_rendering_is_clipped() {
        let (mut a, mut b) = (super::super::Scene::empty(), super::super::Scene::empty());
        a.init(super::super::SETTINGS, 42);
        b.init(super::super::SETTINGS, 42);
        for _ in 0..600 {
            a.advance(100_000);
        }
        for _ in 0..6000 {
            b.advance(10_000);
        }
        // Slot reuse (and thus overlap order) can differ with sampling frequency.
        // Compare both simulations in the same birth order, including all positions.
        a.rockets.sort_unstable_by_key(|r| r.born);
        b.rockets.sort_unstable_by_key(|r| r.born);
        let active_a: Vec<_> = a
            .rockets
            .iter()
            .filter(|r| r.alive)
            .map(|r| (r.born, r.heading, r.position(a.now)))
            .collect();
        let active_b: Vec<_> = b
            .rockets
            .iter()
            .filter(|r| r.alive)
            .map(|r| (r.born, r.heading, r.position(b.now)))
            .collect();
        assert_eq!(active_a, active_b);
        let mut fa = [0; super::super::SCREEN_WIDTH * super::super::SCREEN_HEIGHT];
        let mut fb = fa;
        a.render(&mut fa);
        b.render(&mut fb);
        assert!(
            fa == fb,
            "rendered pixels differ at the same simulation time"
        );
        assert_eq!(a.dropped, 0);
        assert_eq!(b.dropped, 0);
    }

    #[test]
    fn zero_target_is_a_valid_empty_scene() {
        let mut scene = super::super::Scene::empty();
        scene.init(
            Settings {
                target: 0,
                ..super::super::SETTINGS
            },
            42,
        );
        scene.advance(1_000_000);
        assert_eq!(scene.visible(), 0);
    }
}
