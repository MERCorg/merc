use num::PrimInt;
use num::Unsigned;

/// The maximum number of bytes needed to encode a value of type T in most
/// significant bit encoding.
///
/// The encoding stores 7 payload bits per byte, so a `T` of `n` bits needs
/// `ceil(n / 7)` bytes. `((size_of::<T>() + 1) * 8) / 7` computes that ceiling
/// (e.g. 10 bytes for `u64`).
pub const fn encoding_size<T>() -> usize {
    ((std::mem::size_of::<T>() + 1) * 8) / 7
}

/// Encodes a given unsigned variable-length integer using the most significant bit (MSB) algorithm,
/// passing every byte to `write_byte`.
///
/// # Details
///
/// Every byte stores seven bits of the value, least significant first, and has its most
/// significant bit set when more bytes follow. Implementation taken from
/// <https://techoverflow.net/2013/01/25/efficiently-encoding-variable-length-integers-in-cc/>
pub fn write_variablelength<T, E>(mut value: T, mut write_byte: impl FnMut(u8) -> Result<(), E>) -> Result<(), E>
where
    T: PrimInt + Unsigned + From<u8>,
{
    let payload = <T as From<u8>>::from(0b01111111);

    // While more than 7 bits of data are left, occupy the last output byte
    // and set the next byte flag.
    while value > payload {
        write_byte(low_byte(value & payload) | 0b10000000)?;

        // Remove the seven bits we just wrote from value.
        value = value >> 7;
    }

    write_byte(low_byte(value))
}

/// Decodes an unsigned variable-length integer using the MSB algorithm, taking every byte from
/// `read_byte`.
pub fn read_variablelength<T, E>(mut read_byte: impl FnMut() -> Result<u8, E>) -> Result<T, E>
where
    T: PrimInt + Unsigned + From<u8>,
{
    let mut value = T::zero();
    for i in 0..encoding_size::<T>() {
        let byte = read_byte()?;

        // Take 7 bits (mask 0b01111111) from byte and shift it before the bits already written to value.
        value = value | (<T as From<u8>>::from(byte & 0b01111111) << (7 * i));

        if byte & 0b10000000 == 0 {
            // If the next-byte flag is not set then we are finished.
            break;
        }
    }

    Ok(value)
}

/// Returns `value`, which must be less than 256, as a byte.
fn low_byte<T: PrimInt>(value: T) -> u8 {
    value.to_u8().expect("The value fits in a byte")
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use rand::RngExt;

    use merc_utilities::random_test;

    use num::PrimInt;
    use num::Unsigned;

    use super::encoding_size;
    use super::read_variablelength;
    use super::write_variablelength;

    /// Round-trips `value` through the encoder and decoder and asserts equality.
    fn roundtrip<T: PrimInt + Unsigned + From<u8> + std::fmt::Debug>(value: T) {
        let mut bytes = Vec::new();
        write_variablelength(value, |byte| {
            bytes.push(byte);
            Ok::<(), Infallible>(())
        })
        .unwrap();
        assert!(bytes.len() <= encoding_size::<T>());

        let mut bytes = bytes.into_iter();
        assert_eq!(
            read_variablelength::<T, _>(|| bytes.next().ok_or("missing byte")).unwrap(),
            value
        );
        assert!(bytes.next().is_none(), "All bytes must be read");
    }

    #[test]
    fn test_encoding_size() {
        assert_eq!(encoding_size::<u8>(), 2);
        assert_eq!(encoding_size::<u16>(), 3);
        assert_eq!(encoding_size::<u32>(), 5);
        assert_eq!(encoding_size::<u64>(), 10);
    }

    #[test]
    fn test_boundary_encoding() {
        // Edge values and the per-byte continuation boundaries where the
        // encoded length grows by one byte (7 payload bits per byte).
        roundtrip(0u64);
        roundtrip(u64::MAX);
        for shift in [7, 14, 21, 28, 35, 42, 49, 56, 63] {
            roundtrip((1u64 << shift) - 1);
            roundtrip(1u64 << shift);
        }

        // Smaller types use fewer bytes, up to their own maximum.
        roundtrip(u8::MAX);
        roundtrip(u16::MAX);
        roundtrip(u32::MAX);
        roundtrip(1u32 << 28);
    }

    #[test]
    fn test_random_integer_encoding() {
        random_test(1000, |rng| {
            roundtrip(rng.random::<u64>());
            roundtrip(rng.random::<u32>());
        });
    }
}
