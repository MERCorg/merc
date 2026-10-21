use std::io::Read;
use std::io::Write;
use std::io::{self};

use log::error;

use merc_number::read_variablelength;
use merc_number::write_variablelength;
use merc_utilities::MercError;

use crate::BitReader;
use crate::BitWriter;

/// Trait for writing bit-level data.
pub trait BitStreamWrite {
    /// Writes the low `number_of_bits` bits of `value`.
    ///
    /// # Errors
    ///
    /// Returns an error if `number_of_bits` exceeds 64, or `value` does not fit in it.
    fn write_bits(&mut self, value: u64, number_of_bits: u8) -> Result<(), MercError>;

    /// Writes a string prefixed with its length as a variable-width integer.
    fn write_string(&mut self, s: &str) -> Result<(), MercError>;

    /// Writes a u64 value using variable-width encoding.
    fn write_integer(&mut self, value: u64) -> Result<(), MercError>;

    /// Flushes any remaining bits to the underlying writer.
    fn flush(&mut self) -> Result<(), MercError>;
}

/// Trait for reading bit-level data.
pub trait BitStreamRead {
    /// Reads `number_of_bits` bits into the low bits of a u64.
    ///
    /// # Errors
    ///
    /// Returns an error if `number_of_bits` exceeds 64.
    fn read_bits(&mut self, number_of_bits: u8) -> Result<u64, MercError>;

    /// Reads a length-prefixed string.
    ///
    /// # Errors
    ///
    /// Returns an error if the encoded length does not fit in a `usize`, the
    /// bytes cannot be allocated, or the bytes are not valid UTF-8.
    fn read_string(&mut self) -> Result<String, MercError>;

    /// Reads a variable-width encoded integer.
    fn read_integer(&mut self) -> Result<u64, MercError>;
}

/// Bit-level writer wrapping an underlying byte writer.
pub struct BitStreamWriter<W: Write> {
    writer: BitWriter<W>,
}

impl<W: Write> BitStreamWriter<W> {
    /// Wraps `writer` for bit-level output.
    pub fn new(writer: W) -> Self {
        Self {
            writer: BitWriter::new(writer),
        }
    }
}

impl<W: Write> Drop for BitStreamWriter<W> {
    fn drop(&mut self) {
        if self.flush().is_err() {
            error!("Error flushing the stream when dropped!")
        }
    }
}

/// Bit-level reader wrapping an underlying byte reader.
pub struct BitStreamReader<R: Read> {
    reader: BitReader<R>,
    text_buffer: Vec<u8>,
}

impl<R: Read> BitStreamReader<R> {
    /// Wraps `reader` for bit-level input.
    pub fn new(reader: R) -> Self {
        Self {
            reader: BitReader::new(reader),
            text_buffer: Vec::with_capacity(128),
        }
    }
}

impl<W: Write> BitStreamWrite for BitStreamWriter<W> {
    #[inline]
    fn write_bits(&mut self, value: u64, number_of_bits: u8) -> Result<(), MercError> {
        Ok(self.writer.write_bits(value, u32::from(number_of_bits))?)
    }

    fn write_string(&mut self, s: &str) -> Result<(), MercError> {
        self.write_integer(s.len() as u64)?;
        for byte in s.bytes() {
            self.writer.write_bits(u64::from(byte), 8)?;
        }
        Ok(())
    }

    fn write_integer(&mut self, value: u64) -> Result<(), MercError> {
        Ok(write_variablelength(value, |byte| {
            self.writer.write_bits(u64::from(byte), 8)
        })?)
    }

    fn flush(&mut self) -> Result<(), MercError> {
        Ok(self.writer.flush()?)
    }
}

impl<R: Read> BitStreamRead for BitStreamReader<R> {
    #[inline]
    fn read_bits(&mut self, number_of_bits: u8) -> Result<u64, MercError> {
        Ok(self.reader.read_bits(u32::from(number_of_bits))?)
    }

