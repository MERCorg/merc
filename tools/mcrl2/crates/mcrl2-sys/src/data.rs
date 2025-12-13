#[cxx::bridge(namespace = "mcrl2::data")]
pub mod ffi {
    unsafe extern "C++" {
        include!("mcrl2-sys/cpp/data.h");
        include!("mcrl2-sys/cpp/exception.h");

        type data_specification;

        /// Creates a data specification from the given string.
        fn mcrl2_data_specification_from_string(input: &str) -> UniquePtr<data_specification>;

        #[namespace = "mcrl2::data::detail"]
        type RewriterJitty;

        #[cfg(feature = "mcrl2_jittyc")]
        #[namespace = "mcrl2::data::detail"]
        type RewriterCompilingJitty;

        /// Creates a jitty rewriter from the given data specification.
        fn mcrl2_create_rewriter_jitty(data_spec: &data_specification) -> UniquePtr<RewriterJitty>;

        /// Creates a compiling rewriter from the given data specification.
        #[cfg(feature = "mcrl2_jittyc")]
        fn mcrl2_create_rewriter_jittyc(data_spec: &data_specification) -> UniquePtr<RewriterCompilingJitty>;

        #[namespace = "atermpp"]
        type aterm = crate::atermpp::ffi::aterm;

        // Recognizers for the various variants of data expressions.
        fn mcrl2_data_expression_is_variable(input: &aterm) -> bool;
        fn mcrl2_data_expression_is_application(input: &aterm) -> bool;
        fn mcrl2_data_expression_is_abstraction(input: &aterm) -> bool;
        fn mcrl2_data_expression_is_function_symbol(input: &aterm) -> bool;
        fn mcrl2_data_expression_is_where_clause(input: &aterm) -> bool;
        fn mcrl2_data_expression_is_machine_number(input: &aterm) -> bool;
        fn mcrl2_data_expression_is_untyped_identifier(input: &aterm) -> bool;
        fn mcrl2_data_expression_is_data_expression(input: &aterm) -> bool;
    }
}
