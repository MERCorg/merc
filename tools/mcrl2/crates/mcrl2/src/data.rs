use mcrl2_sys::cxx::UniquePtr;
use mcrl2_sys::data::ffi::RewriterJitty;
use mcrl2_sys::data::ffi::data_specification;
use mcrl2_sys::data::ffi::mcrl2_create_rewriter_jitty;
use mcrl2_sys::data::ffi::mcrl2_data_specification_from_string;
use mcrl2_sys::data::ffi::mcrl2_data_specification_user_defined_aliases;
use mcrl2_sys::data::ffi::mcrl2_data_specification_user_defined_constructors;
use mcrl2_sys::data::ffi::mcrl2_data_specification_user_defined_equations;
use mcrl2_sys::data::ffi::mcrl2_data_specification_user_defined_mappings;
use mcrl2_sys::data::ffi::mcrl2_data_specification_user_defined_sorts;

#[cfg(feature = "jittyc")]
use mcrl2_sys::data::ffi::RewriterCompilingJitty;
#[cfg(feature = "jittyc")]
use mcrl2_sys::data::ffi::mcrl2_create_rewriter_jittyc;

use crate::ATerm;
use crate::ATermList;
use crate::lock_global;

pub struct DataSpecification {
    spec: UniquePtr<data_specification>,
}

impl DataSpecification {
    /// Creates a new data specification from the given UniquePtr.
    pub(crate) fn new(spec: UniquePtr<data_specification>) -> Self {
        DataSpecification { spec }
    }

    /// Parses `input` as an mCRL2 data specification and returns the result.
    ///
    /// Acquires the global lock because mCRL2's parser is not thread-safe.
    pub fn from_string(input: &str) -> Self {
        let _guard = lock_global();
        DataSpecification {
            spec: mcrl2_data_specification_from_string(input),
        }
    }

    /// Returns a reference to the underlying UniquePtr.
    pub(crate) fn get(&self) -> &UniquePtr<data_specification> {
        &self.spec
    }

    fn spec_ref(&self) -> &data_specification {
        self.spec
            .as_ref()
            .expect("DataSpecification inner pointer is never null")
    }

    /// Returns the user-declared sorts as an aterm list.
    pub fn user_defined_sorts(&self) -> ATermList<ATerm> {
        ATermList::from(ATerm::from_unique_ptr(mcrl2_data_specification_user_defined_sorts(
            self.spec_ref(),
        )))
    }

    /// Returns the user-declared sort aliases as an aterm list.
    pub fn user_defined_aliases(&self) -> ATermList<ATerm> {
        ATermList::from(ATerm::from_unique_ptr(mcrl2_data_specification_user_defined_aliases(
            self.spec_ref(),
        )))
    }

    /// Returns the user-declared constructors as an aterm list.
    pub fn user_defined_constructors(&self) -> ATermList<ATerm> {
        ATermList::from(ATerm::from_unique_ptr(
            mcrl2_data_specification_user_defined_constructors(self.spec_ref()),
        ))
    }

    /// Returns the user-declared mappings as an aterm list.
    pub fn user_defined_mappings(&self) -> ATermList<ATerm> {
        ATermList::from(ATerm::from_unique_ptr(mcrl2_data_specification_user_defined_mappings(
            self.spec_ref(),
        )))
    }

    /// Returns the user-declared equations as an aterm list.
    pub fn user_defined_equations(&self) -> ATermList<ATerm> {
        ATermList::from(ATerm::from_unique_ptr(mcrl2_data_specification_user_defined_equations(
            self.spec_ref(),
        )))
    }
}

/// Represents a mcrl2::data::detail::RewriterJitty from the mCRL2 toolset.
///
/// TODO: currently only constructs and owns the underlying rewriter; it exposes
/// no rewrite operation yet, so it is inert beyond holding the C++ object alive.
#[expect(dead_code, reason = "placeholder until the mCRL2 rewriter is exposed")]
pub(crate) struct Mcrl2RewriterJitty {
    _rewriter: UniquePtr<RewriterJitty>,
}

#[expect(dead_code, reason = "placeholder until the mCRL2 rewriter is exposed")]
impl Mcrl2RewriterJitty {
    /// Creates a new Jitty rewriter from the given data specification.
    pub fn new(data_spec: &DataSpecification) -> Self {
        let rewriter = mcrl2_create_rewriter_jitty(data_spec.get());
        Self { _rewriter: rewriter }
    }
}

#[cfg(feature = "jittyc")]
/// Represents a mcrl2::data::detail::RewriterJittyCompiling from the mCRL2 toolset.
///
/// TODO: currently only constructs and owns the underlying rewriter; it exposes
/// no rewrite operation yet, so it is inert beyond holding the C++ object alive.
pub(crate) struct Mcrl2RewriterJittyCompiling {
    _rewriter: UniquePtr<RewriterCompilingJitty>,
}

#[cfg(feature = "jittyc")]
impl Mcrl2RewriterJittyCompiling {
    /// Creates a new compiling Jitty rewriter from the given data specification.
    pub fn new(data_spec: &DataSpecification) -> Self {
        let rewriter = mcrl2_create_rewriter_jittyc(data_spec.get());
        Self { _rewriter: rewriter }
    }
}
