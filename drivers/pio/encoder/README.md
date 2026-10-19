# polis-pio-encoder

Allocation-free, `no_std` cumulative quadrature position counters for
**Embassy RP 0.10.0**, RP2040 and RP2350A. No default MCU, executor, clock,
time driver, DMA, product pins, buttons, or transport policy in the library.
Hardware qualification is **unverified**. Linked examples and instruction
simulation are evidence about software, not electrical qualification.

## Electrical contract and direction

Pass **B on the lower-numbered GPIO, A on the next GPIO**, in that order:
`PioEncoder::new(common, sm, pin_b_low, pin_a_high, program, config)`.
For example `(B, A) = (GP1, GP2)`. Low/high in the parameter names describes
GPIO numbers, not input voltages. Nonconsecutive, reversed, and GPIO >29 pairs
are rejected. Each pair has an independent state machine and pin mapping.

Phases are written as **AB**, encoded `(A << 1) | B`. Positive raw travel is
`00 -> 10 -> 11 -> 01 -> 00` (A leads B); reverse travel is negative.
Choose `Direction::BLeadsA` in `Tracker` to invert. Clockwise depends on the
encoder mounting/wiring; verify it on the actual device. Pull-ups are the
default for contacts connected to ground; configurable pulls, enabled Schmitt
inputs, and enabled input synchronizers are part of initialization.
Pushbuttons are separate GPIO inputs and are outside this driver.

The decoder uses all sixteen previous/current phase pairs. Adjacent transitions
add +/-1; a repeated phase adds zero. An observed two-bit jump adds zero and
adopts the new phase, without guessing a direction. Adjacent bounce and a
mid-cycle reversal cancel net travel when every phase is sampled. There is no
blind debounce interval. Noise that resembles a whole legal cycle is counted;
electrical conditioning and hardware qualification remain necessary.

The initial PIO observation sets the previous phase and count zero. It does not
interpret a nonzero initial phase as movement. Inputs must have settled before
startup; travel before the first observation is outside the baseline. Loading
and starting three state machines establishes three independent baselines,
not a simultaneous sampling instant.

## Net travel versus mechanical detents

The deliberately smaller API reports **positions**, not detent events:

| Resolution | Position calculation |
| --- | --- |
| `Transition` | Signed net electrical travel from baseline |
| `HalfStep` | Travel / 2, rounded toward zero |
| `FullStep` | Travel / 4, rounded toward zero |

One complete quadrature cycle is four transitions. Some mechanisms have a
detent every two transitions, others every four; consult their specification.
These names describe units of travel, not a configurable electrical latch phase.
At an arbitrary startup phase, the first FullStep unit requires four net
transitions; a nearby physical detent may therefore report zero. FullStep
travel `0,1,2,3,4,3,0,-3,-4` reports `0,0,0,0,1,0,0,0,-1`.
Bounce across a reporting boundary can change the reported position transiently.

Phase-anchored, hysteretic detent events need additional history, including
partial traversal and reversal order. A delayed cumulative snapshot cannot
recover that history. We intentionally do not pretend it can: the fixed-origin
net-travel rule gives the same final position whether software consumes every
edge or one late snapshot. Do not accumulate rounded deltas; retain one Tracker
and use its absolute `Reading.position` / `Reading.transitions`.
Invalid jumps may leave a fractional travel remainder and make true mechanical
position uncertain; the driver does not automatically erase or repair it.

## FIFO, service bounds, and recovery

The hardware keeps a 32-bit count in scratch Y. Every loop tries to push a copy
to RX with **PUSH NOBLOCK**. When the eight-word FIFO fills, old queued snapshots
stay queued and newer snapshots are discarded; **counting does not stall**.
No CPU service or DMA is needed to keep that modulo counter running.

`snapshot().await` reads the FIFO level N, discards exactly N words (N <= 8),
then awaits one newly queued snapshot: at most nine FIFO reads. It does not
drain-until-empty, disable the SM, reset its registers, or inject instructions
while counting. Cancellation is safe for the cumulative count; another call
discards the remaining backlog. The PIO interrupt wakes a pending RX wait only;
there is no per-edge CPU interrupt.

