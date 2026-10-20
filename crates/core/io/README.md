# Overview

 > ⚠️ **important** This is an internal crate and is not intended for public use.
 
Internal MERC crate for I/O related utilities, including bit-level streams and
also handling of formatting and output to the console.

The `DumpFiles` struct can be used to manage dumping of intermediate results to
files, which can be very useful for debugging failing tests. It uses the
`MERC_DUMP` environment variable to determine the absolute path to dump the
files.

## Safety

This crate contains no unsafe code. If unsafe code is needed it should be in the `merc_unsafety` crate.

## Minimum Supported Rust Version

We do not maintain an official minimum supported rust version (MSRV), and it may be upgraded at any time when necessary.

## License

All MERC crates are licensed under the `BSL-1.0` license. See the [LICENSE](https://raw.githubusercontent.com/MERCorg/merc/refs/heads/main/LICENSE) file in the repository root for more information.