use std::cell::RefCell;

use mcrl2_sys::atermpp::ffi::_aterm;
use mcrl2_sys::cxx::UniquePtr;
use mcrl2_sys::lps::ffi::learn_successors_context;
use mcrl2_sys::lps::ffi::mcrl2_lps_action_summand;
use mcrl2_sys::lps::ffi::mcrl2_lps_action_summand_assignments;
use mcrl2_sys::lps::ffi::mcrl2_lps_action_summand_condition;
use mcrl2_sys::lps::ffi::mcrl2_lps_action_summand_multi_action;
use mcrl2_sys::lps::ffi::mcrl2_lps_action_summand_summation_variables;
use mcrl2_sys::lps::ffi::mcrl2_lps_create_learn_successors_context;
use mcrl2_sys::lps::ffi::mcrl2_lps_create_learn_successors_context_from_data_spec;
use mcrl2_sys::lps::ffi::mcrl2_lps_data_specification;
use mcrl2_sys::lps::ffi::mcrl2_lps_enumerate;
use mcrl2_sys::lps::ffi::mcrl2_lps_load_from_lps_file;
use mcrl2_sys::lps::ffi::mcrl2_lps_load_from_text_file;
use mcrl2_sys::lps::ffi::mcrl2_lps_multi_action_to_string;
use mcrl2_sys::lps::ffi::mcrl2_lps_num_of_action_summands;
use mcrl2_sys::lps::ffi::mcrl2_lps_preprocess_symbolic_exploration;
use mcrl2_sys::lps::ffi::mcrl2_lps_process_initializer;
use mcrl2_sys::lps::ffi::mcrl2_lps_process_initializer_expressions;
use mcrl2_sys::lps::ffi::mcrl2_lps_process_parameters;
use mcrl2_sys::lps::ffi::mcrl2_lps_rewrite_under_sigma;
use mcrl2_sys::lps::ffi::mcrl2_lps_set_assignments;
use mcrl2_sys::lps::ffi::mcrl2_lps_tau_multi_action;
use mcrl2_sys::lps::ffi::mcrl2_preprocessed_specification_constant_assignments;
use mcrl2_sys::lps::ffi::mcrl2_preprocessed_specification_spec;
use mcrl2_sys::lps::ffi::stochastic_action_summand;
use mcrl2_sys::lps::ffi::stochastic_process_initializer;
use mcrl2_sys::lps::ffi::stochastic_specification;

use merc_utilities::MercError;
use merc_utilities::PhantomUnsend;

use crate::ATerm;
use crate::ATermList;
use crate::DataExpression;
use crate::DataExpressionRef;
use crate::DataRewriter;
use crate::DataSpecification;
use crate::DataVariable;

/// A linear process specification.
///
/// This is a wrapper around the `mcrl2::lps::specification` class, which
/// represents a linear process specification (LPS) in mCRL2. An LPS is a number
/// of summands that each specify a condition-action-effect triple, and it
/// has an initial state.
pub struct LinearProcessSpecification {
    lps: UniquePtr<stochastic_specification>,

    /// Constant substitution (`@rewr_var := value`) recorded when the
    /// `replace_constants_by_variables` preprocessing step is applied.
    constant_assignments: Option<ATerm>,

    /// The underlying `stochastic_specification` stores thread-affine `aterm`s,
    /// so the wrapper must not cross threads.
    _marker: PhantomUnsend,
}

impl LinearProcessSpecification {
    /// Returns the initial process of the LPS, which is the process that is specified by the initial state.
    pub fn initial_process(&self) -> LinearProcessInitializer {
        LinearProcessInitializer {
            init: mcrl2_lps_process_initializer(self.lps.as_ref().expect("The lps is always defined"))
                .expect("The initial process is always defined"),
            _marker: PhantomUnsend::default(),
        }
    }

