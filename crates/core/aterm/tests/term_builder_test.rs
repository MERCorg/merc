//! Regression tests for defects in `aterm_builder.rs` (`TermBuilder` / `apply`).

use merc_aterm::ATerm;
use merc_aterm::ATermInt;
use merc_aterm::Symb;
use merc_aterm::Symbol;
use merc_aterm::Term;
use merc_aterm::TermBuilder;
use merc_aterm::Yield;
use merc_aterm::apply;
use merc_aterm::storage::THREAD_TERM_POOL;
use merc_aterm::storage::ThreadTermPool;

/// `apply` documents that a subterm for which the function returns `None` is
/// kept. Due to maximal sharing this means that a function returning `None`
/// everywhere must be the identity, including for integer subterms, whose
/// value lives in the term's annotation rather than in its symbol or
/// arguments.
#[test]
fn test_apply_identity_keeps_int_terms() {
    let int: ATerm = ATermInt::new(42).into();
    let t = ATerm::with_args(&Symbol::new("f_builder_test", 1), &[int.copy()]).protect();

    let result = THREAD_TERM_POOL.with(|tp| apply(tp, &t, &|_: &ThreadTermPool, _: &ATerm| None));

    assert_eq!(
        result, t,
        "apply without substitutions must keep every subterm, including integer terms"
    );
}

/// A failed evaluation must not poison the builder: a subsequent `evaluate`
/// on the same builder has to produce the correct result.
#[test]
fn test_builder_usable_after_error() {
    THREAD_TERM_POOL.with(|tp| {
        let mut builder = TermBuilder::<ATerm, Symbol>::new();

        let a = ATerm::constant(&Symbol::new("a_builder_test", 0));
        let b = ATerm::constant(&Symbol::new("b_builder_test", 0));
        let f_ab = ATerm::with_args(&Symbol::new("f_builder_test", 2), &[a.copy(), b.copy()]).protect();

        // The first evaluation fails when it reaches subterm `a`.
        let result = builder.evaluate(
            tp,
            f_ab.clone(),
            |_tp, args, t| {
                if t == a {
                    Err(merc_utilities::MercError::from("transformer rejects a"))
                } else if t.get_head_symbol().arity() == 0 {
                    Ok(Yield::Term(t))
                } else {
                    for arg in t.arguments() {
                        args.push(arg.protect());
                    }
                    Ok(Yield::Construct(t.get_head_symbol().protect()))
                }
            },
            |tp, symbol, args| Ok(tp.create_term_iter(&symbol, args)),
        );
        assert!(result.is_err(), "the first evaluation should fail");

        // Reusing the builder must give the correct result for a fresh input.
        let c = ATerm::constant(&Symbol::new("c_builder_test", 0));
        let result = builder
            .evaluate(
                tp,
                c.clone(),
                |_tp, _args, t| Ok(Yield::Term(t)),
                |tp, symbol, args| Ok(tp.create_term_iter(&symbol, args)),
            )
            .expect("the second evaluation should succeed");

        assert_eq!(
            result, c,
            "a builder reused after an error must produce the correct term"
        );
    });
}