A newly queued snapshot can have been captured just before the call. Once
space is available the next push occurs within ten PIO cycles while running.
CPU scheduling, interrupt latency, and fractional clocks add latency; a future
resumed late can return an older count. Calls are sequentially sampled, not
atomic or timestamped across encoders. Poll again for a more recent observation.

Raw `Position(u32)` wraps modulo 2^32. `wrapping_delta_since` interprets the
modular difference as `i32`. `Tracker` extends this into signed `i64` and applies
direction/resolution, provided net displacement between accepted snapshots is
**strictly less than 2^31 transitions in magnitude**. Exactly half-range is
rejected as `AmbiguousDelta`; other multi-wrap ambiguity is undetectable.
For a conservative maximum observed transition rate R, keep service interval
T strictly below `2^31 / R` seconds. At 100,000 transitions/s this is about
21,474 seconds. Use a much shorter application-owned interval in practice.
`i64` overflow returns `PositionOverflow`, without changing tracker state.

Retain the encoder and tracker across UART/Pi disconnections; publish complete
position snapshots on reconnect. A communication outage does not need a reset.
If the service bound was violated, wraps cannot be recovered: explicitly create
a new Tracker from a fresh snapshot and announce the new baseline downstream.
Stopping/restarting the PIO, power loss, or resetting the MCU also breaks continuity.
No absolute mechanical position is recovered from power loss.

## Sampling qualification and resource budget

| Resource | One encoder | Three encoders on one block |
| --- | --- | --- |
| Instructions | 27 at addresses 0..26, loaded once | Same 27; five slots remain |
| State machines | One | Three; fourth remains available |
| GPIO | Two adjacent inputs, B then A | Six, pairwise adjacency only |
| FIFO / DMA | RX joined to eight words; no TX / no DMA | Three independent RX FIFOs / no DMA |
| IRQ | Embassy PIO IRQ0 RX wakeup; no PIO IRQ flag instructions | One shared IRQ0 binding |

The program uses PIO v0 instructions on both chips. Computed jumps require
origin zero, so `PioEncoderProgram::new` returns a load error if those addresses
are occupied. An unrelated program can use the remaining five words and fourth
SM if compatible. The example assigns PIO0/SM0..2; callers choose resources.

At system frequency F and divider D, the slowest sampling loop is ten SM cycles.
The nominal ceiling is F/(10D) transitions/s, **not a qualified mechanical rate**.
Every intermediate synchronized AB phase must persist until an IN samples it.
Allow margin for fractional-divider jitter, input synchronization, phase dwell
asymmetry, contact bounce, and electrical settling. `Config::max_sample_interval_sys_ticks`
returns `ceil(10 * D) + 1` ticks, excluding input synchronization latency.
The example's integer D=125 gives a nominal worst sample interval of 10 us at
125 MHz or about 8.33 us at 150 MHz. These are example clock assumptions, not
clock requirements imposed on the BSP. Recalculate when changing clk_sys; gating
PIO/its clock stops counting. Default D=1 costs more switching power.

An entire cycle between samples can look identical to no movement; a skipped
phase can look like a legal edge in the wrong direction. There is **no exact
hardware loss detector**, invalid-jump counter, or automatic recovery of missed
motion. The portable `Decoder::invalid_transitions()` counts observed two-bit
jumps only; that diagnostic is **not available from PIO snapshots**.
PIO RXSTALL can indicate discarded snapshots under NOBLOCK, not missed encoder
motion; this library does not expose it as an encoder error. Qualify minimum
phase dwell and bounce behavior on each board/encoder before relying on counts.

## Public API and downstream use

No features: `Phase`, `Decoder`, `Position`, `Direction`, `Resolution`,
`Tracker`, `Reading`, `TrackError`. Hardware features additionally expose
`Config`, `PioEncoderProgram`, and `PioEncoder` with async `snapshot()`.
No allocation, implicit task spawning, singleton assignment, or macro DSL.