    fn read_string(&mut self) -> Result<String, MercError> {
        let length = self.read_integer()?;
        let length_usize: usize = length
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "string length exceeds usize"))?;

        self.text_buffer.clear();
        self.text_buffer
            .try_reserve(length_usize)
            .map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "string too large to allocate"))?;
        for _ in 0..length_usize {
            let byte = self.reader.read_bits(8)? as u8;
            self.text_buffer.push(byte);
        }

        // Validate in place and copy the result out, so `text_buffer` keeps its
        // allocation for reuse on the next call (taking it would reset it to empty).
        std::str::from_utf8(&self.text_buffer)
            .map(str::to_owned)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e).into())
    }

    fn read_integer(&mut self) -> Result<u64, MercError> {
        Ok(read_variablelength(|| self.reader.read_bits(8).map(|byte| byte as u8))?)
    }
}

#[cfg(test)]
mod tests {
    use super::BitStreamRead;
    use super::BitStreamReader;
    use super::BitStreamWrite;
    use super::BitStreamWriter;

    use log::debug;
    use rand::RngExt;
    use rand::distr::Alphanumeric;

    use merc_utilities::random_test;

    /// Decide (arbitrarily) what to write into the bitstream.
    #[derive(Debug)]
    enum Instruction {
        String(String),
        Integer(u64),
        /// (value, num_of_bits), where num_of_bits must be at most 64.
        Bits(u64, u8),
    }

    /// Encodes the instructions bit by bit, independently of the writer, as the reference for the
    /// format: most significant bit first, integers in groups of seven bits with the least
    /// significant group first, and zero bits to pad the last byte.
    fn reference_encoding(instructions: &[Instruction]) -> Vec<u8> {
        fn push(bits: &mut Vec<bool>, value: u64, number_of_bits: u8) {
            for i in (0..number_of_bits).rev() {
                bits.push((value >> i) & 1 == 1);
            }
        }

        fn push_integer(bits: &mut Vec<bool>, mut value: u64) {
            while value > 0b0111_1111 {
                push(bits, (value & 0b0111_1111) | 0b1000_0000, 8);
                value >>= 7;
            }
            push(bits, value, 8);
        }

        let mut bits = Vec::new();
        for instruction in instructions {
            match instruction {
                Instruction::String(string) => {
                    push_integer(&mut bits, string.len() as u64);
                    for byte in string.bytes() {
                        push(&mut bits, u64::from(byte), 8);
                    }
                }
                Instruction::Integer(value) => push_integer(&mut bits, *value),
                Instruction::Bits(value, number_of_bits) => push(&mut bits, *value, *number_of_bits),
            }
        }

        bits.chunks(8)
            .map(|byte| {
                byte.iter()
                    .chain(std::iter::repeat(&false))
                    .take(8)
                    .fold(0u8, |acc, &bit| (acc << 1) | u8::from(bit))
            })
            .collect()
    }

