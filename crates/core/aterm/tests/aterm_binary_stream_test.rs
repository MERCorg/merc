//! Cross-thread integration test for [`BinaryATermWriter`]. Unlike the crate's other binary
//! stream tests (which live inline in `aterm_binary_stream.rs` since they exercise it from a
//! single thread, the common case), this one specifically drives the writer from a *different*
//! thread than the one that created it, so it belongs alongside the other integration tests
//! rather than in the unit test module.

use std::io::Write;
use std::sync::Arc;
use std::sync::Mutex;

use merc_aterm::ATerm;
use merc_aterm::ATermRead;
use merc_aterm::ATermWrite;
use merc_aterm::BinaryATermReader;
use merc_aterm::BinaryATermWriter;
use merc_aterm::Symbol;
use merc_aterm::Term;

/// A `Write` sink backed by a shared buffer, so the test can move a `BinaryATermWriter` into
/// another thread and still read back what it wrote from the original thread.
#[derive(Clone, Default)]
struct SharedBuf(Arc<Mutex<Vec<u8>>>);

impl Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Builds `f(a_i, g(a_i))` for a fresh, index-unique `a_i`. A plain function of `i` (rather than
/// capturing pre-built `ATerm`/`Symbol` values, which are themselves `!Send`) so the same term
/// can be reconstructed identically -- via maximal sharing -- on whichever thread calls it.
fn build_send_test_term(i: usize) -> ATerm {
    let a = ATerm::constant(&Symbol::new(format!("a_send_test{i}"), 0));
    let inner = ATerm::with_args(&Symbol::new("g_send_test", 1), &[a.copy()]).protect();
    ATerm::with_args(&Symbol::new("f_send_test", 2), &[a, inner]).protect()
}

#[test]
#[cfg_attr(miri, ignore)] // Spawns real OS threads and creates many terms; too slow under miri.
fn test_binary_writer_send_across_threads() {
    // `BinaryATermWriter` must be usable from a thread other than the one that created it: its
    // `function_symbols`/`terms`/`stack` fields are `GlobalProtected` precisely so it can be
    // moved like this (e.g. into a `MutexLtsBuilder` shared by parallel workers).
    const COUNT: usize = 200;
    const SPLIT: usize = 50;

    let buf = SharedBuf::default();
    let mut writer = BinaryATermWriter::new(buf.clone()).unwrap();

    // Write some terms on the creating thread before handing the writer off.
    for i in 0..SPLIT {
        writer.write_aterm(&build_send_test_term(i)).unwrap();
    }

    // Move the writer to another thread and finish writing there (rebuilding the remaining terms
    // there too, see `build_send_test_term`). Concurrently, keep creating and dropping terms on
    // *this* thread so a garbage collection is likely to run on one thread while the other is
    // mid-write -- exactly the scenario `GlobalProtected`'s send-container protection set exists
    // for (see its doc comment).
    let handle = std::thread::spawn(move || {
        let mut writer = writer;
        for i in SPLIT..COUNT {
            writer.write_aterm(&build_send_test_term(i)).unwrap();
        }
        ATermWrite::flush(&mut writer).unwrap();
    });

    for i in 0..2000 {
        let _ = ATerm::constant(&Symbol::new(format!("pressure_send_test{i}"), 0));
    }

    handle.join().unwrap();

    let bytes = buf.0.lock().unwrap().clone();
    let mut reader = BinaryATermReader::new(&bytes[..]).unwrap();
    for i in 0..COUNT {
        assert_eq!(
            reader.read_aterm().unwrap().unwrap(),
            build_send_test_term(i),
            "every term written across the thread hand-off must round-trip"
        );
    }
}
