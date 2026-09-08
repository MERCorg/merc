use std::collections::HashMap;
use std::collections::VecDeque;
use std::collections::hash_map::Entry;

use merc_utilities::MercError;

use crate::graph_symmetry::GapConfig;
use crate::graph_symmetry::run_gap;
use crate::permutation::Permutation;

/// A permutation stored as a dense image vector: `images[i] = π(i)`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DensePermutation {
    images: Vec<usize>,
}

impl DensePermutation {
    pub(crate) fn identity(n: usize) -> Self {
        DensePermutation {
            images: (0..n).collect(),
        }
    }

    pub(crate) fn from_permutation(perm: &Permutation, n: usize) -> Self {
        DensePermutation {
            images: (0..n).map(|i| perm.value(i)).collect(),
        }
    }

    /// `self.apply(x)` returns `π(x)`.
    pub(crate) fn apply(&self, x: usize) -> usize {
        self.images[x]
    }

    /// Returns the result of permuting the *positions* of `v`, i.e.
    /// `result[π(i)] = v[i]`  ↔  `result[i] = v[π⁻¹(i)]`.
    #[cfg(test)]
    pub(crate) fn apply_to_vec(&self, v: &[usize]) -> Vec<usize> {
        debug_assert_eq!(
            v.len(),
            self.images.len(),
            "permutation degree must match vector length"
        );
        let n = v.len();
        let mut result = vec![0usize; n];
        for i in 0..n {
            result[self.images[i]] = v[i];
        }
        result
    }

    /// Compose: returns the permutation `α ∘ β` where `β` is applied first.
    /// `compose(α, β).apply(x) == α.apply(β.apply(x))`.
    pub(crate) fn compose(alpha: &DensePermutation, beta: &DensePermutation) -> Self {
        debug_assert_eq!(alpha.images.len(), beta.images.len());
        let images = beta.images.iter().map(|&x| alpha.apply(x)).collect();
        DensePermutation { images }
    }

    pub(crate) fn inverse(&self) -> Self {
        let n = self.images.len();
        let mut inv = vec![0usize; n];
        for (i, &img) in self.images.iter().enumerate() {
            inv[img] = i;
        }
        DensePermutation { images: inv }
    }

    pub(crate) fn len(&self) -> usize {
        self.images.len()
    }
}

/// One level of a stabilizer chain.
pub(crate) struct SchreierLevel {
    /// The point β fixed by the stabilizer at this level.
    pub(crate) base_point: usize,

    /// The coset representatives as `(x, u)` pairs, where `x` is an orbit point
    /// and `u` the unique representative with `u(base_point) == x`.
    ///
    /// A flat vector rather than a map because canonicalization only ever scans
    /// it in order, and needs `x` alongside `u` to score a candidate.
    pub(crate) transversal: Vec<(usize, DensePermutation)>,
}

impl SchreierLevel {
    /// Builds a level from a transversal keyed on orbit point.
    fn new(base_point: usize, transversal: HashMap<usize, DensePermutation>) -> Self {
        SchreierLevel {
            base_point,
            transversal: transversal.into_iter().collect(),
        }
    }
}

/// Reusable buffers owned by the caller, so that canonicalizing a state
/// allocates nothing.
#[derive(Default)]
pub(crate) struct CanonicalizeContext {
    /// The surviving candidate prefixes after the positions decided so far,
    /// concatenated as image vectors: candidate `k` is `current[k * n..(k + 1) * n]`.
    current: Vec<usize>,

    /// The same arena for the survivors of the position being decided; swapped
    /// with `current` after each position.
    next: Vec<usize>,

    /// Indices `(candidate, transversal slot)` of the pairs that achieve the
    /// best value at a base point. Unused when pruning a non-base position,
    /// since there `current` is filtered directly into `next`.
    survivors: Vec<(usize, usize)>,
}

/// A base and strong generating set, stored as a stabilizer chain.
pub struct Bsgs {
    /// Degree of the permutation group; every point is in `0..n`.
    pub(crate) n: usize,

    /// The stabilizer chain, outermost level first. Empty for the trivial group.
    pub(crate) chain: Vec<SchreierLevel>,
}

impl Bsgs {
    /// Build a BSGS by invoking GAP with the `ExplicitBSGS` script.
    /// Falls back to the local Schreier–Sims construction if GAP is not available.
    pub fn from_generators(gens: &[Permutation], n: usize, config: &GapConfig) -> Result<Self, MercError> {
        if gens.is_empty() {
            return Ok(Bsgs { n, chain: vec![] });
        }

        match bsgs_from_gap(gens, n, config) {
            Ok(bsgs) => Ok(bsgs),
            Err(gap_err) => {
                log::warn!("GAP not available for BSGS ({gap_err}); falling back to local Schreier–Sims");
                bsgs_schreier_sims(gens, n)
            }
        }
    }

    /// Product of transversal sizes — equals |G|.
    pub fn order(&self) -> u128 {
        self.chain.iter().map(|l| l.transversal.len() as u128).product()
    }

    /// BFS over the vector orbit; O(|orbit| · n) per call.
    ///
    /// Kept as the brute-force oracle that [`Bsgs::canonicalize`] is tested
    /// against; the tool itself always uses the pruned walk.
    #[cfg(test)]
    pub(crate) fn canonicalize_naive(&self, state: &[usize], param_offset: usize) -> Vec<usize> {
        use std::collections::HashSet;

        if self.chain.is_empty() {
            return state.to_vec();
        }

        let params: Vec<usize> = state[param_offset..].to_vec();
        let n = self.n;

        // Collect all generators as DensePerms acting on the param slice.
        let generators = self.all_generators();

        let mut visited: HashSet<Vec<usize>> = HashSet::new();
        let mut queue: VecDeque<Vec<usize>> = VecDeque::new();
        let mut min_params = params.clone();

        visited.insert(params.clone());
        queue.push_back(params);

        while let Some(current) = queue.pop_front() {
            if current < min_params {
                min_params = current.clone();
            }

            for g in &generators {
                debug_assert_eq!(g.len(), n);
                let next = g.apply_to_vec(&current);
                if visited.insert(next.clone()) {
                    queue.push_back(next);
                }
            }
        }

        let _ = n; // suppress unused warning when debug_assert is off
        let mut result = state[..param_offset].to_vec();
        result.extend_from_slice(&min_params);
        result
    }

