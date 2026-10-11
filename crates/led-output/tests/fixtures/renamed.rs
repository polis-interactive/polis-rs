#![no_std]

// This caller has only the renamed `led-api` dependency. A hardcoded
// `led_output::` path in the macro would fail here.
use led_api::{FrameLengthError, LedOutput, RGB8};

struct Lane<'a, const N: usize>(&'a mut usize);

impl<const N: usize> LedOutput<RGB8> for Lane<'_, N> {
    const PIXEL_COUNT: usize = N;

    async fn write(&mut self, pixels: &[RGB8]) -> Result<(), FrameLengthError> {
        if pixels.len() != N {
            return Err(FrameLengthError {
                expected: N,
                actual: pixels.len(),
            });
        }
        *self.0 += 1;
        Ok(())
    }
}

pub async fn write(left: &mut usize, right: &mut usize) -> Result<(), FrameLengthError> {
    let mut outputs = led_api::led_outputs!(Lane::<2>(left), Lane::<3>(right),);
    outputs.write(&[RGB8::new(0, 0, 0); 5]).await
}
