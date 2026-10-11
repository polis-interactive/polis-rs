# Fixed-layout LED outputs

`led-output` groups physical RGB or RGBW chains into one allocation-free
`no_std` output capability. Lane counts and pixel counts are fixed in the types.
A frame concatenates lanes in export order, preserving each lane's pixel
transmission order. Rendering, brightness scaling and coordinate mapping belong
to the caller.

Physical drivers implement `LedOutput<C>`. The color type defaults to `RGBW<u8>`
for existing callers; RGB drivers use `RGB8`. All lanes in a group share the
same color type, with no implicit color conversion. Use the variadic macro to
combine different driver types and pixel counts:

```rust,ignore
use led_output::led_outputs;

let mut outputs = led_outputs!(lane0, lane1, lane2, lane3);
outputs.write(&pixels).await?;
```

`led_outputs!` accepts one or more expressions and an optional trailing comma.
It evaluates each expression once, left to right, and moves each result into the
group. Grouping adds no allocation or I/O beyond the expressions' own effects.
There is no fixed library lane-count cap; compiler type-size limits and hardware
resources still apply. Groups expose
`write`, `pixel_count` and `lane_count`. Frame ranges follow argument order.

The macro builds nested pairs internally using linear token repetition. Pairs
compose concurrent writes without trait objects. Existing explicit
`LedOutputs::new(...)` calls remain supported; callers using the macro do not
need to name or construct its nested representation.

Incorrect frame lengths are rejected before any I/O. Writes finish after every
lane's transmission and reset interval; unequal lengths can latch at different
times. Concurrent transfer is not synchronized optical presentation. Cancelling
drops all pending chain futures and can display a partial frame. Each physical
driver must establish reset before its next complete write.

An optional fixed-width expansion can later suppress output based on a
board-specific presence input while retaining its frame range and pixel count.
Absence must not shift later lanes or change frame size. No generic hot-plug
mechanism is provided.

The [SK6812 driver](../../drivers/pio/sk6812/README.md) implements this interface. The
LED BSP (in the upstream Slate project) constructs its group from the validated
hardware export. Its current resource allocation supports one to three lanes;
that limit belongs to the BSP, not the generic grouping layer.

## Validation

```sh
cargo test -p led-output
cargo build -p led-output
cargo build -p led-output --target thumbv6m-none-eabi --release
cargo clippy -p led-output --all-targets -- -D warnings
```

Tests cover one through three and eight lanes, unequal lengths, concurrent progress,
completion, invalid frames without I/O, and cancellation. Macro tests cover
evaluation order, hygiene, rejected syntax and a renamed dependency in a `no_std`
consumer. Run `python3 tools/check-led-macro.py` for that consumer on all targets.
Electrical timing and DMA behavior
require hardware tests; the BSP guide owns consuming image builds.

## Source and distribution

See [PROVENANCE.md](PROVENANCE.md) for source hashes, extraction changes and
license status. Product BSPs and bench firmware remain in their original projects.