    /// Returns the lex-min image of `state` under the group.
    ///
    /// `param_offset` is the index of the first parameter in `state`; positions
    /// `0..param_offset` (the equation index) are never permuted. The parameter
    /// block `state[param_offset..]` must have exactly [`Bsgs::n`] entries, so
    /// callers must not pass states that lack a full parameter block.
    ///
    /// # Details
    ///
    /// Pruned transversal walk over the stabilizer chain, deciding one image
    /// position at a time in index order `j = 0, …, n-1`. Writing the image as
    /// `w[j] = params[σ(j)]` for `σ ∈ G`, each `σ` factorises uniquely as
    /// `σ = u_1 ∘ … ∘ u_k` with `u_i ∈ U_i`. Every factor beyond level `i` lies in
    /// the stabilizer of `β_i`, so `σ(β_i) = (u_1 ∘ … ∘ u_i)(β_i)` is already fixed
    /// once the level-`i` choice is made. That is what makes the greedy sound: at
    /// level `i` we can keep only the prefixes minimising `w[β_i]` and discard the
    /// rest, because no later choice can change that entry.
    ///
    /// A position `j` the base skips is fixed by the stabilizer at that depth, so
    /// it is invariant across every way of *extending* one fixed prefix — but not
    /// across prefixes that only tied at an earlier level via different coset
    /// representatives, since those genuinely differ on `j`. So `j` still has to
    /// be scored and pruned on before moving to the next base point; it's just
    /// pruned by evaluating `params[prefix[j]]` directly rather than searching a
    /// transversal, since no factor remains to choose there.
    ///
    /// Cost: Σ |U_i| transversal scans, one comparison per surviving candidate for
    /// each skipped position, and one composition per surviving candidate per
    /// level — versus |orbit|·n for the naive BFS.
    #[allow(dead_code)]
    pub(crate) fn canonicalize(&self, state: &[usize], param_offset: usize) -> Vec<usize> {
        let mut out = Vec::with_capacity(state.len());
        self.canonicalize_into(state, param_offset, &mut CanonicalizeContext::default(), &mut out);
        out
    }

    /// [`Bsgs::canonicalize`] writing into `out` and borrowing `scratch`, so that
    /// a call costs no allocation. Both are cleared first; reuse them across calls.
    pub(crate) fn canonicalize_into(
        &self,
        state: &[usize],
        param_offset: usize,
        scratch: &mut CanonicalizeContext,
        out: &mut Vec<usize>,
    ) {
        out.clear();
        if self.chain.is_empty() {
            out.extend_from_slice(state);
            return;
        }

        let n = self.n;
        let params: &[usize] = &state[param_offset..];
        debug_assert_eq!(
            params.len(),
            n,
            "canonicalize requires a full parameter block; sinks and subformula states have none"
        );
        debug_assert!(
            self.chain.windows(2).all(|w| w[0].base_point < w[1].base_point),
            "the greedy relies on a strictly increasing base"
        );

        let CanonicalizeContext {
            current,
            next,
            survivors,
        } = scratch;

        // Candidate prefixes `u_1 ∘ … ∘ u_i` that all achieve the lex-min entries
        // at every position decided so far, as concatenated image vectors.
        current.clear();
        current.extend(0..n); // the identity, the empty product

        let mut levels = self.chain.iter().peekable();
        for j in 0..n {
            if levels.peek().is_some_and(|level| level.base_point == j) {
                let level = levels.next().expect("just peeked");

                // Score every extension without building it. Extending on the
                // right gives `(prefix ∘ u)(β_i) = prefix(u(β_i))`, and `u(β_i)`
                // is exactly the orbit point the transversal entry is paired
                // with, so the image entry `w[β_i] = params[(prefix ∘ u)(β_i)]`
                // is one lookup away.
                survivors.clear();
                let mut best_value = usize::MAX;
                for (p, prefix) in current.chunks_exact(n).enumerate() {
                    for (u_idx, &(orbit_point, _)) in level.transversal.iter().enumerate() {
                        let value = params[prefix[orbit_point]];

                        if value < best_value {
                            best_value = value;
                            survivors.clear();
                            survivors.push((p, u_idx));
                        } else if value == best_value {
                            survivors.push((p, u_idx));
                        }
                    }
                }

                // Only now compose, and only for the extensions that survived.
                next.clear();
                for &(p, u_idx) in survivors.iter() {
                    let prefix = &current[p * n..(p + 1) * n];
                    let u = &level.transversal[u_idx].1;
                    next.extend(u.images.iter().map(|&x| prefix[x]));
                }
            } else {
                // `j` isn't a base point: no transversal choice remains to make
                // here, but surviving prefixes can still disagree on `params[prefix[j]]`,
                // so prune on it directly rather than skipping straight to the
                // next base point.
                next.clear();
                let mut best_value = usize::MAX;
                for prefix in current.chunks_exact(n) {
                    let value = params[prefix[j]];

                    if value < best_value {
                        best_value = value;
                        next.clear();
                        next.extend_from_slice(prefix);
                    } else if value == best_value {
                        next.extend_from_slice(prefix);
                    }
                }
            }

            std::mem::swap(current, next);
        }

        // Every position has now been scored and pruned on in index order, so
        // all surviving candidates already agree on the full image; any one of
        // them is the lex-min representative.
        out.extend_from_slice(&state[..param_offset]);
        out.extend(current[..n].iter().map(|&i| params[i]));
    }

