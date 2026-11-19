//! Pico/Pico 2 boot counter with a linker-reserved settings region.
//! Running this example writes the final four flash sectors; use a test board.
#![no_std]
#![no_main]

use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_rp::{bind_interrupts, dma, flash::Flash, peripherals};
use panic_probe as _;
use polis_settings::{AlignedBuffer, CodecError, Load, Record, Settings};

#[cfg(feature = "example-rp2040")]
const FLASH_SIZE: usize = 2 * 1024 * 1024;
#[cfg(feature = "example-rp235xa")]
const FLASH_SIZE: usize = 4 * 1024 * 1024;

bind_interrupts!(struct Irqs {
    DMA_IRQ_0 => dma::InterruptHandler<peripherals::DMA_CH0>;
});

struct BootCount(u32);
impl Record for BootCount {
    const KEY: u16 = 1;
    const VERSION: u16 = 1;
    fn encode(&self, output: &mut [u8]) -> Result<usize, CodecError> {
        output
            .get_mut(..4)
            .ok_or(CodecError::BufferTooSmall)?
            .copy_from_slice(&self.0.to_le_bytes());
        Ok(4)
    }
    fn decode(payload: &[u8]) -> Result<Self, CodecError> {
        let bytes = payload
            .try_into()
            .map_err(|_| CodecError::InvalidEncoding)?;
        Ok(Self(u32::from_le_bytes(bytes)))
    }
}

unsafe extern "C" {
    static __settings_start: u8;
    static __settings_end: u8;
}

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let flash = Flash::<_, _, FLASH_SIZE>::new(p.FLASH, p.DMA_CH0, Irqs);
    // Addresses come from the same linker symbols that bound the firmware image.
    // Take addresses only; these symbols are not Rust objects to dereference.
    let start = core::ptr::addr_of!(__settings_start) as u32 - 0x1000_0000;
    let end = core::ptr::addr_of!(__settings_end) as u32 - 0x1000_0000;
    assert_eq!(end as usize, FLASH_SIZE);
    assert_eq!(end - start, 16 * 1024);
    let mut scratch = AlignedBuffer([0; 4096]);
    let mut encoded = [0; 7];
    let mut settings = Settings::new(flash, start..end, &mut scratch.0, &mut encoded).unwrap();
    let count = match settings.load::<BootCount>().await {
        Ok(Load::Present(count)) => Some(count.0),
        Ok(Load::Missing) => Some(0), // Example policy; may also mean lost/corrupt data.
        Ok(Load::Unusable(_)) => {
            defmt::error!("Unsupported or malformed boot count; preserving record");
            None
        }
        Err(_) => {
            defmt::error!("Settings read failed; preserving flash");
            None
        }
    };
    if let Some(count) = count {
        let snapshot = BootCount(count.saturating_add(1));
        match settings.save(&snapshot).await {
            Ok(()) => defmt::info!("Saved boot count: {=u32}", snapshot.0),
            Err(_) => defmt::error!("Boot count save failed; snapshot remains unsaved"),
        }
    }
    core::future::pending::<()>().await;
}
