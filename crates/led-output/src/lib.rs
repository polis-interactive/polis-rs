#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![allow(async_fn_in_trait)]
//! Fixed-layout typed pixel output, composing physical chains without allocation.

pub use smart_leds::{RGB8, RGBW};

/// Group one or more lane expressions into a concurrent [`LedOutputs`].
///
/// Lanes may have different types and pixel counts, but must implement
/// [`LedOutput`] for the same color type. Each expression is evaluated exactly
/// once, from left to right, and its result is moved into the group. An optional
/// trailing comma is accepted. Grouping adds no driver I/O or allocation beyond
/// any effects of evaluating the supplied expressions.
/// The nested grouping type is an implementation detail; let inference name it.
///
/// ```
/// use polis_led_output::{FrameLengthError, LedOutput, RGB8, led_outputs};
/// struct Lane<const N: usize>;
/// impl<const N: usize> LedOutput<RGB8> for Lane<N> {
///     const PIXEL_COUNT: usize = N;
///     async fn write(&mut self, pixels: &[RGB8]) -> Result<(), FrameLengthError> {
///         assert_eq!(pixels.len(), N);
///         Ok(())
///     }
/// }
/// let outputs = led_outputs!(Lane::<2>, Lane::<3>, Lane::<4>,);
/// assert_eq!(outputs.lane_count(), 3);
/// assert_eq!(outputs.pixel_count(), 9);
/// ```
///
/// Empty groups and separators other than commas are rejected:
///
/// ```compile_fail
/// let outputs = polis_led_output::led_outputs!();
/// ```
///
/// ```compile_fail
/// let outputs = polis_led_output::led_outputs!(lane0; lane1);
/// ```
///
/// Each argument must be an output driver:
///
/// ```compile_fail,E0277
/// let outputs = polis_led_output::led_outputs!(42_u8);
/// ```
#[macro_export]
macro_rules! led_outputs {
    () => {
        compile_error!("led_outputs! requires at least one lane expression")
    };
    ($first:expr $(, $rest:expr)* $(,)?) => {{
        // Shadowing changes the inferred type at each step. Repetition keeps
        // expansion linear without recursive token matching or repeated input.
        let lanes = $first;
        $(let lanes = (lanes, $rest);)*
        $crate::LedOutputs::new(lanes)
    }};
    ($($invalid:tt)*) => {
        compile_error!("led_outputs! expects comma-separated lane expressions")
    };
}

/// A frame must exactly fill the output's compile-time pixel capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameLengthError {
    /// Required number of pixels.
    pub expected: usize,
    /// Supplied number of pixels.
    pub actual: usize,
}

impl core::fmt::Display for FrameLengthError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "expected {} LED pixels, received {}",
            self.expected, self.actual
        )
    }
}

impl core::error::Error for FrameLengthError {}

/// One physical chain or a statically composed group of chains.
///
/// Implementors reject incorrect lengths before I/O. Successful completion
/// includes transmission and latch/reset time. Cancelling can display a partial
/// frame; implementations must reestablish reset before the next complete write.
/// This interface reports length errors, not electrically undetectable LED faults.
pub trait LedOutput<C = RGBW<u8>> {
    /// Fixed total number of pixels, including any reserved optional lane.
    const PIXEL_COUNT: usize;
    /// Number of physical data outputs in the fixed layout.
    const LANE_COUNT: usize = 1;

    /// Write a complete frame in chain order and then pixel transmission order.
    async fn write(&mut self, pixels: &[C]) -> Result<(), FrameLengthError>;
}

fn check_length(expected: usize, actual: usize) -> Result<(), FrameLengthError> {
    if expected == actual {
        Ok(())
    } else {
        Err(FrameLengthError { expected, actual })
    }
}

// Nested pairs support any compile-time lane count and heterogeneous drivers.
// Neither async trait objects nor a heap are needed.
impl<C, A: LedOutput<C>, B: LedOutput<C>> LedOutput<C> for (A, B) {
    const PIXEL_COUNT: usize = A::PIXEL_COUNT + B::PIXEL_COUNT;
    const LANE_COUNT: usize = A::LANE_COUNT + B::LANE_COUNT;

    async fn write(&mut self, pixels: &[C]) -> Result<(), FrameLengthError> {
        check_length(Self::PIXEL_COUNT, pixels.len())?;
        let (left, right) = pixels.split_at(A::PIXEL_COUNT);
        // Await both outputs even if one reports an error; never abandon a
        // sibling's in-flight transfer merely because another finished first.
        let (a, b) = embassy_futures::join::join(self.0.write(left), self.1.write(right)).await;
        a.and(b)
    }
}

/// Board-facing output capability with a fixed frame layout.
pub struct LedOutputs<L, C = RGBW<u8>> {
    lanes: L,
    color: core::marker::PhantomData<fn() -> C>,
}

impl<L: LedOutput<C>, C> LedOutputs<L, C> {
    /// Group already-configured drivers without I/O or allocation.
    /// A single driver, a pair, or nested pairs all work. Prefer [`led_outputs!`]
    /// to group multiple drivers without writing the nested representation.
    pub const fn new(lanes: L) -> Self {
        Self {
            lanes,
            color: core::marker::PhantomData,
        }
    }

    /// Total number of pixels in every complete frame.
    pub const fn pixel_count(&self) -> usize {
        L::PIXEL_COUNT
    }

    /// Number of physical data outputs in this build.
    pub const fn lane_count(&self) -> usize {
        L::LANE_COUNT
    }

    /// Write all chains concurrently and wait for all drain/reset intervals.
    /// Unequal-length chains may latch at different times; this is not an
    /// optical synchronization barrier. No color/brightness conversion is done.
    pub async fn write(&mut self, pixels: &[C]) -> Result<(), FrameLengthError> {
        check_length(L::PIXEL_COUNT, pixels.len())?;
        self.lanes.write(pixels).await
    }
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