    /// Flat list of all generators appearing across all levels.
    #[cfg(test)]
    fn all_generators(&self) -> Vec<DensePermutation> {
        use std::collections::HashSet;

        let mut gens: HashSet<DensePermutation> = HashSet::new();
        for level in &self.chain {
            for (_, u) in &level.transversal {
                gens.insert(u.clone());
                gens.insert(u.inverse());
            }
        }
        gens.into_iter().collect()
    }
}

/// GAP script template: renders generators in GAP cycle notation and calls
/// `ExplicitBSGS`, then prints the result to stdout.
fn build_gap_bsgs_script(gens: &[Permutation], n: usize) -> String {
    let gen_strs: Vec<String> = gens.iter().map(|p| permutation_to_gap_cycles(p, n)).collect();
    let gens_joined = gen_strs.join(", ");

    // Build a comma-separated base list "1,2,...,n".
    let base: String = (1..=n).map(|i| i.to_string()).collect::<Vec<_>>().join(",");

    format!(
        r#"
ExplicitBSGS := function(G, chain)
    local b, level, rest;
    b := chain.orbit[1];
    level := rec(
        basepoint := b,
        transversal := List(chain.orbit, x -> RepresentativeAction(G, b, x))
    );
    if IsBound(chain.stabilizer) and Length(chain.stabilizer.generators) > 0 then
        rest := ExplicitBSGS(Stabilizer(G, b), chain.stabilizer);
        return Concatenation([level], rest);
    else
        return [level];
    fi;
end;

G := Group({gens});
base := [{base}];
chain := StabChain(G, base);
bsgs := ExplicitBSGS(G, chain);
Print("BSGS-BEGIN\n");
Print(bsgs, "\n");
Print("BSGS-END\n");
quit;
"#,
        gens = gens_joined,
        base = base,
    )
}

/// Render a `Permutation` as a GAP cycle string using 1-based indices.
/// The identity is rendered as `()`.
pub fn permutation_to_gap_cycles(perm: &Permutation, n: usize) -> String {
    let dense = DensePermutation::from_permutation(perm, n);
    let mut visited = vec![false; n];
    let mut result = String::new();

    for start in 0..n {
        if visited[start] || dense.apply(start) == start {
            visited[start] = true;
            continue;
        }

        result.push('(');
        let mut current = start;
        let mut first = true;

        loop {
            if !first {
                result.push(',');
            }

            first = false;
            result.push_str(&(current + 1).to_string()); // 1-based
            visited[current] = true;
            current = dense.apply(current);

            if current == start {
                break;
            }
        }
        result.push(')');
    }

    if result.is_empty() {
        result.push_str("()");
    }
    result
}

/// Invoke GAP to compute the explicit BSGS for `gens` acting on `0..n`.
pub(crate) fn bsgs_from_gap(gens: &[Permutation], n: usize, config: &GapConfig) -> Result<Bsgs, MercError> {
    let script = build_gap_bsgs_script(gens, n);
    let stdout = run_gap(&script, config)?;
    parse_gap_bsgs_output(&stdout, n)
}

/// Parse the GAP output produced by `ExplicitBSGS`.
///
/// GAP prints a list of records of the form:
/// `[ rec( basepoint := B, transversal := [ perm, ... ] ), ... ]`
///
/// Points are 1-based in GAP; we convert to 0-based.
fn parse_gap_bsgs_output(stdout: &str, n: usize) -> Result<Bsgs, MercError> {
    let begin = stdout
        .find("BSGS-BEGIN")
        .ok_or_else(|| MercError::from("GAP BSGS output missing 'BSGS-BEGIN' sentinel"))?;
    let after_begin = &stdout[begin + "BSGS-BEGIN".len()..];
    let end = after_begin
        .find("BSGS-END")
        .ok_or_else(|| MercError::from("GAP BSGS output missing 'BSGS-END' sentinel"))?;
    let inner = after_begin[..end].trim();

    // Flatten to a single string without newlines for easier parsing.
    let flat: String = inner.chars().filter(|&c| c != '\n' && c != '\r').collect();

    let chain = parse_bsgs_list(&flat, n)?;
    Ok(Bsgs { n, chain })
}

/// Parse `[ rec(...), rec(...), ... ]` into a Vec of SchreierLevel.
fn parse_bsgs_list(s: &str, n: usize) -> Result<Vec<SchreierLevel>, MercError> {
    let s = s.trim();
    // Strip outer `[ ... ]`
    let s = s
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| MercError::from(format!("Expected outer list brackets in BSGS output, got: {s}")))?
        .trim();

    let records = split_top_level(s, ',');
    let mut levels = Vec::new();
    for rec_str in records {
        let rec_str = rec_str.trim();
        if rec_str.is_empty() {
            continue;
        }
        levels.push(parse_schreier_level(rec_str, n)?);
    }
    Ok(levels)
}

/// Parse one `rec( basepoint := B, transversal := [ perm, ... ] )`.
fn parse_schreier_level(s: &str, n: usize) -> Result<SchreierLevel, MercError> {
    let inner = s
        .trim()
        .strip_prefix("rec(")
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| MercError::from(format!("Expected 'rec(...)' but got: {s}")))?
        .trim();

    // Split into fields at top-level commas.
    let fields = split_top_level(inner, ',');

    let mut base_point: Option<usize> = None;
    let mut transversal_str: Option<String> = None;

    for field in &fields {
        let field = field.trim();
        if let Some(rest) = field.strip_prefix("basepoint :=") {
            let bp: usize = rest
                .trim()
                .parse::<usize>()
                .map_err(|_| MercError::from(format!("Invalid basepoint: {rest}")))?;
            base_point = Some(bp - 1); // convert to 0-based
        } else if let Some(rest) = field.strip_prefix("transversal :=") {
            transversal_str = Some(rest.trim().to_string());
        }
    }

    let base_point = base_point.ok_or_else(|| MercError::from("Missing 'basepoint' in rec"))?;
    let transversal_str = transversal_str.ok_or_else(|| MercError::from("Missing 'transversal' in rec"))?;

    let transversal = parse_transversal_list(&transversal_str, base_point, n)?;
    Ok(SchreierLevel::new(base_point, transversal))
}

