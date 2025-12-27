# Technical Debt Analysis

This document identifies and tracks technical debt within the MERC codebase. Technical debt items are categorized by severity and impact.

## 1. Code Quality Issues

### 1.1 Clippy Warnings

**Severity:** Medium  
**Impact:** Code maintainability and consistency

#### let_unit_value warnings
- **Location:** `crates/syntax/tests/example_test.rs` (lines 220, 385, 482, 498)
- **Location:** `crates/syntax/tests/grammar_test.rs` (lines 76, 96)
- **Location:** `crates/io/src/bitstream.rs` (line 181)
- **Location:** `crates/utilities/src/protection_set.rs` (line 286)
- **Location:** `crates/aterm/src/aterm_int.rs` (line 82)
- **Description:** Test files use `let _ = test_logger();` which binds a unit value unnecessarily
- **Recommendation:** Replace with direct function call `test_logger();`
- **Status:** ✅ Fixed

### 1.2 TODO Comments

**Severity:** Low to High (varies by item)  
**Impact:** Incomplete implementations and potential bugs

The codebase contains 30+ TODO comments indicating incomplete implementations or areas requiring improvement:

#### High Priority TODOs

1. **Drop Function Not Called**
   - **Location:** `crates/utilities/src/compressed_vec.rs:16`
   - **Description:** `TODO: The drop() function of T is never called`
   - **Impact:** Memory leak potential for types requiring cleanup
   - **Status:** ❌ Not Fixed

2. **Potential Race Condition**
   - **Location:** `crates/unsafety/src/stable_pointer_set.rs`
   - **Description:** `TODO: I suppose this can go wrong with begin_insert(x); insert(x); remove(x); end_insert(x) chain`
   - **Impact:** Possible data corruption or undefined behavior
   - **Status:** ❌ Not Fixed

3. **Substitution Clear Violation**
   - **Location:** `crates/sabre/src/utilities/data_substitution.rs`
   - **Description:** `TODO: When write is dropped we check whether all terms where inserted, but this clear violates that assumption`
   - **Impact:** Potential invariant violation
   - **Status:** ❌ Not Fixed

#### Medium Priority TODOs

1. **API Design Issues**
   - **Location:** `tools/mcrl2/crates/mcrl2/src/pbes.rs`
   - **Description:** `TODO: This should probably be private`
   - **Status:** ❌ Not Fixed

2. **Optimization Opportunities**
   - **Location:** `crates/lts/src/incoming_transitions.rs`
   - **Description:** `TODO: This could be more efficient by simply grouping them instead of sorting`
   - **Status:** ❌ Not Fixed

3. **Naive Implementations**
   - **Location:** `tools/mcrl2/pbes/src/symmetry.rs`
   - **Description:** `TODO: Fix naive implementation`
   - **Status:** ❌ Not Fixed

#### Low Priority TODOs

1. **Missing Features**
   - **Location:** `crates/aterm/src/parse_term.rs`
   - **Description:** `TODO: Parse integer terms and aterm list as in the old toolset`
   - **Status:** ❌ Not Fixed

2. **Implementation Clarity**
   - **Location:** `crates/syntax/src/syntax_tree.rs`
   - **Description:** `TODO: What should this be called?`
   - **Status:** ❌ Not Fixed

See full list in appendix below.

### 1.3 Unsafe Code Usage

**Severity:** High  
**Impact:** Memory safety and program correctness

- **Total unsafe blocks:** 286 occurrences across the codebase
- **Primary locations:**
  - `crates/unsafety/src/stable_pointer_set.rs` (869 lines)
  - `crates/aterm/src/aterm.rs` (658 lines)
  - `crates/aterm/src/thread_aterm_pool.rs` (537 lines)
  - `crates/aterm/src/global_aterm_pool.rs` (510 lines)
- **Mitigation:** All unsafe code is tested under sanitizers (AddressSanitizer, ThreadSanitizer) and miri
- **Status:** ✅ Monitored (extensive testing in place)