    /// Returns the parameters of the LPS as an aterm list of data variables.
    pub fn parameters(&self) -> ATermList<DataVariable> {
        // SAFETY: the FFI returns the live process-parameters term of this LPS.
        ATermList::from(unsafe {
            ATerm::from_ptr(mcrl2_lps_process_parameters(
                self.lps.as_ref().expect("The lps is always defined"),
            ))
        })
    }

    /// Returns the data specification of the LPS.
    pub fn data_specification(&self) -> DataSpecification {
        DataSpecification::new(mcrl2_lps_data_specification(
            self.lps.as_ref().expect("The lps is always defined"),
        ))
    }

    /// Returns the number of summands in the LPS.
    pub fn num_summands(&self) -> usize {
        mcrl2_lps_num_of_action_summands(self.lps.as_ref().expect("The lps is always defined"))
    }

    /// Returns the action summand at the given index.
    pub fn action_summand(&self, index: usize) -> Result<LinearSummand, MercError> {
        Ok(LinearSummand {
            summand: mcrl2_lps_action_summand(self.lps.as_ref().expect("The lps is always defined"), index)?,
            _marker: PhantomUnsend::default(),
        })
    }
}

/// Represents a `condition-action-effect` summand of an LPS.
pub struct LinearSummand {
    summand: UniquePtr<stochastic_action_summand>,

    /// The underlying summand holds thread-affine `aterm`s, so the wrapper must
    /// not cross threads. See [`LinearProcessSpecification`].
    _marker: PhantomUnsend,
}

impl LinearSummand {
    /// Returns the condition of this summand.
    pub fn condition(&self) -> DataExpression {
        // SAFETY: the FFI returns the live condition term of this summand.
        DataExpression::new(unsafe {
            ATerm::from_ptr(mcrl2_lps_action_summand_condition(
                self.summand.as_ref().expect("The summand is always defined"),
            ))
        })
    }

    /// Returns the summation variables of this summand (the "sum" variables).
    pub fn summation_variables(&self) -> ATermList<DataVariable> {
        // SAFETY: the FFI returns the live summation-variables term of this summand.
        ATermList::from(unsafe {
            ATerm::from_ptr(mcrl2_lps_action_summand_summation_variables(
                self.summand.as_ref().expect("The summand is always defined"),
            ))
        })
    }

    /// Returns the multi-action of this summand.
    pub fn multi_action(&self) -> ATerm {
        // SAFETY: the FFI returns the live multi-action term of this summand.
        unsafe {
            ATerm::from_ptr(mcrl2_lps_action_summand_multi_action(
                self.summand.as_ref().expect("The summand is always defined"),
            ))
        }
    }

    /// Returns the assignments (update) of this summand as an aterm list.
    /// Each assignment represents `variable := expression` for the next state.
    pub fn assignments(&self) -> ATermList<ATerm> {
        // SAFETY: the FFI returns the live assignment-list term of this summand.
        ATermList::from(unsafe {
            ATerm::from_ptr(mcrl2_lps_action_summand_assignments(
                self.summand.as_ref().expect("The summand is always defined"),
            ))
        })
    }
}

/// Represents the initial process of an LPS.
pub struct LinearProcessInitializer {
    init: UniquePtr<stochastic_process_initializer>,

    /// The underlying initializer holds thread-affine `aterm`s, so the wrapper
    /// must not cross threads. See [`LinearProcessSpecification`].
    _marker: PhantomUnsend,
}

impl LinearProcessInitializer {
    /// Returns the initial state expressions as an aterm list of data expressions.
    pub fn expressions(&self) -> ATermList<DataExpression> {
        // SAFETY: the FFI returns the live initial-state expressions term.
        ATermList::from(unsafe {
            ATerm::from_ptr(mcrl2_lps_process_initializer_expressions(
                self.init.as_ref().expect("The initializer is always defined"),
            ))
        })
    }
}

/// Pretty-prints a multi-action term using the mCRL2 pretty printer.
pub fn pretty_print_multi_action(multi_action: &ATerm) -> String {
    mcrl2_lps_multi_action_to_string(multi_action.get())
}

