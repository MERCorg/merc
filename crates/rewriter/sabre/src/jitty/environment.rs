use merc_aterm::Protected;
use merc_data::DataExpressionRef;

use crate::rewrite_machine::TermStackId;
use crate::subject::subterm;
use crate::utilities::DataPosition;

/// The index of an environment in [Environments].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EnvId(u32);

/// A term stack that is not evaluated yet, with the environment that its variables refer to. It
/// holds a reference to that environment.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Closure {
    pub term_stack: TermStackId,
    pub env: EnvId,
}

/// What the variables of a rule stand for, while parts of its right-hand side may still have to
/// be evaluated: the arguments of the matched term. Only a right-hand side with hooks has one.
///
/// An argument is a term, either a normal form or a subterm that is not rewritten yet, or a
/// [Closure] when it is a part of an earlier right-hand side that has not been evaluated.
///
/// For example, take `f(b, x) = if(b, g(x), 0)` and `if(true, x, y) = x`, with `if` lazy in its
/// branches. The term stacks `T1` and `T2` of the right-hand side of `f` build `g(x)` and `0`, and
/// rewriting `f(p, t)` with `p` rewriting to `true` creates
///
/// ```text
/// E1 = [p, t]
/// E2 = [true, (T1, E1), (T2, E1)]
/// ```
///
/// for `f` and `if` respectively. Evaluating the right-hand side `x` of `if` in E2 evaluates `T1`
/// in E1, which rewrites `t` and matches `g` on the result. `T2` is never evaluated, so `0` is
/// never built.
///
/// An environment is freed when the configuration that evaluates its right-hand side and the
/// closures over it have released it. Arguments share one protected vector, whose slots are reused
/// by environments of the same arity.
#[derive(Default)]
pub(crate) struct Environments {
    /// The arguments of all environments. An argument is `None` when it is a closure.
    arguments: Protected<Vec<Option<DataExpressionRef<'static>>>>,
    entries: Vec<Entry>,
    /// The entries that are not in use.
    free: Vec<u32>,
    /// The starts of the argument slots that are not in use, by arity.
    free_arguments: Vec<Vec<usize>>,
}

struct Entry {
    references: u32,
    start: usize,
    arity: usize,
    /// The closures of the arguments that are one.
    closures: Vec<Option<Closure>>,
}

impl Environments {
    /// Creates an environment with one reference, whose argument `i` is given by `arguments(i)`.
    pub fn create<'a, F>(&mut self, arity: usize, mut arguments: F) -> EnvId
    where
        F: FnMut(usize) -> Argument<'a>,
    {
        let start = match self.free_arguments.get_mut(arity).and_then(|free| free.pop()) {
            Some(start) => start,
            None => {
                let start = self.arguments.read().len();
                self.arguments.write(|write| write.resize(start + arity, None));
                start
            }
        };

        let mut closures: Vec<Option<Closure>> = Vec::new();
        let entries = &mut self.entries;
        self.arguments.write(|write| {
            for index in 0..arity {
                let closure = match arguments(index) {
                    Argument::Term(term) => {
                        // Safety: term is stored in the container on the same line.
                        write[start + index] = Some(unsafe { write.protect(&term) }.into());
                        continue;
                    }
                    Argument::Closure(closure) => closure,
                    Argument::Forward(env, argument) => {
                        let entry = &entries[env.0 as usize];
                        match entry.closures.get(argument).copied().flatten() {
                            Some(closure) => closure,
                            None => {
                                let term = write[entry.start + argument]
                                    .as_ref()
                                    .expect("an argument without a closure is a term");
                                // Safety: term is stored in the container on the next line.
                                let term = unsafe { write.protect(term) };
                                write[start + index] = Some(term.into());
                                continue;
                            }
                        }
                    }
                };

                entries[closure.env.0 as usize].references += 1;
                if closures.is_empty() {
                    closures.resize(arity, None);
                }
                closures[index] = Some(closure);
            }
        });

        let entry = Entry {
            references: 1,
            start,
            arity,
            closures,
        };
        match self.free.pop() {
            Some(id) => {
                self.entries[id as usize] = entry;
                EnvId(id)
            }
            None => {
                self.entries.push(entry);
                EnvId(u32::try_from(self.entries.len() - 1).expect("fewer than 2^32 environments"))
            }
        }
    }

    /// Adds a reference to `env`.
    pub fn retain(&mut self, env: EnvId) {
        self.entries[env.0 as usize].references += 1;
    }

    /// Removes a reference to `env`, and frees it and the environments of its closures that no
    /// longer have one.
    pub fn release(&mut self, env: EnvId) {
        // Only freeing an environment with closures allocates.
        let mut pending = Vec::new();
        let mut next = Some(env);
        while let Some(env) = next.take().or_else(|| pending.pop()) {
            let entry = &mut self.entries[env.0 as usize];
            entry.references -= 1;
            if entry.references > 0 {
                continue;
            }

            pending.extend(entry.closures.drain(..).flatten().map(|closure| closure.env));
            let (start, arity) = (entry.start, entry.arity);
            self.arguments.write(|write| write[start..start + arity].fill(None));
            if self.free_arguments.len() <= arity {
                self.free_arguments.resize_with(arity + 1, Vec::new);
            }
            self.free_arguments[arity].push(start);
            self.free.push(env.0);
        }
    }

    /// Returns the closure of argument `index` of `env`, if it is one.
    pub fn closure(&self, env: EnvId, index: usize) -> Option<Closure> {
        self.entries[env.0 as usize].closures.get(index).copied().flatten()
    }

    /// Calls `f` with argument `index` of `env`, which must be a term.
    pub fn with_argument<R>(&self, env: EnvId, index: usize, f: impl FnOnce(&DataExpressionRef<'_>) -> R) -> R {
        let entry = &self.entries[env.0 as usize];
        let arguments = self.arguments.read();
        f(arguments[entry.start + index]
            .as_ref()
            .expect("an argument without a closure is a term"))
    }

    /// Calls `f` with the subterm at `position` of the arguments of `env`.
    pub fn with_subterm<R>(
        &self,
        env: EnvId,
        position: &DataPosition,
        f: impl FnOnce(&DataExpressionRef<'_>) -> R,
    ) -> R {
        let entry = &self.entries[env.0 as usize];
        let arguments = self.arguments.read();
        f(&subterm(&arguments[entry.start..entry.start + entry.arity], position))
    }
}

/// What an argument of an environment stands for, see [Environments::create].
pub(crate) enum Argument<'a> {
    Term(DataExpressionRef<'a>),
    /// A closure, to which the environment adds a reference.
    Closure(Closure),
    /// The same value as the given argument of another environment.
    Forward(EnvId, usize),
}