/// Parse `[ perm, perm, ... ]` where the i-th perm maps `base_point` to
/// `orbit[i]`; orbit points are determined by applying each perm to `base_point`.
fn parse_transversal_list(s: &str, base_point: usize, n: usize) -> Result<HashMap<usize, DensePermutation>, MercError> {
    let s = s
        .trim()
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| MercError::from(format!("Expected list brackets in transversal, got: {s}")))?
        .trim();

    let perm_strs = split_top_level(s, ',');
    let mut map = HashMap::new();

    for perm_str in &perm_strs {
        let perm_str = perm_str.trim();
        if perm_str.is_empty() {
            continue;
        }
        let dense = parse_gap_perm(perm_str, n)?;
        let orbit_point = dense.apply(base_point);
        map.insert(orbit_point, dense);
    }

    Ok(map)
}

/// Parse a single GAP permutation: either `()` (identity) or cycle product `(a,b,...)(c,d,...)`.
/// Points are 1-based in GAP; converts to 0-based.
fn parse_gap_perm(s: &str, n: usize) -> Result<DensePermutation, MercError> {
    let s = s.trim();
    if s == "()" {
        return Ok(DensePermutation::identity(n));
    }

    let mut images: Vec<usize> = (0..n).collect();

    // Parse each cycle `(a,b,c,...)`.
    let mut pos = 0;
    while pos < s.len() {
        let Some(open) = s[pos..].find('(') else {
            break;
        };
        let open = pos + open;
        let close = s[open..]
            .find(')')
            .ok_or_else(|| MercError::from(format!("Unclosed '(' in GAP permutation: {s}")))?
            + open;

        let content = &s[open + 1..close];
        let pts: Vec<usize> = content
            .split(',')
            .map(|t| {
                t.trim()
                    .parse::<usize>()
                    .map_err(|_| MercError::from(format!("Bad point '{t}' in cycle")))
                    .and_then(|v| {
                        v.checked_sub(1)
                            .ok_or_else(|| MercError::from("GAP point 0 is invalid (expected >= 1)"))
                    })
            })
            .collect::<Result<_, _>>()?;

        let len = pts.len();
        for i in 0..len {
            images[pts[i]] = pts[(i + 1) % len];
        }

        pos = close + 1;
    }

    Ok(DensePermutation { images })
}