/// Returns the tau (empty) multi-action term as a protected [`ATerm`].
pub fn tau_multi_action() -> ATerm {
    // SAFETY: the FFI returns the live tau multi-action term.
    unsafe { ATerm::from_ptr(mcrl2_lps_tau_multi_action()) }
}

/// Read an LPS from a file in the binary mCRL2 format.
pub fn read_lps(filename: &str) -> Result<LinearProcessSpecification, MercError> {
    Ok(LinearProcessSpecification {
        lps: mcrl2_lps_load_from_lps_file(filename)?,
        constant_assignments: None,
        _marker: PhantomUnsend::default(),
    })
}

/// Read an LPS from a textual mCRL2 process specification file.
pub fn read_lps_text(filename: &str) -> Result<LinearProcessSpecification, MercError> {
    Ok(LinearProcessSpecification {
        lps: mcrl2_lps_load_from_text_file(filename)?,
        constant_assignments: None,
        _marker: PhantomUnsend::default(),
    })
}

/// Toggles for the individual preprocessing steps applied by [`preprocess`].
#[derive(Debug, Clone, Copy)]
pub struct PreprocessOptions {
    /// Replace global variables by concrete values.
    pub instantiate_global_variables: bool,

    /// Order the summation variables of every summand.
    pub order_summand_variables: bool,

    /// Resolve name clashes between summation variables. Required by the enumerator.
    pub resolve_name_clashes: bool,

    /// Apply the one-point rule rewriter.
    pub one_point_rule_rewrite: bool,

    /// Replace constant subexpressions by fresh variables and record them in the
    /// enumeration substitution.
    pub replace_constants_by_variables: bool,
}

impl Default for PreprocessOptions {
    fn default() -> Self {
        Self {
            instantiate_global_variables: true,
            order_summand_variables: true,
            resolve_name_clashes: true,
            one_point_rule_rewrite: true,
            replace_constants_by_variables: true,
        }
    }
}

/// Preprocess the LPS to make it suitable for symbolic exploration.
///
/// The individual preprocessing steps are enabled according to `options`; see
/// [`PreprocessOptions`]. This mirrors the preprocessing performed by the mCRL2
/// explorer used by `lps2lts`.
pub fn preprocess(
    lps: &LinearProcessSpecification,
    options: &PreprocessOptions,
) -> Result<LinearProcessSpecification, MercError> {
    let preprocessed = mcrl2_lps_preprocess_symbolic_exploration(
        lps.lps.as_ref().expect("The lps is always defined"),
        options.instantiate_global_variables,
        options.order_summand_variables,
        options.resolve_name_clashes,
        options.one_point_rule_rewrite,
        options.replace_constants_by_variables,
    )?;
    let preprocessed = preprocessed
        .as_ref()
        .expect("The preprocessed specification is always defined");

    // SAFETY: the FFI returns the live constant-assignments term of the preprocessed spec.
    let constant_assignments =
        unsafe { ATerm::from_ptr(mcrl2_preprocessed_specification_constant_assignments(preprocessed)) };

    Ok(LinearProcessSpecification {
        lps: mcrl2_preprocessed_specification_spec(preprocessed),
        constant_assignments: Some(constant_assignments),
        _marker: PhantomUnsend::default(),
    })
}

/// Context for learning successors during symbolic exploration.
pub struct LearnSuccessorsContext {
    context: RefCell<UniquePtr<learn_successors_context>>,

    /// The context owns a per-thread mCRL2 enumerator backed by thread-affine
    /// `aterm`s, so it must be created and destroyed on the same thread.
    _marker: PhantomUnsend,
}