### 1.4 Error Handling

**Severity:** Medium  
**Impact:** Program robustness

- **unwrap() calls:** 193 occurrences in `crates/` directory
- **Recommendation:** Replace with proper error handling using `Result` or `expect()` with descriptive messages
- **Status:** ❌ Not Fixed

## 2. Code Structure Issues

### 2.1 Large Files

**Severity:** Medium  
**Impact:** Code maintainability and review difficulty

Files exceeding 500 lines:

1. `crates/syntax/src/consume.rs` - 1,356 lines
2. `crates/unsafety/src/stable_pointer_set.rs` - 869 lines
3. `crates/ldd/src/operations.rs` - 806 lines
4. `crates/vpg/src/variability_zielonka.rs` - 731 lines
5. `crates/syntax/src/syntax_tree_display.rs` - 725 lines
6. `crates/syntax/src/precedence.rs` - 665 lines
7. `crates/aterm/src/aterm.rs` - 658 lines
8. `crates/syntax/src/syntax_tree.rs` - 655 lines
9. `crates/reduction/src/signature_refinement.rs` - 641 lines
10. `crates/aterm/src/aterm_binary_stream.rs` - 629 lines

**Recommendation:** Consider splitting large files into smaller, more focused modules
**Status:** ❌ Not Fixed

### 2.2 Code Duplication

**Severity:** Low  
**Impact:** Maintenance burden

- **clone() calls:** 242 occurrences
- **Recommendation:** Review for unnecessary clones, consider using references or `Cow` where appropriate
- **Status:** ❌ Not Fixed

## 3. Dependency Management

### 3.1 Duplicate Dependencies

**Severity:** Low  
**Impact:** Binary size and compilation time

- **allocator-api2:** Two versions (v0.2.21 and v0.4.0)
  - v0.2.21 used by hashbrown v0.16.1
  - v0.4.0 used directly by merc_unsafety
- **Recommendation:** Consolidate to single version if possible
- **Status:** ❌ Not Fixed

### 3.2 Documentation Collisions

**Severity:** Low  
**Impact:** Documentation generation

- Output filename collision at `/target/doc/merc_lts/index.html`
- Output filename collision at `/target/doc/merc_vpg/index.html`
- **Cause:** Tool crates in `tools/` have same names as library crates in `crates/`
- **Recommendation:** Rename tool crates to avoid collision (e.g., `merc-lts-tool`, `merc-vpg-tool`)
- **Status:** ❌ Not Fixed

## 4. Testing and Documentation

### 4.1 Test Coverage

**Current State:**
- **Test files:** 68 files with `#[cfg(test)]`
- **Testing infrastructure:** Good use of property-based testing (arbtest), criterion benchmarks
- **Status:** ✅ Good coverage

### 4.2 Documentation

**Issues:**
- Missing documentation warnings in cargo doc output
- TODO comments in public API documentation
- **Recommendation:** Add comprehensive documentation for public APIs
- **Status:** ❌ Partial

## 5. Build and Tooling

### 5.1 Build Configuration

**Current State:**
- Rust edition: 2024
- Minimum rustc version: 1.85.0
- LTO enabled in release builds
- **Status:** ✅ Well configured

### 5.2 Formatting

**Current State:**
- Uses nightly rustfmt with custom configuration
- Formatting must be run before commit
- **Status:** ✅ Good

## Recommendations Priority

### High Priority (Fix Soon)
1. ❌ Fix drop() not being called in ByteCompressedVec
2. ❌ Investigate and fix race condition in StablePointerSet
3. ❌ Address substitution clear violation in sabre utilities
4. ✅ Fix clippy warnings (let_unit_value)

### Medium Priority (Plan for Improvement)
5. ❌ Reduce unwrap() usage with better error handling
6. ❌ Refactor large files (>800 lines) into smaller modules
7. ❌ Review and optimize naive implementations
8. ❌ Fix documentation collisions

