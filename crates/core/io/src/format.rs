use std::fmt;

/// Displays a byte count with a `bytes`/`KB`/`MB`/`GB` suffix chosen by magnitude.
///
/// Uses decimal units (1 KB = 1000 bytes), not binary (1 KiB = 1024 bytes).
pub struct BytesFormatter(pub usize);

impl fmt::Display for BytesFormatter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const KB: usize = 1_000;
        const MB: usize = 1_000_000;
        const GB: usize = 1_000_000_000;

        // Choose appropriate unit based on size for best readability
        if self.0 >= GB {
            write!(f, "{:.2} GB", self.0 as f64 / GB as f64)
        } else if self.0 >= MB {
            write!(f, "{:.2} MB", self.0 as f64 / MB as f64)
        } else if self.0 >= KB {
            write!(f, "{:.2} KB", self.0 as f64 / KB as f64)
        } else {
            write!(f, "{} bytes", self.0)
        }
    }
}

pub struct LargeFormatter<T: ToString>(pub T);

impl<T: ToString> fmt::Display for LargeFormatter<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let num_str = self.0.to_string();

        // Handle an optional leading '-' sign separately so comma positions are
        // calculated only over the digit characters.
        let (sign, digits) = if let Some(stripped) = num_str.strip_prefix('-') {
            ("-", stripped)
        } else {
            ("", num_str.as_str())
        };

        if !sign.is_empty() {
            write!(f, "{sign}")?;
        }

        let len = digits.len();
        for (i, ch) in digits.chars().enumerate() {
            if i > 0 && (len - i).is_multiple_of(3) {
                write!(f, ",")?;
            }
            write!(f, "{ch}")?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::BytesFormatter;
    use super::LargeFormatter;

    use rand::RngExt;

    use merc_utilities::random_test;

    #[test]
    fn test_bytes_formatter_units() {
        assert_eq!(format!("{}", BytesFormatter(0)), "0 bytes");
        assert_eq!(format!("{}", BytesFormatter(1)), "1 bytes");
        assert_eq!(format!("{}", BytesFormatter(999)), "999 bytes");
        // Exact unit boundaries.
        assert_eq!(format!("{}", BytesFormatter(1_000)), "1.00 KB");
        assert_eq!(format!("{}", BytesFormatter(1_000_000)), "1.00 MB");
        assert_eq!(format!("{}", BytesFormatter(1_000_000_000)), "1.00 GB");
        // Fractional values within a unit.
        assert_eq!(format!("{}", BytesFormatter(1_500)), "1.50 KB");
        assert_eq!(format!("{}", BytesFormatter(2_500_000_000)), "2.50 GB");
        // Just below the next boundary rounds up to the boundary in the lower unit.
        assert_eq!(format!("{}", BytesFormatter(999_999)), "1000.00 KB");
    }

    /// The chosen unit must always match the magnitude of the value.
    #[test]
    fn test_bytes_formatter_random_unit_selection() {
        random_test(1000, |rng| {
            let value: usize = rng.random_range(0..10_000_000_000);
            let formatted = format!("{}", BytesFormatter(value));

            let expected_suffix = if value >= 1_000_000_000 {
                "GB"
            } else if value >= 1_000_000 {
                "MB"
            } else if value >= 1_000 {
                "KB"
            } else {
                "bytes"
            };
            assert!(
                formatted.ends_with(expected_suffix),
                "{value} formatted as {formatted:?}, expected unit {expected_suffix}"
            );
        });
    }

    /// Inserting comma separators must never corrupt the digits: stripping the
    /// commas back out has to reproduce the plain decimal representation.
    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_large_formatter_random_preserves_value() {
        random_test(1000, |rng| {
            let value: i64 = rng.random();
            let formatted = format!("{}", LargeFormatter(value));
            assert_eq!(formatted.replace(',', ""), value.to_string());
        });
    }

    #[test]
    fn test_large_formatter_small_numbers() {
        assert_eq!(format!("{}", LargeFormatter(0)), "0");
        assert_eq!(format!("{}", LargeFormatter(123)), "123");
    }

    #[test]
    fn test_large_formatter_millions() {
        assert_eq!(format!("{}", LargeFormatter(1234567)), "1,234,567");
        assert_eq!(format!("{}", LargeFormatter(12345678)), "12,345,678");
    }

    #[test]
    fn test_large_formatter_negative() {
        assert_eq!(format!("{}", LargeFormatter(-123456i64)), "-123,456");
        assert_eq!(format!("{}", LargeFormatter(-1234567i64)), "-1,234,567");
        assert_eq!(format!("{}", LargeFormatter(-123456789i64)), "-123,456,789");
    }
}
