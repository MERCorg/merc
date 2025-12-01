use merc_tools::verbosity::Verbosity;

#[cxx::bridge(namespace = "mcrl2::log")]
pub mod ffi {
    unsafe extern "C++" {
        include!("mcrl2-sys/cpp/log.h");

        /// Sets the reporting level for mCRL2 utilities logging.
        fn mcrl2_set_reporting_level(level: usize);
    }
}

pub fn verbosity_to_log_level_t(verbosity: Verbosity) -> usize {
    match verbosity {
        Verbosity::Quiet => 0,
        Verbosity::Verbose => 5,
        Verbosity::Debug => 6,
        Verbosity::Trace => 7,
    }
}