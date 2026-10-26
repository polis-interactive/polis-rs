use embassy_sync::{
    blocking_mutex::raw::RawMutex,
    mutex::{Mutex, MutexGuard},
};
use embedded_hal::i2c::{ErrorType, Operation};
use embedded_hal_async::i2c::I2c;

use crate::{Channel, Error, Xca9548a};

/// One upstream bus and mux selection cache protected by an async mutex.
///
/// Use `NoopRawMutex` for tasks on one executor or `CriticalSectionRawMutex` with
/// a suitable platform critical-section implementation for cross-context access.
/// This feature selects no executor, MCU, timer or critical-section provider.
/// No fairness or bounded acquisition latency is promised.
pub struct SharedMux<M: RawMutex, BUS> {
    inner: Mutex<M, Xca9548a<BUS>>,
}

impl<M: RawMutex, BUS> SharedMux<M, BUS> {
    /// Wrap a mux without I/O or allocation. The mux must exclusively own its bus.
    pub const fn new(mux: Xca9548a<BUS>) -> Self {
        Self {
            inner: Mutex::new(mux),
        }
    }

    /// Create a bus handle; repeated calls for the same channel are supported.
    ///
    /// Devices on the same channel need distinct addresses. Use only one logical
    /// driver per physical device when its commands span multiple transactions.
    pub fn channel(&self, channel: Channel) -> SharedChannel<'_, M, BUS> {
        SharedChannel { mux: self, channel }
    }

    /// Acquire exclusive mux control for recovery, reset or manual selection.
    ///
    /// Call `invalidate_selection` before starting a reset, and hold this guard
    /// until hardware is ready. `bus_mut` invalidates automatically. Do not use any
    /// channel handle while holding this guard: it would wait for the same lock.
    pub async fn lock(&self) -> MutexGuard<'_, M, Xca9548a<BUS>> {
        self.inner.lock().await
    }

    /// Consume the manager, returning the driver without changing hardware.
    pub fn into_inner(self) -> Xca9548a<BUS> {
        self.inner.into_inner()
    }
}

/// Shareable async I²C bus for one channel; contains no device address or buffer.
///
/// The lock spans channel selection (including its STOP) and the full downstream
/// transaction, including repeated STARTs. Success leaves the channel selected.
/// Dropping a pending operation releases the lock and leaves uncertain selection
/// uncached; it does not repair the underlying bus's cancellation state.
pub struct SharedChannel<'a, M: RawMutex, BUS> {
    mux: &'a SharedMux<M, BUS>,
    channel: Channel,
}

impl<M: RawMutex, BUS> Clone for SharedChannel<'_, M, BUS> {
    fn clone(&self) -> Self {
        Self {
            mux: self.mux,
            channel: self.channel,
        }
    }
}

impl<M: RawMutex, BUS: ErrorType> ErrorType for SharedChannel<'_, M, BUS> {
    type Error = Error<BUS::Error>;
}

impl<M: RawMutex, BUS: I2c> I2c for SharedChannel<'_, M, BUS> {
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        self.mux
            .inner
            .lock()
            .await
            .channel_transaction(self.channel, address, operations)
            .await
    }
}