    /// Minimum number of bits needed to represent `value` (at least 1).
    pub(super) fn required_bits(value: u64) -> u8 {
        if value == 0 {
            1
        } else {
            64 - value.leading_zeros() as u8
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_arbitrary_bitstream() {
        random_test(100, |rng| {
            let instructions: Vec<Instruction> = (0..100)
                .map(|_| match rng.random_range(0..3) {
                    0 => {
                        let string = rng.sample_iter(&Alphanumeric).take(7).map(char::from).collect();
                        Instruction::String(string)
                    }
                    1 => Instruction::Integer(rng.random()),
                    2 => {
                        let value: u64 = rng.random();
                        Instruction::Bits(value, required_bits(value))
                    }
                    _ => unreachable!("The range is from 0 to 3"),
                })
                .collect();

            let mut buffer = Vec::new();
            {
                let mut writer = BitStreamWriter::new(&mut buffer);

                for inst in &instructions {
                    debug!("Writing {inst:?}");
                    match inst {
                        Instruction::String(string) => {
                            writer.write_string(string).expect("Failed to write into stream")
                        }
                        Instruction::Integer(value) => {
                            writer.write_integer(*value).expect("Failed to write into stream")
                        }
                        Instruction::Bits(value, number_of_bits) => writer
                            .write_bits(*value, *number_of_bits)
                            .expect("Failed to write into stream"),
                    }
                }

                writer.flush().expect("Failed to write into stream");
            }

            assert_eq!(
                buffer,
                reference_encoding(&instructions),
                "The encoding must match the reference"
            );

            let mut reader = BitStreamReader::new(&buffer[..]);

            for inst in &instructions {
                debug!("Checking {inst:?}");
                match inst {
                    Instruction::String(string) => {
                        debug_assert_eq!(
                            reader.read_string().expect("Failed to read from stream"),
                            *string,
                            "Failed to read back the string"
                        )
                    }
                    Instruction::Integer(value) => {
                        debug_assert_eq!(
                            reader.read_integer().expect("Failed to read from stream"),
                            *value,
                            "Failed to read back the integer"
                        )
                    }
                    Instruction::Bits(value, number_of_bits) => {
                        debug_assert_eq!(
                            reader.read_bits(*number_of_bits).expect("Failed to read from stream"),
                            *value,
                            "Failed to read back the bits"
                        )
                    }
                }
            }
        });
    }

    /// Writes the given strings and reads them back, asserting they round-trip.
    fn roundtrip_strings(strings: &[&str]) {
        let mut buffer = Vec::new();
        {
            let mut writer = BitStreamWriter::new(&mut buffer);
            for s in strings {
                writer.write_string(s).expect("Failed to write string");
            }
            writer.flush().expect("Failed to flush");
        }

        let mut reader = BitStreamReader::new(&buffer[..]);
        for s in strings {
            assert_eq!(&reader.read_string().expect("Failed to read string"), s);
        }
    }

    #[test]
    fn test_string_edge_cases() {
        // Empty strings, multi-byte UTF-8, and a long string all in one stream so
        // that the shared `text_buffer` is reused across reads of differing length.
        roundtrip_strings(&["", "a", "", "héllo wörld", "🦀∑≈ç", &"x".repeat(10_000), ""]);
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_string_roundtrip_random_unicode() {
        random_test(100, |rng| {
            let strings: Vec<String> = (0..50)
                .map(|_| {
                    let len = rng.random_range(0..32);
                    (0..len).map(|_| rng.random::<char>()).collect()
                })
                .collect();
            let refs: Vec<&str> = strings.iter().map(String::as_str).collect();
            roundtrip_strings(&refs);
        });
    }

    #[test]
    fn test_read_string_rejects_invalid_utf8() {
        // Manually craft a stream: length 2 followed by an invalid UTF-8 sequence.
        let mut buffer = Vec::new();
        {
            let mut writer = BitStreamWriter::new(&mut buffer);
            writer.write_integer(2).expect("Failed to write length");
            writer.write_bits(0xFF, 8).expect("Failed to write byte");
            writer.write_bits(0xFE, 8).expect("Failed to write byte");
            writer.flush().expect("Failed to flush");
        }

        let mut reader = BitStreamReader::new(&buffer[..]);
        assert!(reader.read_string().is_err(), "Invalid UTF-8 should produce an error");
    }

    #[test]
    fn test_integer_boundaries() {
        // Edge values and the boundaries at which the encoding grows by a byte, written after three
        // bits so that no integer is byte aligned.
        let mut values = vec![0, u64::MAX];
        for shift in (7..64).step_by(7) {
            values.extend([(1u64 << shift) - 1, 1u64 << shift]);
        }

        let mut buffer = Vec::new();
        {
            let mut writer = BitStreamWriter::new(&mut buffer);
            writer.write_bits(0b101, 3).unwrap();
            for value in &values {
                writer.write_integer(*value).unwrap();
            }
            writer.flush().unwrap();
        }

        let mut reader = BitStreamReader::new(&buffer[..]);
        assert_eq!(reader.read_bits(3).unwrap(), 0b101);
        for value in &values {
            assert_eq!(reader.read_integer().unwrap(), *value);
        }
    }

    #[test]
    fn test_read_past_end_is_error() {
        let mut buffer = Vec::new();
        {
            let mut writer = BitStreamWriter::new(&mut buffer);
            writer.write_bits(0b10110, 5).unwrap();
            writer.flush().unwrap();
        }
        assert_eq!(buffer, vec![0b1011_0000], "The last byte is padded with zero bits");

        let mut reader = BitStreamReader::new(&buffer[..]);
        assert_eq!(reader.read_bits(5).unwrap(), 0b10110);
        assert_eq!(reader.read_bits(3).unwrap(), 0, "The padding reads as zero bits");
        assert!(reader.read_bits(1).is_err(), "Reading past the end must fail");
    }

    #[test]
    fn test_write_bits_rejects_too_many_bits() {
        let mut buffer = Vec::new();
        let mut writer = BitStreamWriter::new(&mut buffer);
        assert!(writer.write_bits(0, 65).is_err(), "More than 64 bits must be rejected");
    }
}
