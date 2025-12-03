#[cxx::bridge(namespace = "atermpp")]
pub mod ffi {
    unsafe extern "C++" {
        include!("mcrl2-sys/cpp/atermpp.h");
        include!("mcrl2-sys/cpp/exception.h");

        type aterm;


        /// Returns the `index` argument of the term.
        fn mcrl2_aterm_argument(input: &aterm, index: usize) -> UniquePtr<aterm>;

        /// Converts the given aterm to a string.
        fn mcrl2_aterm_to_string(input: &aterm) -> Result<String>;

        type aterm_string;

        fn mcrl2_aterm_string_to_string(input: &aterm_string) -> Result<String>;

        fn mcrl2_aterm_string() -> UniquePtr<aterm_string>;

        type aterm_list;

        /// Returns the size of the aterm list.
        fn mcrl2_aterm_list_front(input: &aterm_list) -> UniquePtr<aterm>;

        fn mcrl2_aterm_list_tail(input: &aterm_list) -> UniquePtr<aterm>;

        fn mcrl2_aterm_list() -> UniquePtr<aterm_list>;
    }
}