/// Split `s` at top-level occurrences of `sep`, not entering `(` / `[` / `]` / `)`.
fn split_top_level(s: &str, sep: char) -> Vec<String> {
    let mut result = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();

    for ch in s.chars() {
        match ch {
            '(' | '[' => {
                depth += 1;
                current.push(ch);
            }
            ')' | ']' => {
                depth -= 1;
                current.push(ch);
            }
            c if c == sep && depth == 0 => {
                result.push(current.trim().to_string());
                current = String::new();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        result.push(current.trim().to_string());
    }
    result
}

/// Build a BSGS using the deterministic Schreier–Sims algorithm.
///
/// Base selection: choose the first non-fixed point of any generator at each level.
fn bsgs_schreier_sims(gens: &[Permutation], n: usize) -> Result<Bsgs, MercError> {
    let dense_gens: Vec<DensePermutation> = gens.iter().map(|p| DensePermutation::from_permutation(p, n)).collect();
    let chain = schreier_sims_chain(&dense_gens, n);
    Ok(Bsgs { n, chain })
}

fn schreier_sims_chain(gens: &[DensePermutation], n: usize) -> Vec<SchreierLevel> {
    if gens.is_empty() {
        return vec![];
    }

    // Pick base point: first non-fixed point of any generator.
    let base_point = (0..n).find(|&x| gens.iter().any(|g| g.apply(x) != x)).unwrap_or(0);

    // BFS to compute the orbit and transversal for `base_point`.
    let transversal = compute_orbit_transversal(gens, base_point, n);

    // Compute Schreier generators for the stabilizer Stab(base_point).
    let stab_gens = schreier_generators(gens, &transversal, base_point);

    let mut chain = vec![SchreierLevel::new(base_point, transversal)];

    if !stab_gens.is_empty() {
        let mut sub = schreier_sims_chain(&stab_gens, n);
        chain.append(&mut sub);
    }

    chain
}

/// BFS orbit + transversal for `base_point` under `gens`.
/// `transversal[x]` = coset rep mapping `base_point` to `x`.
fn compute_orbit_transversal(
    gens: &[DensePermutation],
    base_point: usize,
    n: usize,
) -> HashMap<usize, DensePermutation> {
    let _ = n;
    let mut transversal: HashMap<usize, DensePermutation> = HashMap::new();
    let mut queue: VecDeque<usize> = VecDeque::new();

    transversal.insert(base_point, DensePermutation::identity(gens[0].len()));
    queue.push_back(base_point);

    while let Some(x) = queue.pop_front() {
        let rep_x = transversal[&x].clone();
        for g in gens {
            let y = g.apply(x);
            if let Entry::Vacant(entry) = transversal.entry(y) {
                entry.insert(DensePermutation::compose(g, &rep_x));
                queue.push_back(y);
            }
        }
    }

    transversal
}

/// Compute Schreier generators for `Stab(base_point)`.
/// For each orbit point `x` and generator `s`, form `u_x · s · u_{s(x)}^{-1}`,
/// discarding the identity.
fn schreier_generators(
    gens: &[DensePermutation],
    transversal: &HashMap<usize, DensePermutation>,
    _base_point: usize,
) -> Vec<DensePermutation> {
    let mut result: Vec<DensePermutation> = Vec::new();

    for (&x, u_x) in transversal {
        for s in gens {
            let sx = s.apply(x);
            let u_sx_inv = transversal[&sx].inverse();
            // schr = u_x · s · u_sx^{-1}
            let schr = DensePermutation::compose(&u_sx_inv, &DensePermutation::compose(s, u_x));

            // Keep only non-identity Schreier generators.
            let id: Vec<usize> = (0..schr.len()).collect();
            if schr.images != id {
                result.push(schr);
            }
        }
    }

    // Deduplicate.
    result.sort_unstable_by(|a, b| a.images.cmp(&b.images));
    result.dedup();
    result
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use itertools::Itertools;
    use rand::RngExt;
    use rand::rngs::StdRng;
    use rand::seq::SliceRandom;

    use merc_utilities::random_test;

    use crate::bsgs::DensePermutation;
    use crate::bsgs::bsgs_from_gap;
    use crate::bsgs::bsgs_schreier_sims;
    use crate::bsgs::parse_gap_perm;
    use crate::bsgs::permutation_to_gap_cycles;
    use crate::graph_symmetry::GapConfig;
    use crate::graph_symmetry::run_gap;
    use crate::permutation::Permutation;

    fn s3_generators() -> (Vec<Permutation>, usize) {
        // S3 on {0,1,2}: generators (0 1) and (0 1 2).
        let swap = Permutation::from_mapping(vec![(0, 1), (1, 0)]);
        let rot = Permutation::from_mapping(vec![(0, 1), (1, 2), (2, 0)]);
        (vec![swap, rot], 3)
    }

    fn s4_generators() -> (Vec<Permutation>, usize) {
        // S4 on {0,1,2,3}: a transposition and a 4-cycle. Order 24, and unlike
        // the other two groups its stabilizer chain is three levels deep.
        let swap = Permutation::from_cycle_notation("(0 1)").unwrap();
        let cycle = Permutation::from_cycle_notation("(0 1 2 3)").unwrap();
        (vec![swap, cycle], 4)
    }

    fn alloc3_generators() -> (Vec<Permutation>, usize) {
        // From the alloc3 INFO log: generators (4 6)(5 7) and (2 4)(3 5), 0-based.
        let g1 = Permutation::from_cycle_notation("(4 6)(5 7)").unwrap();
        let g2 = Permutation::from_cycle_notation("(2 4)(3 5)").unwrap();
        (vec![g1, g2], 8)
    }

    #[test]
    fn dense_perm_compose_and_inverse() {
        let (gens, n) = s3_generators();
        let swap = DensePermutation::from_permutation(&gens[0], n);
        let rot = DensePermutation::from_permutation(&gens[1], n);

        // swap ∘ rot: first rot then swap
        let composed = DensePermutation::compose(&swap, &rot);
        // rot: 0→1→2→0; swap: 0↔1
        // 0 -rot-> 1 -swap-> 0
        // 1 -rot-> 2 -swap-> 2
        // 2 -rot-> 0 -swap-> 1
        assert_eq!(composed.images, vec![0, 2, 1]);

        let inv = swap.inverse();
        let id = DensePermutation::compose(&swap, &inv);
        assert_eq!(id.images, vec![0, 1, 2]);
    }

    #[test]
    fn dense_perm_apply_to_vec() {
        // swap (0 1) on vector [a, b, c] → [b, a, c]
        let (gens, n) = s3_generators();
        let swap = DensePermutation::from_permutation(&gens[0], n);
        let v = vec![10, 20, 30];
        // swap.apply(0)=1, swap.apply(1)=0, swap.apply(2)=2
        // result[1]=v[0]=10, result[0]=v[1]=20, result[2]=v[2]=30 → [20,10,30]
        assert_eq!(swap.apply_to_vec(&v), vec![20, 10, 30]);
    }

    #[test]
    fn schreier_sims_order_s3() {
        let (gens, n) = s3_generators();
        let bsgs = bsgs_schreier_sims(&gens, n).unwrap();
        assert_eq!(bsgs.order(), 6); // |S3| = 6
    }

    #[test]
    fn schreier_sims_order_alloc3() {
        let (gens, n) = alloc3_generators();
        let bsgs = bsgs_schreier_sims(&gens, n).unwrap();
        assert_eq!(bsgs.order(), 6);
    }

    #[test]
    fn canonicalize_stage_a_is_idempotent() {
        let (gens, n) = s3_generators();
        let bsgs = bsgs_schreier_sims(&gens, n).unwrap();

        for v in [[2usize, 0, 1], [1, 2, 0], [0, 2, 1]] {
            let state: Vec<usize> = std::iter::once(0).chain(v).collect(); // eq_idx=0
            let canon = bsgs.canonicalize_naive(&state, 1);
            let canon2 = bsgs.canonicalize_naive(&canon, 1);
            assert_eq!(canon, canon2, "Stage A not idempotent on {state:?}");
        }
    }

    #[test]
    fn canonicalize_stage_b_agrees_with_stage_a() {
        let (gens, n) = s3_generators();
        let bsgs = bsgs_schreier_sims(&gens, n).unwrap();

        // Test all 6 permutations of (10, 20, 30).
        let base = [10usize, 20, 30];
        let perms = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]];
        for perm in perms {
            let params: Vec<usize> = perm.iter().map(|&i| base[i]).collect();
            let state: Vec<usize> = std::iter::once(99).chain(params).collect();
            let a = bsgs.canonicalize_naive(&state, 1);
            let b = bsgs.canonicalize(&state, 1);
            assert_eq!(a, b, "Stage A and B disagree on {state:?}");
        }
    }

    /// Exhaustive agreement on vectors with *repeated* values.
    ///
    /// Pairwise-distinct vectors are the one class where a wrong coset
    /// factorisation still happens to land on the lex-min representative, so they
    /// hide exactly the bug this checks for.
    #[test]
    fn canonicalize_agrees_with_naive_on_repeated_values() {
        for (gens, n) in [s3_generators(), alloc3_generators()] {
            let bsgs = bsgs_schreier_sims(&gens, n).unwrap();

            // All binary vectors of length n: every value repeats.
            for mask in 0..(1u32 << n) {
                let params: Vec<usize> = (0..n).map(|i| ((mask >> i) & 1) as usize).collect();
                let state: Vec<usize> = std::iter::once(7).chain(params).collect();

                let naive = bsgs.canonicalize_naive(&state, 1);
                let pruned = bsgs.canonicalize(&state, 1);
                assert_eq!(naive, pruned, "disagreement on {state:?} (n = {n})");
            }
        }
    }

    /// Checks a case where cosets tie at a base point but disagree on a
    /// skipped (non-base) index.
    ///
    /// The Schreier–Sims base for these generators is `[0, 2, 3]`, skipping index
    /// `1` because it's fixed by the stabilizer of `0`. At base point `0`, three
    /// cosets tie for the lex-min value (reaching `params` through orbit points
    /// `2`, `4`, and `3`), but they disagree on the skipped index `1` (values
    /// `1`, `0`, `0`). [`Bsgs::canonicalize`] must prune on every position, not
    /// just the base points, or a candidate that only ties at `0` could go on
    /// to win at base point `2` before index `1` is ever compared, evicting the
    /// true minimizers. See [`Bsgs::canonicalize`]'s doc comment for why every
    /// position — not just the base — has to be pruned on.
    #[test]
    fn canonicalize_matches_naive_regression_ties_at_skipped_base_position() {
        let n = 6;
        let g1 = Permutation::from_cycle_notation("(3 4)").unwrap();
        let g2 = Permutation::from_cycle_notation("(0 2 4)(1 5 3)").unwrap();
        let bsgs = bsgs_schreier_sims(&[g1, g2], n).unwrap();

        let state = vec![7usize, 2, 2, 0, 0, 0, 1];
        let naive = bsgs.canonicalize_naive(&state, 1);
        let pruned = bsgs.canonicalize(&state, 1);

        assert_eq!(pruned, naive, "canonicalize and the BFS oracle disagree on {state:?}");
        assert_eq!(pruned, vec![7, 0, 0, 2, 0, 1, 2]);
    }

    /// Canonicalization must be constant on orbits: every image of a state has to
    /// canonicalize to the same representative, which is what makes the quotient
    /// sound.
    #[test]
    fn canonicalize_is_constant_on_orbits() {
        let (gens, n) = alloc3_generators();
        let bsgs = bsgs_schreier_sims(&gens, n).unwrap();
        let generators = bsgs.all_generators();

        for mask in 0..(1u32 << n) {
            let params: Vec<usize> = (0..n).map(|i| ((mask >> i) & 1) as usize).collect();
            let state: Vec<usize> = std::iter::once(0).chain(params.iter().copied()).collect();
            let expected = bsgs.canonicalize(&state, 1);

            for g in &generators {
                let image: Vec<usize> = std::iter::once(0).chain(g.apply_to_vec(&params)).collect();
                assert_eq!(
                    bsgs.canonicalize(&image, 1),
                    expected,
                    "state {state:?} and its image {image:?} have different representatives"
                );
            }
        }
    }

    #[test]
    fn canonicalize_is_idempotent() {
        let (gens, n) = alloc3_generators();
        let bsgs = bsgs_schreier_sims(&gens, n).unwrap();

        let state: Vec<usize> = (0..9).collect(); // eq_idx=0, params=1..8
        let canon = bsgs.canonicalize(&state, 1);
        let canon2 = bsgs.canonicalize(&canon, 1);
        assert_eq!(canon, canon2);
    }

    #[test]
    fn equation_index_untouched_by_canonicalize() {
        let (gens, n) = s3_generators();
        let bsgs = bsgs_schreier_sims(&gens, n).unwrap();

        let state = vec![42usize, 2, 0, 1];
        let canon = bsgs.canonicalize(&state, 1);
        assert_eq!(canon[0], 42, "equation index must not be permuted");
    }

    /// Returns `true` when a GAP that can run the `ExplicitBSGS` script is on the
    /// path. Cached so the probe runs at most once per test process.
    ///
    /// Unlike graph symmetry detection this needs no Digraphs package — only
    /// core GAP's `StabChain` — so it probes for plain GAP.
    fn gap_available() -> bool {
        static AVAILABLE: OnceLock<bool> = OnceLock::new();
        *AVAILABLE.get_or_init(|| {
            duct::cmd("gap", ["-q", "-A", "--quitonbreak"])
                .stdin_bytes("QUIT_GAP(0);;")
                .stdout_null()
                .stderr_null()
                .unchecked()
                .run()
                .map(|o| o.status.success())
                .unwrap_or(false)
        })
    }

    /// `Bsgs::from_generators` silently falls back to the local Schreier–Sims
    /// when GAP fails, so every other test in this module exercises the fallback
    /// even on a machine that has GAP. This one calls [`bsgs_from_gap`] directly,
    /// which is the path the tool actually takes, and checks that GAP's chain
    /// describes the same group and canonicalizes identically.
    #[test]
    fn bsgs_from_gap_agrees_with_schreier_sims() {
        if !gap_available() {
            return;
        }

        for (gens, n) in [s3_generators(), alloc3_generators()] {
            let gap = bsgs_from_gap(&gens, n, &GapConfig::default()).expect("GAP is available");
            let local = bsgs_schreier_sims(&gens, n).unwrap();

            assert_eq!(gap.order(), local.order(), "GAP and Schreier–Sims disagree on |G|");
            assert_eq!(gap.n, n);

            // A different base gives a different chain, so the chains themselves
            // need not match; what has to match is the representative each one
            // picks. Binary vectors cover the repeated values that a wrong coset
            // factorisation gets wrong.
            for mask in 0..(1u32 << n) {
                let params: Vec<usize> = (0..n).map(|i| ((mask >> i) & 1) as usize).collect();
                let state: Vec<usize> = std::iter::once(3).chain(params).collect();
                assert_eq!(
                    gap.canonicalize(&state, 1),
                    local.canonicalize(&state, 1),
                    "GAP and Schreier–Sims canonicalize {state:?} differently (n = {n})"
                );
            }
        }
    }

    /// The alloc3 generators generate a group of order 6, as GAP itself reports
    /// for that PBES (`|Sym(pbes)| = 6`).
    #[test]
    fn bsgs_from_gap_order_alloc3() {
        if !gap_available() {
            return;
        }

        let (gens, n) = alloc3_generators();
        let bsgs = bsgs_from_gap(&gens, n, &GapConfig::default()).expect("GAP is available");
        assert_eq!(bsgs.order(), 6);
    }

    /// Every vector in `{0, 1, 2}^n`, least-significant digit first.
    ///
    /// Ternary rather than binary because it covers both patterns that matter:
    /// entries that repeat, where a wrong coset factorisation shows up, and
    /// entries that differ, where the tie-breaking between levels does.
    fn ternary_vectors(n: usize) -> Vec<Vec<usize>> {
        (0..3usize.pow(n as u32))
            .map(|code| {
                let mut code = code;
                (0..n)
                    .map(|_| {
                        let digit = code % 3;
                        code /= 3;
                        digit
                    })
                    .collect()
            })
            .collect()
    }

    /// Lex-min of each state under the group generated by `gens`, as computed by
    /// GAP's `Minimum(List(Elements(G), g -> Permuted(s, g)))`.
    fn gap_lex_min(gens: &[Permutation], n: usize, states: &[Vec<usize>]) -> Vec<Vec<usize>> {
        debug_assert!(
            states.iter().all(|s| s.len() == n),
            "each state must be a full parameter block of degree n"
        );

        let gens_joined = gens.iter().map(|p| permutation_to_gap_cycles(p, n)).format(", ");
        let states_joined = states
            .iter()
            .map(|s| format!("[{}]", s.iter().format(",")))
            .format(",\n  ");

        // `Permuted(s, g)[g(i)] = s[i]`, matching `DensePermutation::apply_to_vec`.
        let script = format!(
            r#"
G := Group([{gens_joined}]);;
elems := Elements(G);;
states := [
  {states_joined}
];;
Print("LEXMIN-BEGIN\n");
for s in states do
    Print(Minimum(List(elems, g -> Permuted(s, g))), "\n");
od;
Print("LEXMIN-END\n");
quit;
"#
        );

        let stdout = run_gap(&script, &GapConfig::default()).expect("GAP is available");
        parse_gap_lex_min_output(&stdout, states.len(), n)
    }

    /// Parse the bracketed lists GAP prints between the sentinels. All whitespace
    /// is dropped first, so GAP wrapping a long list over several lines is fine.
    fn parse_gap_lex_min_output(stdout: &str, expected: usize, n: usize) -> Vec<Vec<usize>> {
        let begin = stdout
            .find("LEXMIN-BEGIN")
            .expect("GAP lex-min output missing 'LEXMIN-BEGIN' sentinel");
        let after_begin = &stdout[begin + "LEXMIN-BEGIN".len()..];
        let end = after_begin
            .find("LEXMIN-END")
            .expect("GAP lex-min output missing 'LEXMIN-END' sentinel");

        let flat: String = after_begin[..end].chars().filter(|c| !c.is_whitespace()).collect();

        let minima: Vec<Vec<usize>> = flat
            .split_terminator(']')
            .map(|chunk| {
                let inner = chunk
                    .strip_prefix('[')
                    .unwrap_or_else(|| panic!("expected a bracketed list from GAP, got: {chunk}"));
                inner
                    .split(',')
                    .map(|entry| {
                        entry
                            .parse::<usize>()
                            .unwrap_or_else(|_| panic!("expected a number from GAP, got: {entry}"))
                    })
                    .collect()
            })
            .collect();

        assert_eq!(minima.len(), expected, "GAP returned the wrong number of minima");
        assert!(
            minima.iter().all(|m| m.len() == n),
            "GAP returned a minimum of the wrong degree"
        );
        minima
    }

    /// The pruned walk claims to return the lex-least image, so check that claim
    /// against GAP rather than only against the in-crate BFS oracle.
    #[test]
    fn canonicalize_matches_gap_lex_min() {
        if !gap_available() {
            return;
        }

        for (gens, n) in [s3_generators(), s4_generators(), alloc3_generators()] {
            let states = ternary_vectors(n);
            let expected = gap_lex_min(&gens, n, &states);
            let bsgs = bsgs_schreier_sims(&gens, n).unwrap();

            for (params, expected) in states.iter().zip(&expected) {
                let state: Vec<usize> = std::iter::once(7).chain(params.iter().copied()).collect();
                let canon = bsgs.canonicalize(&state, 1);
                assert_eq!(
                    &canon[1..],
                    expected.as_slice(),
                    "canonicalize and GAP disagree on {params:?} (n = {n})"
                );
            }
        }
    }

    /// Same check for the chain GAP builds, which is the one the tool actually
    /// walks — a different base from Schreier–Sims, so a separate code path
    /// through [`Bsgs::canonicalize`].
    #[test]
    fn gap_chain_canonicalize_matches_gap_lex_min() {
        if !gap_available() {
            return;
        }

        for (gens, n) in [s3_generators(), s4_generators(), alloc3_generators()] {
            let states = ternary_vectors(n);
            let expected = gap_lex_min(&gens, n, &states);
            let bsgs = bsgs_from_gap(&gens, n, &GapConfig::default()).expect("GAP is available");

            for (params, expected) in states.iter().zip(&expected) {
                let state: Vec<usize> = std::iter::once(7).chain(params.iter().copied()).collect();
                let canon = bsgs.canonicalize(&state, 1);
                assert_eq!(
                    &canon[1..],
                    expected.as_slice(),
                    "the GAP-built chain and GAP disagree on {params:?} (n = {n})"
                );
            }
        }
    }

    /// A uniformly random permutation of degree `n`, never the identity: GAP's
    /// `StabChain` has no level to report for the trivial group, so an all-identity
    /// generator set would test the error path rather than the canonicalization.
    fn random_permutation(rng: &mut StdRng, n: usize) -> Permutation {
        debug_assert!(n >= 2, "no non-identity permutation exists below degree 2");

        loop {
            let mut image: Vec<usize> = (0..n).collect();
            image.shuffle(rng);

            let mapping: Vec<(usize, usize)> = (0..n).zip(image).filter(|(x, y)| x != y).collect();
            if !mapping.is_empty() {
                return Permutation::from_mapping(mapping);
            }
        }
    }

    /// Random states over a value range far smaller than the degree, so that
    /// entries mostly repeat — pairwise-distinct entries let a wrong coset
    /// factorisation still land on the lex-min representative, which is exactly
    /// the case that hides the bug.
    fn random_states(rng: &mut StdRng, n: usize, count: usize) -> Vec<Vec<usize>> {
        let mut states = Vec::with_capacity(count);
        for _ in 0..count {
            states.push((0..n).map(|_| rng.random_range(0..3usize)).collect());
        }
        states
    }

    /// Random groups checked against GAP for the canonicalization property.
    #[test]
    #[cfg_attr(miri, ignored)] // This uses an external tool.
    fn random_canonicalize_matches_gap_lex_min() {
        if !gap_available() {
            return;
        }

        // GAP enumerates the whole group and each iteration spawns two GAP
        // processes, so both the degree and the iteration count stay small.
        random_test(10, |rng| {
            let n = rng.random_range(3..=6usize);
            let num_gens = rng.random_range(1..=3);
            let gens: Vec<Permutation> = (0..num_gens).map(|_| random_permutation(rng, n)).collect();
            let states = random_states(rng, n, 128);

            let expected = gap_lex_min(&gens, n, &states);
            let local = bsgs_schreier_sims(&gens, n).unwrap();
            let from_gap = bsgs_from_gap(&gens, n, &GapConfig::default()).expect("GAP is available");

            assert_eq!(
                local.order(),
                from_gap.order(),
                "the two chains describe different groups for {gens:?}"
            );

            for (params, expected) in states.iter().zip(&expected) {
                let state: Vec<usize> = std::iter::once(7).chain(params.iter().copied()).collect();

                for (source, bsgs) in [("Schreier–Sims", &local), ("GAP", &from_gap)] {
                    assert_eq!(
                        &bsgs.canonicalize(&state, 1)[1..],
                        expected.as_slice(),
                        "the {source} chain disagrees with GAP on {params:?} (n = {n}, generators {gens:?})"
                    );
                }
            }
        });
    }

    /// The same property against the in-crate BFS oracle. GAP is the stronger
    /// check, but it costs a process per group; this one is cheap enough to
    /// cover an order of magnitude more groups.
    #[test]
    fn random_canonicalize_matches_naive() {
        random_test(200, |rng| {
            let n = rng.random_range(3..=6usize);
            let num_gens = rng.random_range(1..=3);
            let gens: Vec<Permutation> = (0..num_gens).map(|_| random_permutation(rng, n)).collect();
            let bsgs = bsgs_schreier_sims(&gens, n).unwrap();

            for params in random_states(rng, n, 20) {
                let state: Vec<usize> = std::iter::once(7).chain(params.iter().copied()).collect();
                assert_eq!(
                    bsgs.canonicalize(&state, 1),
                    bsgs.canonicalize_naive(&state, 1),
                    "canonicalize and the BFS oracle disagree on {params:?} (n = {n}, generators {gens:?})"
                );
            }
        });
    }

    /// Canonicalization has to be constant on orbits so every image of a random
    /// state must reach the same representative as the state itself.
    #[test]
    fn random_canonicalize_is_constant_on_orbits() {
        random_test(200, |rng| {
            let n = rng.random_range(3..=6usize);
            let num_gens = rng.random_range(1..=3);
            let gens: Vec<Permutation> = (0..num_gens).map(|_| random_permutation(rng, n)).collect();
            let bsgs = bsgs_schreier_sims(&gens, n).unwrap();
            let generators = bsgs.all_generators();

            for params in random_states(rng, n, 20) {
                let state: Vec<usize> = std::iter::once(7).chain(params.iter().copied()).collect();
                let expected = bsgs.canonicalize(&state, 1);

                assert_eq!(
                    bsgs.canonicalize(&expected, 1),
                    expected,
                    "canonicalize is not idempotent on {params:?} (n = {n}, generators {gens:?})"
                );

                for g in &generators {
                    let image: Vec<usize> = std::iter::once(7).chain(g.apply_to_vec(&params)).collect();
                    assert_eq!(
                        bsgs.canonicalize(&image, 1),
                        expected,
                        "{params:?} and its image {image:?} have different representatives \
                         (n = {n}, generators {gens:?})"
                    );
                }
            }
        });
    }

    #[test]
    fn permutation_to_gap_cycles_roundtrip() {
        let p = Permutation::from_cycle_notation("(0 2)(1 3)").unwrap();
        let gap_str = permutation_to_gap_cycles(&p, 4);
        // Should produce 1-based cycles
        let parsed = parse_gap_perm(&gap_str, 4).unwrap();
        let dense = DensePermutation::from_permutation(&p, 4);
        assert_eq!(parsed.images, dense.images);
    }
}
