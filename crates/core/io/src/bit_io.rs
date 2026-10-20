use std::io;
use std::io::Read;
use std::io::Write;

/// Returns a mask of the lowest `bits` bits, for `bits` at most 64.
fn low_bits(bits: u32) -> u64 {
    if bits == 64 { u64::MAX } else { (1 << bits) - 1 }
}

/// Reads bits, most significant first, from an underlying byte reader.
///
/// The bits are kept in a word that is refilled eight bytes at a time, so that most reads do not
/// access the underlying reader at all. As a consequence it may read up to eight bytes beyond the
/// bits that were requested.
pub struct BitReader<R: Read> {
    reader: R,

    /// The bits that have not been read yet, in the lowest `num_bits` bits.
    buffer: u64,
    num_bits: u32,
}

impl<R: Read> BitReader<R> {
    /// Wraps `reader` for reading bits.
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            buffer: 0,
            num_bits: 0,
        }
    }

    /// Reads `number_of_bits` bits into the low bits of a u64.
    ///
    /// # Errors
    ///
    /// Returns an error if `number_of_bits` exceeds 64, the underlying reader fails, or the stream
    /// ends before all bits were read.
    #[inline]
    pub fn read_bits(&mut self, number_of_bits: u32) -> io::Result<u64> {
        if number_of_bits > 64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "number_of_bits must be <= 64",
            ));
        }

        if number_of_bits > self.num_bits {
            return self.read_bits_refill(number_of_bits);
        }

        // All bits are in the buffer, which is the common case.
        self.num_bits -= number_of_bits;
        Ok(self.buffer.checked_shr(self.num_bits).unwrap_or(0) & low_bits(number_of_bits))
    }

    /// Reads bits that are not all in the buffer: the remaining ones, followed by the first bits
    /// of the next (at most) eight bytes.
    #[cold]
    fn read_bits_refill(&mut self, number_of_bits: u32) -> io::Result<u64> {
        let high_bits = self.num_bits;
        let high = self.buffer & low_bits(high_bits);

        // Read until the requested bits are available, keeping any further bytes that a read
        // returns. Reading more would block, or fail, on input that is not needed yet.
        let mut bytes = [0u8; 8];
        let mut filled = 0;
        while (filled as u32) * 8 < number_of_bits - high_bits {
            match self.reader.read(&mut bytes[filled..]) {
                Ok(0) => break,
                Ok(read) => filled += read,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }

        let low_bits_needed = number_of_bits - high_bits;
        let available = filled as u32 * 8;
        if low_bits_needed > available {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "failed to fill whole buffer",
            ));
        }

        // The bytes are read most significant first, and at least one byte was read above.
        self.buffer = u64::from_be_bytes(bytes) >> (64 - available);
        self.num_bits = available - low_bits_needed;
        let low = (self.buffer >> self.num_bits) & low_bits(low_bits_needed);

        if low_bits_needed == 64 {
            Ok(low)
        } else {
            Ok((high << low_bits_needed) | low)
        }
    }
}

/// Writes bits, most significant first, to an underlying byte writer.
///
/// The bits are collected in a word that is written eight bytes at a time, so nothing reaches
/// the underlying writer before eight bytes are complete or [`BitWriter::flush`] is called.
pub struct BitWriter<W: Write> {
    writer: W,

    /// The bits that have not been written yet, in the lowest `num_bits` bits.
    pending: u128,
    num_bits: u32,
}

