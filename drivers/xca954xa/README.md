# Async XCA954xA and shared channel buses

`polis-xca954xa` is an allocation-free `no_std` driver package for the XCA954xA
family. Its `Xca9548a` driver supports the PCA9548A and TCA9548A eight-channel
I²C switches. Channel handles implement `embedded_hal_async::i2c::I2c` 1.0, so
existing sensor and display drivers accept them directly. Construction performs
no I/O. No MCU, executor, timer, allocator or reset policy is selected by the library.

The implementation follows TI PCA9548A datasheet SCPS143G, sections 8.6.1–8.6.3:
[address, control register and STOP behavior](https://www.ti.com/lit/ds/symlink/pca9548a.pdf).
The [TCA9548A datasheet, SCPS207H](https://www.ti.com/lit/ds/symlink/tca9548a.pdf)
defines the same address range, eight-bit channel mask and STOP-based activation.
Use the selected part's electrical limits and reset timing; protocol compatibility
does not make their electrical specifications identical.

The family package name does not imply support for every variant. XCA9543A and
XCA9545A need narrower channel/address validation and interrupt-bit handling;
other variants also require model-specific review. They are not supported yet.

The initial support contract covers one PCA9548A/TCA9548A owning one upstream bus. Cascaded
muxes, multiple independently managed muxes on one controller, and other bus
masters are not supported by the cached shared-channel API.

## Shared channels

Enable `features = ["embassy"]` for the optional `embassy-sync` adapter. It does
not select an Embassy executor, MCU or critical-section implementation.

```rust
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embedded_hal_async::i2c::I2c;
use polis_xca954xa::{Address, Channel, Xca9548a, SharedMux};
use polis_ssd1306::{DisplayError, Ssd1306Async};

async fn draw_three<B: I2c>(bus: B) -> Result<(), DisplayError> {
    let mux = SharedMux::<NoopRawMutex, _>::new(
        Xca9548a::new(bus, Address::DEFAULT), // mux address 0x70
    );
    // All three displays have address 0x3C, on separate physical branches.
    let mut screens = [
        Ssd1306Async::new(mux.channel(Channel::Ch0)),
        Ssd1306Async::new(mux.channel(Channel::Ch1)),
        Ssd1306Async::new(mux.channel(Channel::Ch2)),
    ];
    for screen in &mut screens {
        screen.init().await?;
        // Draw into this screen's framebuffer here.
        screen.flush().await?;
    }
    Ok(())
}
```

`channel()` is repeatable, allocation-free and performs no I/O. Handles may also
be cloned. Multiple handles for one channel share the same selection cache and
lock; devices on that channel must have distinct addresses. Keep one logical
driver per physical device when commands span several I²C transactions.

Every handle acquires the manager's async mutex, selects exactly `1 << channel`
if the cached mask differs, performs the downstream transaction, then releases
the mutex. Reads, writes, write-read and arbitrary operation arrays all take the
same path. Selection is a standalone write ending in STOP; downstream repeated
STARTs remain inside the original transaction. The mutex remains held across
both wire transactions. Successful operations leave the selected channel active,
avoiding redundant control writes. An empty operation array performs no I/O.

The lock protects individual I²C transactions, not a complete display frame.
Three separate screens can safely interleave commands because each has its own
controller state. A task does not hold the lock while drawing or sleeping.
There is no guaranteed fairness or maximum acquisition latency.

Use `NoopRawMutex` only for tasks on one executor. Use
`CriticalSectionRawMutex` with an appropriate platform critical-section provider
when sharing across execution contexts; the examples use the RP provider.
The manager must outlive its handles. `StaticCell` supplies this lifetime for
spawned Embassy tasks without allocation.

## Portable exclusive use

With no features, `Xca9548a::channel(&mut self, channel)` returns an exclusively
borrowed `ChannelBus` implementing the same async I²C trait, without Embassy:

```rust
use embedded_hal_async::i2c::I2c;
use polis_xca954xa::{Address, Channel, Error, Xca9548a};

async fn read<B: I2c>(bus: B) -> Result<[u8; 2], Error<B::Error>> {
    let mut mux = Xca9548a::new(bus, Address::from_pins(false, false, false));
    let mut bytes = [0; 2];
    mux.channel(Channel::Ch2).write_read(0x48, &[0], &mut bytes).await?;
    Ok(bytes)
}
```

`Address::new` validates 0x70..=0x77; `Channel::new` validates 0..=7.
Low-level `select`, `select_channels`, `disable_all` and `read_channels` expose
control-register access. Multi-channel masks are explicit low-level operations:
the caller must avoid address collisions. A channel transfer always replaces a
multi-channel mask with one bit. `into_inner` returns the bus without deselecting.
Channel handles reject the mux's own address and reserved/non-seven-bit addresses,
including general call, before bus I/O. They cannot be used to bypass mux control.

## Errors, reset and cancellation

`Error::Select(E)` identifies mux-control failures; `Error::Transfer(E)` identifies
downstream failures. Both preserve the original bus error and its HAL error kind.
`InvalidDeviceAddress(u8)` reports an invalid channel destination. The driver
does not retry, impose deadlines or reset hardware automatically.

Selection starts unknown. Before selection or downstream I/O, the cache becomes
unknown; only successful completion restores it. Failed or cancelled operations
therefore force the next transfer to reselect. Cancellation releases a shared
lock but inherits the physical bus's cancellation behavior: cache invalidation
is not bus recovery, and a failed write may already have affected a device.

The caller owns RESET, power sequencing and bus recovery. Coordinate these with
channel users. After external reset or power loss, invalidate before further use:

```rust,ignore
let mut control = mux.lock().await;
control.invalidate_selection(); // before starting a reset that might be cancelled
// Perform the board-specific RESET/power sequence while holding this guard.
// Recover the controller using control.bus_mut() if needed; it invalidates too.
drop(control); // channel users can now resume and explicitly select again
```

Do not call a channel handle while holding `mux.lock()`; it would wait for the
same lock. Unexpected power loss or an external actor changing mux state cannot
be detected from a cached value. Sole ownership and coordinated invalidation
are required; a bus proxy shared with unrelated users does not establish this
ownership guarantee.

## Three-screen Pico / Pico 2 examples

Both examples use three **128×32 SSD1306** modules at **0x3C**, with PCA9548A/TCA9548A
address pins A2/A1/A0 low (**0x70**). They use the existing `polis-ssd1306` driver
without modification. They do not configure 128×64 panels.

| Connection | Wiring |
| --- | --- |
| Upstream SDA / SCL | Pico GP6 / GP7 → mux SDA / SCL |
| Screen 1 SDA / SCL | Mux SD0 / SC0 |
| Screen 2 SDA / SCL | Mux SD1 / SC1 |
| Screen 3 SDA / SCL | Mux SD2 / SC2 |
| Power and reset | Common ground; compatible 3.3 V modules; mux RESET pulled high |

Use suitable external pull-ups to 3.3 V on the upstream bus and each populated
downstream branch; account for pull-ups already fitted to modules. Examples use
400 kHz and disable the MCU's internal pulls. Display modules must provide
power-on reset; startup waits 100 ms before initialization. Unused channels remain
disabled after the first selection. RESET/power policy belongs in downstream
firmware, not in this reusable driver.

[`examples/embassy.rs`](examples/embassy.rs) keeps all three displays in one task,
initializes them in order, and draws a channel label plus a moving bar on each
screen every 100 ms.

[`examples/embassy_tasks.rs`](examples/embassy_tasks.rs) spawns three instances of
one Embassy task. Each task owns its display and 512-byte framebuffer, running
at a different interval (100, 150 or 200 ms). The shared mux has static storage.
These are independently scheduled cooperative tasks, not OS threads or three
CPU cores. Both examples stop on I/O errors; products should implement recovery.

Build from the repository root; choose one board feature per invocation:

```sh
# Pico / RP2040: one task, then three tasks
cargo build -p polis-xca954xa --example embassy --features example-rp2040 --target thumbv6m-none-eabi --release --locked
cargo build -p polis-xca954xa --example embassy_tasks --features example-rp2040 --target thumbv6m-none-eabi --release --locked

# Pico 2 / RP2350A: one task, then three tasks
cargo build -p polis-xca954xa --example embassy --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
cargo build -p polis-xca954xa --example embassy_tasks --features example-rp235xa --target thumbv8m.main-none-eabihf --release --locked
```

ELFs are under `.target/<target>/release/examples/`. These commands build and link;
they do not flash hardware. Package-local memory maps and linker arguments are
included. Application code supplies pins, power/reset and deployment choices.

## Validation and distribution

```sh
python3 tools/check.py test --package polis-xca954xa
python3 tools/check.py build --platform all --package polis-xca954xa
python3 tools/check.py clippy --platform all --package polis-xca954xa
```

The matrix exercises no-feature and `embassy` libraries independently, and builds
and links both examples on RP2040 and RP2350A. Host tests cover wire transaction
boundaries, cache reuse/invalidation, reserved addresses, errors after hardware
effects, cancellation, competing channel handles, coordinated recovery, and three
real SSD1306 driver instances against a yielding mock bus. The mock does not
establish physical controller cancellation safety or electrical behavior.

Hardware is not qualified: confirm routing, pull-ups, panel startup, reset recovery
and visible independent updates on the actual three-screen setup.

See [PROVENANCE.md](PROVENANCE.md) for origins and release prerequisites and
[CHANGELOG.md](CHANGELOG.md) for the initial API. Original implementation licensed
under MIT; see [LICENSE-MIT](LICENSE-MIT).