```toml
# Local integration; use a reviewed commit revision once these changes are committed.
polis-pio-encoder = { path = "../polis-rs/drivers/pio/encoder", features = ["rp235xa"] }
# Pico uses features = ["rp2040"]. Do not enable both chips.
```

```rust,ignore
use polis_pio_encoder::{Config, Direction, PioEncoder, PioEncoderProgram,
    Position, Resolution, Tracker};

// pio was created with the application's PIO0 IRQ0 binding.
let program = PioEncoderProgram::new(&mut pio.common).unwrap();
let mut encoder = PioEncoder::new(
    &mut pio.common, pio.sm0, p.PIN_1, p.PIN_2, &program, Config::default(),
);
let mut state = Tracker::new(Position(0), Resolution::FullStep, Direction::ALeadsB);

// In the application's periodic state collector, independently of UART readiness:
let current = state.update(encoder.snapshot().await)?;
// Store current.position in the application's complete snapshot.
// Keep encoder/state alive; reconnecting the Pi does not change this baseline.
```

## Linked Pico / Pico 2 example and checks

`examples/embassy.rs` loads one program and runs three counters. Connect each
encoder common terminal to ground; connect its (B,A) terminals to (GP1,GP2),
(GP4,GP5), (GP7,GP8). Internal pull-ups are enabled. Buttons are unused.
RTT prints three net FullStep positions every 100 ms; rotation should change
only the corresponding position, and reversing should undo net travel. Startup
at any settled phase should print zero until movement is observed.

```sh
cargo test --locked -p polis-pio-encoder
python3 tools/check.py build --platform all --package polis-pio-encoder
python3 tools/check.py clippy --platform all --package polis-pio-encoder
cargo fmt --all -- --check
cargo package --locked --allow-dirty -p polis-pio-encoder
```

Direct **linked** example builds (no flashing):

```sh
cargo build --locked --release -p polis-pio-encoder --example embassy --features example-rp2040 --target thumbv6m-none-eabi
cargo build --locked --release -p polis-pio-encoder --example embassy --features example-rp235xa --target thumbv8m.main-none-eabihf
```

Example memory maps, boot metadata, linker arguments, executor and panic/logging
dependencies are package-local and opt-in. Normal library builds select none.
Workspace membership and package metadata automatically include the package in
`tools/check.py` and the existing CI matrix; no custom global cfg is used.

## Validation evidence and provenance

`src/position/tests.rs` checks each startup phase, both directions, bounce,
reversals, invalid jumps, all reporting units, inversion, modular wrap, overflow,
and delayed consumption. `src/program/tests.rs` decodes the **actual assembled
instruction words** used by the HAL and interprets branches, modulo scratch
arithmetic, shifts, wrap addresses, and bounded nonblocking FIFO behavior.
It compares 16,384 seven-phase traces against the independent portable decoder,
exercises counter boundaries, random instruction-time pin changes, the ten-cycle
bound, and 100,000 unconsumed transitions with a full FIFO. This validates the
algorithm rather than only a second copy of the transition table.

The interpreter follows PIO v0 semantics (RP2040 datasheet sections 3.4.2,
3.4.4–3.4.7), including decrement-at-zero, zero-extended OUT ISR, and clearing
ISR on a discarded PUSH. HAL configuration explicitly selects left IN, right
OUT, no automatic FIFO transfers, and RX joining. It is not a silicon emulator;
it cannot qualify pads, metastability, interrupts, actual clock frequency or
linker/runtime execution. Real hardware validation remains outstanding.

The tested HAL registry dependency is `embassy-rp 0.10.0`, source revision
`84444a19eb57d978e3c09fcdc8e60cdd7278eb03`. Embassy's rotary encoder waits for
one falling edge and pushes its direction sample through a blocking FIFO;
this package does not use that program. Our counter adapts Raspberry Pi's
BSD-3-Clause quadrature example with arbitrary-phase initialization and an
Embassy interface. See [PROVENANCE.md](PROVENANCE.md) for immutable source links,
hashes, licenses, and changes, and [VALIDATION.md](VALIDATION.md) for check results.
