# polis-pio-step-dir

Allocation-free, DMA-fed STEP/DIR execution for Embassy RP 0.10 on RP2040 and
RP2350A. Uses one state machine, **28 instruction words**, one TX DMA channel,
its per-SM IRQ, and independent STEP/DIR GPIOs in 0..=29. STEP is active high.
Normal features select no executor, time driver, motor enable, brake or planner.

This initial implementation has host instruction-model and linked-example
coverage. **Electrical timing, abort latency and count snapshots still require
hardware qualification.** Generated edges are not measured motor position.

## Execution and ownership

`Segment::Pulses` emits a constant-period run; `Delay` extends STEP low; `End`
distinguishes intentional completion from starvation. Cruising and ramps use
the same stream. The caller owns replaceable plans, acceleration, jerk, homing,
mechanical units, phase conversion and brake sequencing.

```rust,ignore
// engine is configured; units are PIO ticks, not microseconds.
let segment = Segment::Pulses {
    direction: Direction::Positive, pulses: 40, period_ticks: 80,
};
engine.commit(&[segment, segment]).await?; // DMA preload; no STEP edges yet
engine.start()?;
// Poll/refill while updating the caller's uncommitted plan.
// When admission permits, submit final deceleration segments and End.
engine.commit(&[Segment::End]).await?;
let report = engine.wait_stopped().await?;
assert_eq!(report.reason, StopReason::Completed);
```

At most **four commands** are outstanding, including active work, uncollected
completions and End. This bounds both four-word FIFOs. Each command is one DMA
word and can cover many pulses; no per-pulse CPU interrupt is needed. No DMA
buffer is retained after commit returns. Batches are validated before admission.
DMA completion establishes delivery, never physical execution.

Accepted commands are **committed**, not replaceable. QueueFull/Horizon reject
the entire batch without changing motion. Poll drains completion records and
frees capacity; the active segment is charged in full until completion. New
velocity targets replace caller-owned uncommitted plans. There is no live FIFO
rewrite; abort is the only way to discard already-committed commands.

`max_committed_ticks` caps outstanding durations, a 17-tick startup guard until
the first completion, and one credit-return tick. This bounds accepted execution,
**not panel-to-motion response time**. Input scanning, bus turns, task scheduling,
refill/DMA latency and admission opportunity are additional. No 1 ms task period
or 2 ms horizon is hardcoded.

At low speed, split long low intervals into bounded Delay commands followed by
a pulse command. Delay durations add exactly; the caller preserves fractional
timing/phase when converting trajectories to ticks. A finite Delay cannot leave
an indefinitely delayed pulse armed after feeder failure. Periods exceeding the
commitment budget are rejected.

## Timing contract

Config supplies tick_hz, STEP high time H, low DIR hold time L, DIR setup minimum
and the commitment budget. tick_hz must divide clk_sys exactly, with integer
divider 1..=65536. No fractional divider or silently rounded duration is used.
Keep clk_sys fixed for the driver's lifetime.

| Quantity | PIO ticks |
| --- | --- |
| STEP high | H, configurable in 2..=17 |
| Pulse period P | H + L + 16 + d, with d in 1..=32767 |
| DIR setup before STEP rising | At least d + 7 |
| Hold after STEP falling before DIR change | At least L, configurable in 1..=16 |
| Pulse command duration | pulses × P |
| Delay command duration | 14 + d, in 15..=32781 |
| Initial low guard | 17 |
| End through terminal IRQ | 10 |

From the previous segment's last rising edge to the incoming segment's first,
the interval is **the incoming period**, plus intervening Delay durations. The
repeat path is padded to match the boundary path: speed/direction changes add
no incidental gap. First edge after start: `17 + P - H - L - 4` ticks. Completion
records follow the final falling edge and hold delay. PIO terminal IRQ, not DMA
completion, establishes end-of-execution.

