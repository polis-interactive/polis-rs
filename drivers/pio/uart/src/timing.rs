/// The clock/baud combination cannot be represented by Embassy's integer divider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// Zero clock/baud or divider outside the PIO range 1..=65536.
    OutOfRange,
    /// Choose a system clock divisible by eight times the requested baud.
    InexactBaud,
}

pub(crate) fn flush_us(sys_hz: u32, baud: u32) -> Result<u64, ConfigError> {
    let bit_clock = baud
        .checked_mul(8)
        .filter(|&v| v != 0)
        .ok_or(ConfigError::OutOfRange)?;
    let divider = sys_hz / bit_clock;
    if !(1..=65536).contains(&divider) {
        return Err(ConfigError::OutOfRange);
    }
    if !sys_hz.is_multiple_of(bit_clock) {
        return Err(ConfigError::InexactBaud);
    }
    // Joined TX FIFO (8 words) + OSR (1 word), ten bits per 8N1 character.
    // Include a 20 us scheduling/timer margin: 110 us at 1 Mbaud.
    Ok(90_000_000_u64.div_ceil(u64::from(baud)) + 20)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_covers_nine_characters_at_each_supported_baud() {
        assert_eq!(flush_us(128_000_000, 1_000_000), Ok(110));
        assert_eq!(flush_us(128_000_000, 500_000), Ok(200));
        assert_eq!(flush_us(147_456_000, 115_200), Ok(802));
        assert_eq!(flush_us(8_000_000, 1_000_000), Ok(110));
        assert!(flush_us(524_288, 1).is_ok());
    }

    #[test]
    fn rejects_silent_baud_rounding_and_invalid_dividers() {
        for hz in [125_000_000, 150_000_000] {
            assert_eq!(flush_us(hz, 1_000_000), Err(ConfigError::InexactBaud));
        }
        for (hz, baud) in [
            (0, 1),
            (128_000_000, 0),
            (128_000_000, u32::MAX),
            (128_000_000, 20_000_000),
            (524_296, 1),
        ] {
            assert_eq!(flush_us(hz, baud), Err(ConfigError::OutOfRange));
        }
    }
}