impl<W: Write> BitWriter<W> {
    /// Wraps `writer` for writing bits.
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            pending: 0,
            num_bits: 0,
        }
    }

    /// Writes the low `number_of_bits` bits of `value`.
    ///
    /// # Errors
    ///
    /// Returns an error if `number_of_bits` exceeds 64, `value` does not fit in it, or the
    /// underlying writer fails.
    #[inline]
    pub fn write_bits(&mut self, value: u64, number_of_bits: u32) -> io::Result<()> {
        if number_of_bits > 64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "number_of_bits must be <= 64",
            ));
        }
        if value & !low_bits(number_of_bits) != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "excessive value for bits written",
            ));
        }

        // At most 63 pending bits and 64 new ones fit in the 128 bits of `pending`.
        self.pending = (self.pending << number_of_bits) | u128::from(value);
        self.num_bits += number_of_bits;

        if self.num_bits >= 64 {
            self.num_bits -= 64;
            let word = (self.pending >> self.num_bits) as u64;
            self.pending &= (1 << self.num_bits) - 1;
            self.writer.write_all(&word.to_be_bytes())?;
        }
        Ok(())
    }

    /// Writes the pending bits as whole bytes, padded with zero bits, and flushes the underlying
    /// writer.
    pub fn flush(&mut self) -> io::Result<()> {
        let bytes = self.num_bits.div_ceil(8);
        let padded = (self.pending << (bytes * 8 - self.num_bits)) as u64;
        self.writer.write_all(&padded.to_be_bytes()[8 - bytes as usize..])?;
        self.pending = 0;
        self.num_bits = 0;

        self.writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use rand::RngExt;

    use merc_utilities::random_test;

    use super::BitReader;
    use super::BitWriter;

    /// Packs the given bits, most significant first, into bytes padded with zero bits.
    fn reference_encoding(fields: &[(u64, u32)]) -> Vec<u8> {
        let bits: Vec<bool> = fields
            .iter()
            .flat_map(|&(value, number_of_bits)| (0..number_of_bits).rev().map(move |i| (value >> i) & 1 == 1))
            .collect();

        bits.chunks(8)
            .map(|byte| {
                byte.iter()
                    .chain(std::iter::repeat(&false))
                    .take(8)
                    .fold(0u8, |acc, &bit| (acc << 1) | u8::from(bit))
            })
            .collect()
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_bits() {
        random_test(100, |rng| {
            let fields: Vec<(u64, u32)> = (0..200)
                .map(|_| {
                    let number_of_bits = rng.random_range(0..=64);
                    let value = if number_of_bits == 0 {
                        0
                    } else {
                        rng.random::<u64>() >> (64 - number_of_bits)
                    };
                    (value, number_of_bits)
                })
                .collect();

            let mut bytes = Vec::new();
            let mut writer = BitWriter::new(&mut bytes);
            for &(value, number_of_bits) in &fields {
                writer.write_bits(value, number_of_bits).unwrap();
            }
            writer.flush().unwrap();
            assert_eq!(bytes, reference_encoding(&fields));

            let mut reader = BitReader::new(&bytes[..]);
            for &(value, number_of_bits) in &fields {
                assert_eq!(reader.read_bits(number_of_bits).unwrap(), value);
            }
        });
    }

    #[test]
    fn test_padding_and_end_of_stream() {
        let mut bytes = Vec::new();
        let mut writer = BitWriter::new(&mut bytes);
        writer.write_bits(0b10110, 5).unwrap();
        writer.flush().unwrap();
        assert_eq!(bytes, vec![0b1011_0000], "The last byte is padded with zero bits");

        let mut reader = BitReader::new(&bytes[..]);
        assert_eq!(reader.read_bits(5).unwrap(), 0b10110);
        assert_eq!(reader.read_bits(3).unwrap(), 0, "The padding reads as zero bits");
        assert!(reader.read_bits(1).is_err(), "Reading past the end must fail");
    }

    #[test]
    fn test_read_stops_once_bits_are_available() {
        /// Returns the given bytes, after which reading would block.
        struct Partial<'a>(&'a [u8]);

        impl std::io::Read for Partial<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.0.is_empty() {
                    return Err(std::io::ErrorKind::WouldBlock.into());
                }
                let read = self.0.len().min(buf.len());
                buf[..read].copy_from_slice(&self.0[..read]);
                self.0 = &self.0[read..];
                Ok(read)
            }
        }

        let mut reader = BitReader::new(Partial(&[0xab]));
        assert_eq!(reader.read_bits(8).unwrap(), 0xab);

        // Bytes beyond the requested bits that a single read returns are kept.
        let mut reader = BitReader::new(Partial(&[0xab, 0xcd]));
        assert_eq!(reader.read_bits(4).unwrap(), 0xa);
        assert_eq!(reader.read_bits(12).unwrap(), 0xbcd);
    }

    #[test]
    fn test_invalid_writes_are_rejected() {
        let mut writer = BitWriter::new(Vec::new());
        assert!(writer.write_bits(0, 65).is_err(), "More than 64 bits must be rejected");
        assert!(writer.write_bits(0b100, 2).is_err(), "A value must fit in its bits");
    }
}