### Low Priority (Technical Improvement)
9. ❌ Consolidate duplicate dependencies
10. ❌ Review clone() usage for optimization opportunities
11. ❌ Complete TODO items for missing features
12. ❌ Improve API naming and visibility

## Appendix: Complete TODO List

### All TODO Comments by File

```
3rd-party/pest_consume_macros/src/make_parser.rs:
- TODO: return a proper error?

tools/mcrl2/crates/mcrl2/src/atermpp/global_aterm_pool.rs:
- TODO: use existing free spots

tools/mcrl2/crates/mcrl2/src/atermpp/thread_aterm_pool.rs:
- TODO: This will always print, but only depends on aterm_configuration.h

tools/mcrl2/crates/mcrl2/src/pbes.rs:
- TODO: This should probably be private

tools/mcrl2/pbes/src/symmetry.rs:
- TODO: This is not optimal since we are not interested in the outgoing edges
- TODO: used is used_for and used_in in the theory (should be split)
- TODO: Fix naive implementation

tools/mcrl2/pbes/src/clone_iterator.rs:
- TODO: This I don't understand fully, but it works

crates/sabre/src/utilities/innermost_stack.rs:
- TODO: This ignores the first element of the stack

crates/sabre/src/utilities/term_stack.rs:
- TODO: It would make sense if Protected could implement Clone

crates/sabre/src/utilities/data_substitution.rs:
- TODO: When write is dropped we check whether all terms where inserted

crates/sabre/src/utilities/substitution.rs:
- TODO: When write is dropped we check whether all terms where inserted

crates/syntax/src/syntax_tree.rs:
- TODO: What should this be called?

crates/utilities/src/protection_set.rs:
- TODO: Is it possible to get the size of entries down to sizeof(NonZero<usize>)?

crates/utilities/src/generational_index.rs:
- TODO: Should we have a default index?

crates/utilities/src/compressed_vec.rs:
- TODO: The drop() function of T is never called

crates/data/src/data_specification.rs:
- TODO: Not yet useful, but can be used to read from binary stream

crates/data/src/data_expression.rs:
- TODO: Storing terms temporarily is not optimal (2 occurrences)

crates/sharedmutex/src/bf_sharedmutex.rs:
- TODO: Maybe use pin to share the control bits

crates/ldd/src/storage/ldd.rs:
- TODO: This function should only be called by Storage and [Ldd]

crates/lts/src/io_lts.rs:
- TODO: This should consider multi-actions properly

crates/lts/src/incoming_transitions.rs:
- TODO: This could be more efficient

crates/reduction/src/indexed_partition.rs:
- TODO: This assumes that the blocks are dense

crates/unsafety/src/stable_pointer_set.rs:
- TODO: I suppose this can go wrong with race conditions

crates/aterm/src/symbol_pool.rs:
- TODO: Not optimal

crates/aterm/src/symbol.rs:
- TODO: How to actually hide this implementation?

crates/aterm/src/aterm.rs:
- TODO: Why is this necessary

crates/aterm/src/parse_term.rs:
- TODO: Parse integer terms and aterm list as in old toolset

crates/aterm/src/aterm_list.rs:
- TODO: This should use the trait Term<'a, 'b>
```

## Conclusion

The MERC codebase demonstrates good software engineering practices overall with:
- Strong testing infrastructure (property-based testing, benchmarks, sanitizers)
- Well-configured build system
- Clear code formatting standards

However, there are several areas requiring attention:
1. **Immediate fixes needed:** Clippy warnings, potential memory leaks, race conditions
2. **Architectural improvements:** Large file refactoring, API visibility
3. **Code quality:** Better error handling, TODO completion

The extensive use of unsafe code is well-justified for the low-level data structures and is properly tested. The TODO comments provide a good roadmap for future improvements.

---

**Document Version:** 1.0  
**Last Updated:** 2025-12-27  
**Maintainer:** MERC Development Team
