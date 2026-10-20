#[cfg(kani)]
use rand::TryRng;

/// A simple wrapper around `kani::any` to implement the `TryRng` trait, which
/// is required by the random LTS generation functions.
///
/// This adapter lets any test helper that takes a `TryRng` (e.g. `random_lts`)
/// be reused inside a Kani harness without rewriting it: every drawn byte
/// becomes a symbolic input that Kani enumerates.
#[cfg(kani)]
pub struct ArbRng;

#[cfg(kani)]
impl TryRng for ArbRng {
    type Error = std::convert::Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        Ok(kani::any())
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        Ok(kani::any())
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        for byte in dst {
            *byte = kani::any();
        }

        Ok(())
    }
}