impl LearnSuccessorsContext {
    /// Creates a new context from the given LPS specification, which rewrites
    /// with a clone of `rewriter`, a rewriter for the data specification of `lps`.
    ///
    /// When the LPS carries a constant substitution recorded by the
    /// `replace_constants_by_variables` preprocessing step, it is seeded into
    /// the context's substitution.
    pub fn new(lps: &LinearProcessSpecification, rewriter: &DataRewriter) -> Self {
        let context = LearnSuccessorsContext {
            context: RefCell::new(mcrl2_lps_create_learn_successors_context(
                lps.lps.as_ref().expect("The lps is always defined"),
                rewriter.get(),
            )),
            _marker: PhantomUnsend::default(),
        };

        if let Some(assignments) = &lps.constant_assignments {
            context.seed_constant_assignments(assignments);
        }

        context
    }

    /// Seeds the persistent substitution with the constant assignments
    /// (`@rewr_var := value`) recorded during preprocessing.
    fn seed_constant_assignments(&self, assignments: &ATerm) {
        let list: ATermList<ATerm> = ATermList::from(assignments.protect());

        let mut variables = Vec::new();
        let mut values = Vec::new();
        for assignment in list.iter() {
            let variable = assignment.arg(0);
            let value = assignment.arg(1);
            log::debug!("Seeding sigma with constant assignment {variable} := {value}");
            variables.push(variable.address());
            values.push(value.address());
        }

        if !variables.is_empty() {
            self.set_assignments(&variables, &values);
        }
    }

    /// Creates a new context from a data specification (used when no LPS is
    /// available), which rewrites with a clone of `rewriter`, a rewriter for
    /// `data_spec`.
    pub fn from_data_spec(data_spec: &DataSpecification, rewriter: &DataRewriter) -> Self {
        LearnSuccessorsContext {
            context: RefCell::new(mcrl2_lps_create_learn_successors_context_from_data_spec(
                data_spec.get(),
                rewriter.get(),
            )),
            _marker: PhantomUnsend::default(),
        }
    }

    /// Enumerate all solutions for the summand's condition under the given
    /// read parameter assignments from the current state.
    ///
    /// For each solution found, calls `callback` with a slice of the
    /// next-state values (one per assignment in the summand) and a pointer
    /// to the rewritten multi-action term.
    pub fn enumerate<F>(
        &self,
        summand: &LinearSummand,
        read_parameters: &[*const _aterm],
        read_values: &[*const _aterm],
        callback: F,
    ) where
        F: FnMut(&[*const _aterm], *const _aterm),
    {
        let condition = summand.condition();
        let summation_variables = summand.summation_variables();
        let assignments = summand.assignments();
        let multi_action = summand.multi_action();
        self.set_assignments(read_parameters, read_values);
        self.enumerate_raw_inner(
            condition.get(),
            ATerm::from(summation_variables).get(),
            ATerm::from(assignments).get(),
            multi_action.get(),
            callback,
        );
    }

    /// Assign variables in the persistent substitution (sigma).
    pub fn set_assignments(&self, variables: &[*const _aterm], values: &[*const _aterm]) {
        assert_eq!(
            variables.len(),
            values.len(),
            "Variables and values must have equal length"
        );

        let mut context = self.context.borrow_mut();
        // The only error the FFI reports is the length mismatch asserted above.
        mcrl2_lps_set_assignments(
            context.as_mut().expect("The context is always defined"),
            variables,
            values,
        )
        .expect("variables and values have equal length");
    }

    /// Rewrites `expr` under the context's current substitution (sigma) and
    /// returns the resulting term as a protected [`ATerm`].
    pub fn rewrite_under_sigma(&self, expr: &DataExpressionRef<'_>) -> ATerm {
        let mut context = self.context.borrow_mut();
        // SAFETY: `expr` is a live term reference, and the returned pointer is
        // immediately protected by `ATerm::from_ptr` before any further aterm
        // operation can collect it.
        let result = unsafe {
            mcrl2_lps_rewrite_under_sigma(context.as_mut().expect("The context is always defined"), expr.get())
        };
        // SAFETY: `result` is the live rewritten term just returned by the FFI.
        unsafe { ATerm::from_ptr(result) }
    }

