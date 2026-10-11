#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

use embedded_hal::digital::OutputPin;
use embedded_hal_async::{digital::Wait, spi::SpiDevice};

pub use embassy_net_wiznet::{Device, InitError, State};

/// Background packet transfer and link monitoring; firmware must run `run()`.
pub type Runner<'d, SPI, INT, RST> =
    embassy_net_wiznet::Runner<'d, embassy_net_wiznet::chip::W5500, SPI, INT, RST>;

/// W5500 startup over an owned SPI device, interrupt input and reset output.
///
/// Independent of board pins and MCU HAL. Chip initialization and packet I/O
/// delegate to Embassy's W5500 implementation; firmware owns network policy.
pub struct W5500<SPI, INT, RST> {
    spi: SPI,
    interrupt: INT,
    reset: RST,
}

impl<SPI, INT, RST> W5500<SPI, INT, RST> {
    /// Take ownership without accessing the bus or changing GPIO levels.
    ///
    /// BSP supplies configured SPI/CS and active-low interrupt/reset resources.
    /// Hold reset low beforehand if the chip must stay reset until `start`.
    /// An `ExclusiveDevice` or another async `SpiDevice` adapter is suitable.
    pub const fn new(spi: SPI, interrupt: INT, reset: RST) -> Self {
        Self {
            spi,
            interrupt,
            reset,
        }
    }
}

impl<SPI: SpiDevice, INT: Wait, RST: OutputPin> W5500<SPI, INT, RST> {
    /// Reset and initialize the W5500 in raw Ethernet mode.
    ///
    /// Firmware supplies a valid, unique unicast MAC address and packet storage.
    /// Pass the returned device to the network stack and run the runner in a
    /// task. Use static storage for spawned tasks. Requires an Embassy time
    /// driver; reset takes at least 101 ms. SPI/chip-version failures are returned
    /// unchanged from Embassy; its driver ignores reset GPIO errors.
    ///
    /// # Panics
    /// Panics if either packet queue has zero slots.
    pub async fn start<'d, const N_RX: usize, const N_TX: usize>(
        self,
        state: &'d mut State<N_RX, N_TX>,
        mac_address: [u8; 6],
    ) -> Result<(Device<'d>, Runner<'d, SPI, INT, RST>), InitError<SPI::Error>> {
        assert!(
            N_RX > 0 && N_TX > 0,
            "Ethernet requires RX and TX packet slots"
        );
        embassy_net_wiznet::new(mac_address, state, self.spi, self.interrupt, self.reset).await
    }
}