```rust
use polis_pio_step_dir::Config;
let timing = Config {
    tick_hz: 4_000_000,        // 250 ns ticks at 128 MHz / 32
    step_high_ticks: 4,       // 1 us
    dir_hold_ticks: 4,        // at least 1 us after STEP falling
    dir_setup_ticks: 4,       // at least 1 us
    max_committed_ticks: 8000, // 2 ms accepted horizon
};
assert_eq!(timing.divider(128_000_000), Ok(32));
assert_eq!(timing.min_period_ticks(), 25);
```

These are waveform-fixture settings, not mechanical operating limits. Electrical
reversal is supported at segment boundaries; the planner must decelerate through
zero first when the mechanism requires it.

## Stop, cancellation and counts

- End closes admission and latches Completed with STEP low.
- Empty input latches Starved with STEP low. Late DMA/FIFO writes and IRQ clears
  cannot resume motion. Software fault-observation latency depends on service.
- Abort disables PIO, forces STEP low, snapshots remaining pulses and clears both
  FIFOs. It may truncate the active pulse; it does not decelerate or apply a brake.
- Dropping an admitted commit future first drops Embassy's DMA transfer, which
  synchronously aborts DMA, then stops the engine with reason Cancelled. Dropping
  a pending wait_stopped also aborts. An unpolled future has no effect. Dropping
  the driver leaves STEP low and clears pending work.
- Stopped engines require reset, preload, and start. Reset discards the previous
  report/counters. There is no automatic restart or replay.

Poll returns **completed-segment checkpoints**, not live position. On abort the
driver disables the SM, drains completion records, saves PC, reads remaining Y,
and accounts for the partial segment. Y decrements on the same instruction that
raises STEP. DIR low counts negative, high positive. Totals explicitly wrap
modulo 2^64. `pulse_may_be_truncated` conservatively marks aborts near the high
phase: the edge is counted, but external-driver acceptance is uncertain.

There is **no exact Hall-edge snapshot** in this version. Poll omits active and
undrained segments; count uncertainty is bounded by their total pulse counts,
and checkpoint age includes caller latency. Abort captures when PIO is disabled,
not at an earlier Hall edge. Hardware capture needs a separate explicit resource
design before phase correction. Even intact edges are not encoder feedback.

Smooth stop is a planner-generated ramp ending in End. Brake sequencing and
motor enable remain product-owned; pulse stopping does not prove mechanical stop.

## Validation and waveform fixture

No MCU features exposes portable types/tests. Select exactly one of rp2040 or
rp235xa for hardware. Example features additionally select the image runtime.

```sh
cargo test -p polis-pio-step-dir --locked
python3 tools/check.py build --platform all --package polis-pio-step-dir
python3 tools/check.py clippy --platform all --package polis-pio-step-dir
```

The example uses PIO0 SM0, DMA_CH0, STEP GP20, DIR GP21 and EN_N GP22 held high.
Disconnect motor power and attach a logic analyzer. It streams periods, finishes,
reverses in a new run, aborts a partial segment and cancels a pending wait. It is
not a motor/brake controller.

```sh
cargo build -p polis-pio-step-dir --example embassy --features example-rp2040 --target thumbv6m-none-eabi
probe-rs run --chip RP2040 .target/thumbv6m-none-eabi/debug/examples/embassy

cargo build -p polis-pio-step-dir --example embassy --features example-rp235xa --target thumbv8m.main-none-eabihf
probe-rs run --chip RP2350 .target/thumbv8m.main-none-eabihf/debug/examples/embassy
```

Host tests interpret the actual assembled words: edge spacing, high/hold timing,
boundaries, delays, zero/one/long segments, starvation/late input, FIFO credit,
counter wrap and stop accounting at every modeled cycle. The model does not
validate silicon PC/delay visibility, DMA abort ordering, register/pad propagation
or external-driver acceptance. Measure these on both chips, including cancellation
at both STEP levels, no post-abort edges, and downstream UART/task load.

Dropping the driver does not explicitly reclaim Embassy instruction allocations
through Common::free_instr; plan allocation for the driver lifetime.

Registry name availability could not be confirmed during initial work: the
crates.io API returned HTTP 403. Recheck before publication; no name is reserved.
