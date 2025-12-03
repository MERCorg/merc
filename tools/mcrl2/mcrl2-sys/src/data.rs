#[cxx::bridge(namespace = "mcrl2::data")]
pub mod ffi {
    unsafe extern "C++" {
        include!("mcrl2-sys/cpp/data.h");
        include!("mcrl2-sys/cpp/exception.h");

        type variable;

        /// Returns the variable in string form.
        fn mcrl2_variable_to_string(input: &variable) -> Result<String>;

        #[namespace = "atermpp"]
        type aterm_string = crate::atermpp::ffi::aterm_string;

        fn mcrl2_variable_name(input: &variable) -> Result<UniquePtr<aterm_string>>;

        type sort_expression;

        fn mcrl2_variable_sort(input: &variable) -> Result<UniquePtr<sort_expression>>;

        #[namespace = "atermpp"]
        type aterm = crate::atermpp::ffi::aterm;

        /// Returns true if the given term is correct.
        fn mcrl2_is_variable(input: &aterm) -> bool;

        type data_expression;
        
        fn mcrl2_data_expression() -> UniquePtr<data_expression>;
    }
}