    /// Enumerate using stored ATerm values directly.
    ///
    /// This is useful when the summand's condition, summation variables,
    /// assignments and multi-action have already been extracted and stored as
    /// aterms. For each solution the callback receives a slice of next-state
    /// values together with a pointer to the multi-action term rewritten under
    /// the current substitution.
    #[allow(clippy::too_many_arguments)]
    pub fn enumerate_raw<F>(
        &self,
        // Information of the summand
        condition: &DataExpression,
        summation_variables: &ATermList<DataVariable>,
        assignments: &ATermList<ATerm>,
        multi_action: &ATerm,
        // Encodes the current substitution
        read_parameters: &[*const _aterm],
        read_values: &[*const _aterm],
        callback: F,
    ) where
        F: FnMut(&[*const _aterm], *const _aterm),
    {
        self.set_assignments(read_parameters, read_values);
        self.enumerate_raw_inner(
            condition.get(),
            summation_variables.get(),
            assignments.get(),
            multi_action.get(),
            callback,
        );
    }

    /// Enumerate using stored ATerm values directly with assignments that are
    /// already present in sigma.
    pub fn enumerate_raw_with_current_assignments<F>(
        &self,
        condition: &DataExpression,
        summation_variables: &ATermList<DataVariable>,
        assignments: &ATermList<ATerm>,
        multi_action: &ATerm,
        callback: F,
    ) where
        F: FnMut(&[*const _aterm], *const _aterm),
    {
        self.enumerate_raw_inner(
            condition.get(),
            summation_variables.get(),
            assignments.get(),
            multi_action.get(),
            callback,
        );
    }

    /// The implementation of enumeration that takes raw aterm pointers. This is
    /// used by both `enumerate` and `enumerate_raw` to avoid code duplication.
    fn enumerate_raw_inner<F>(
        &self,
        condition: &_aterm,
        summation_variables: &_aterm,
        assignments: &_aterm,
        multi_action: &_aterm,
        mut callback: F,
    ) where
        F: FnMut(&[*const _aterm], *const _aterm),
    {
        /// Trampoline that casts the context pointer back to the closure and calls it.
        fn trampoline(context: *mut u8, values: &[*const _aterm], multi_action: *const _aterm) {
            // SAFETY: `context` points to a live `&mut dyn FnMut(...)` set by the caller below.
            let callback = unsafe { &mut *(context as *mut &mut dyn FnMut(&[*const _aterm], *const _aterm)) };

            // The callback runs across the C++/Rust FFI boundary. A panic
            // unwinding into the C++ enumerator frame is undefined behaviour, so
            // contain it here and abort cleanly instead. (Errors are reported
            // out-of-band by the callers, so a panic here is always a bug.)
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(values, multi_action))).is_err() {
                eprintln!("panic in enumeration callback; aborting to avoid unwinding into C++");
                std::process::abort();
            }
        }

        // Unsized coercion: &mut F → &mut dyn FnMut(...) produces a fat pointer (data + vtable).
        // Must be a named binding so its stack address is stable for the *mut u8 cast below;
        // an inline temporary would be dropped before the unsafe block, dangling the raw pointer.
        // NLL sees this borrow of the local `callback` as disjoint from the `self.context` borrow below.
        let mut callback_ref: &mut dyn FnMut(&[*const _aterm], *const _aterm) = &mut callback;
        let mut context = self.context.borrow_mut();
        // SAFETY: `callback_ref` outlives the call (it is a stack local borrowed
        // for the duration), and `trampoline` reconstructs exactly the
        // `&mut dyn FnMut` written here from the `*mut u8` we pass.
        unsafe {
            mcrl2_lps_enumerate(
                context.as_mut().expect("The context is always defined"),
                condition,
                summation_variables,
                assignments,
                multi_action,
                &mut callback_ref as *mut _ as *mut u8,
                trampoline,
            );
        }
    }
}
