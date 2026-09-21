use oxidd::BooleanFunction;
use oxidd::Edge;
use oxidd::Function;
use oxidd::Manager;
use oxidd::ManagerRef;
use oxidd::VarNo;
use oxidd::bdd::BDDFunction;
use oxidd::bdd::BDDManagerRef;
use oxidd::ldd::LDDFunction;
use oxidd::ldd::LDDManagerRef;
use oxidd::ldd::Value;
use oxidd::util::Borrowed;
use oxidd::util::OptBool;
use oxidd::util::OutOfMemory;
use oxidd_core::function::EdgeOfFunc;
use oxidd_core::util::EdgeDropGuard;
use oxidd_rules_bdd::simple::BDDTerminal;
use rustc_hash::FxHashMap;

use crate::height;

/// Converts an LDD representing a set of vectors into a BDD representing the
/// same set by bitblasting the vector elements using the given variables as
/// bits.
///
/// # Details
///
/// The `bits_per_layer` should be a singleton LDD containing the result of
/// [compute_bits]. The conversion works recursively by processing the LDD node
/// by node, and introducing bit number of BDD variables (given by
/// `bit_variables`) for each layer in the LDD. These variables *must* already
/// exist in the given BDD manager.
pub fn ldd_to_bdd(
    _ldd_manager_ref: &LDDManagerRef,
    bdd_manager_ref: &BDDManagerRef,
    ldd: &LDDFunction,
    bits_per_layer: &LDDFunction,
    bit_variables: &[VarNo],
) -> Result<BDDFunction, OutOfMemory> {
    // For LDDs we can assume that nodes in one layer are unique, so we don't
    // need the bits_per_layer and bit_variables in the cache.
    let mut cache = FxHashMap::default();

    bdd_manager_ref.with_manager_shared(|bdd_manager| -> Result<BDDFunction, OutOfMemory> {
        Ok(BDDFunction::from_edge(
            bdd_manager,
            ldd_to_bdd_edge(bdd_manager, &mut cache, ldd, bits_per_layer, bit_variables)?,
        ))
    })
}

/// Recursive implementation of [ldd_to_bdd].
#[allow(clippy::mutable_key_type)]
pub fn ldd_to_bdd_edge<'id>(
    bdd_manager: &<BDDFunction as Function>::Manager<'id>,
    cache: &mut FxHashMap<LDDFunction, BDDFunction>,
    ldd: &LDDFunction,
    bits_per_layer: &LDDFunction,
    bit_variables: &[VarNo],
) -> Result<EdgeOfFunc<'id, BDDFunction>, OutOfMemory> {
    // Base cases
    if ldd.is_empty() {
        return bdd_manager.get_terminal(BDDTerminal::False);
    }
    if ldd.is_empty_vector() {
        return bdd_manager.get_terminal(BDDTerminal::True);
    }

    if let Some(cached) = cache.get(ldd) {
        return Ok(bdd_manager.clone_edge(cached.as_edge(bdd_manager)));
    }

    let (value, down, right) = ldd.node().expect("ldd is an inner node");
    let (bits_value, bits_down, _bits_right) = bits_per_layer.node().expect("bits_per_layer is an inner node");

    // Right branch does not consume variables at this layer
    let right_bdd = EdgeDropGuard::new(
        bdd_manager,
        ldd_to_bdd_edge(bdd_manager, cache, &right, bits_per_layer, bit_variables)?,
    );

    let needed_bits = bits_value as usize;
    assert!(
        bit_variables.len() >= needed_bits,
        "Insufficient variables: need {needed_bits}, have {} for current layer",
        bit_variables.len()
    );

    // Recurse on down with the remaining variables after consuming this layer
    let mut down_bdd = EdgeDropGuard::new(
        bdd_manager,
        ldd_to_bdd_edge(bdd_manager, cache, &down, &bits_down, &bit_variables[needed_bits..])?,
    );

    // Encode current value using the variables for this layer (MSB to LSB)
    // Current layer variables: vars[0..bits_value]
    // The `ite` is necessary since the variables are not in sorted order.
    let f_edge = EdgeDropGuard::new(bdd_manager, bdd_manager.get_terminal(BDDTerminal::False)?);
    for i in 0..bits_value {
        let bit = bits_value - i - 1; // MSB first
        let var_no = bit_variables[bit as usize];
        let var = EdgeDropGuard::new(bdd_manager, BDDFunction::var_edge(bdd_manager, var_no)?);
        if value & (1 << i) != 0 {
            // bit is 1
            down_bdd = EdgeDropGuard::new(
                bdd_manager,
                BDDFunction::ite_edge(bdd_manager, &var, &down_bdd, &f_edge)?,
            );
        } else {
            // bit is 0
            down_bdd = EdgeDropGuard::new(
                bdd_manager,
                BDDFunction::ite_edge(bdd_manager, &var, &f_edge, &down_bdd)?,
            );
        }
    }

    let result = BDDFunction::or_edge(bdd_manager, &down_bdd, &right_bdd)?;
    cache.insert(ldd.clone(), BDDFunction::from_edge_ref(bdd_manager, &result));
    Ok(result)
}

/// Converts a BDD representing a set of bitblasted vectors back into an LDD
/// representing the same set, i.e., the inverse of [ldd_to_bdd].
pub fn bdd_to_ldd(
    ldd_manager_ref: &LDDManagerRef,
    manager_ref: &BDDManagerRef,
    bdd: &BDDFunction,
    variables: &[VarNo],
    bits_per_layer: &[Value],
    current_bit: Value,
    current_value: Value,
) -> Result<LDDFunction, OutOfMemory> {
    let mut cache = FxHashMap::default();
    // Hold both manager locks for the whole recursion: every LDD node is built with the lock-free
    // `*_edge` constructors, so the entire conversion runs under a single shared lock per manager.
    manager_ref.with_manager_shared(|manager| {
        ldd_manager_ref.with_manager_shared(|ldd_manager| {
            let edge = bdd.as_edge(manager);
            bdd_to_ldd_edge(
                ldd_manager,
                manager,
                &mut cache,
                edge.borrowed(),
                variables,
                bits_per_layer,
                current_bit,
                current_value,
            )
        })
    })
}

/// Recursive implementation of [bdd_to_ldd].
#[allow(clippy::mutable_key_type)]
#[allow(clippy::too_many_arguments)]
pub fn bdd_to_ldd_edge<'id, 'ldd>(
    ldd_manager: &<LDDFunction as Function>::Manager<'ldd>,
    manager: &<BDDFunction as Function>::Manager<'id>,
    cache: &mut FxHashMap<(BDDFunction, usize, Value, Value), LDDFunction>,
    bdd: Borrowed<EdgeOfFunc<'id, BDDFunction>>,
    variables: &[VarNo],
    bits_per_layer: &[Value],
    current_bit: Value,
    current_value: Value,
) -> Result<LDDFunction, OutOfMemory> {
    // Base case: the empty set is represented by the False terminal.
    if let oxidd::Node::Terminal(BDDTerminal::False) = manager.get_node(&bdd) {
        return LDDFunction::empty_set(ldd_manager);
    }

    // Base case: once all variables have been consumed the decoded value is complete. The BDD must
    // be the True terminal here, since any remaining path would require further variables.
    if variables.is_empty() {
        debug_assert!(
            matches!(manager.get_node(&bdd), oxidd::Node::Terminal(BDDTerminal::True)),
            "Expected the True terminal after consuming all variables"
        );
        return LDDFunction::singleton(ldd_manager, &[current_value]);
    }

    // Cache lookup: shared BDD subgraphs (including the True terminal reached while variables
    // remain) are otherwise expanded independently each time, causing exponential blowup when the
    // same node is reachable via multiple paths.
    let cache_key = (
        BDDFunction::from_edge_ref(manager, &*bdd),
        variables.len(),
        current_bit,
        current_value,
    );
    if let Some(cached) = cache.get(&cache_key) {
        return Ok(cached.clone());
    }

    // Read the bits required per layer
    let num_bits = bits_per_layer
        .first()
        .copied()
        .expect("Missing bits per layer for current layer");

    let result = if num_bits == current_bit {
        // We reached the last bit for this layer, so emit an LDD node holding the decoded value and
        // continue decoding the next layer.
        let down = bdd_to_ldd_edge(ldd_manager, manager, cache, bdd, variables, &bits_per_layer[1..], 0, 0)?;
        let right = LDDFunction::empty_set(ldd_manager)?;
        LDDFunction::make_node(ldd_manager, current_value, &down, &right)?
    } else {
        debug_assert!(current_bit < num_bits, "Current bit exceeds number of bits for layer");

        // Cofactor the BDD with respect to the variable for the current bit. Extracting an explicit
        // cofactor (instead of assuming the variable sits at the top of the BDD) makes decoding
        // independent of the BDD variable ordering. This is required because state and next-state
        // bit variables are interleaved level-wise, whereas a relation groups all read bits before
        // all write bits, so the variable list is generally not in level order.
        let var = *variables.first().expect("Missing variable for current layer");
        let var_edge = EdgeDropGuard::new(manager, BDDFunction::var_edge(manager, var)?);
        let neg_var_edge = EdgeDropGuard::new(manager, BDDFunction::not_edge(manager, &var_edge)?);

        let high_edge = EdgeDropGuard::new(manager, BDDFunction::restrict_edge(manager, &*bdd, &var_edge)?);
        let low_edge = EdgeDropGuard::new(manager, BDDFunction::restrict_edge(manager, &*bdd, &neg_var_edge)?);

        // Recurse for high and low cofactors
        let high = bdd_to_ldd_edge(
            ldd_manager,
            manager,
            cache,
            high_edge.borrowed(),
            &variables[1..],
            bits_per_layer,
            current_bit + 1,
            current_value | (1 << (num_bits - current_bit - 1)),
        )?;
        let low = bdd_to_ldd_edge(
            ldd_manager,
            manager,
            cache,
            low_edge.borrowed(),
            &variables[1..],
            bits_per_layer,
            current_bit + 1,
            current_value,
        )?;
        LDDFunction::union_edge(ldd_manager, &high, &low)?
    };

    cache.insert(cache_key, result.clone());
    Ok(result)
}

/// Computes the highest value for every layer in the LDD
pub fn compute_highest(storage: &LDDManagerRef, ldd: &LDDFunction) -> Vec<u32> {
    let mut result = vec![0; height(storage, ldd)];
    compute_highest_rec(&mut result, ldd, 0);
    result
}

/// Iterator that yields values reconstructed from a bit cube, from a BDD obtained by
/// [ldd_to_bdd].
pub struct ValuesIter<'a> {
    bits: &'a [OptBool],
    num_of_bits: &'a [u32],
    offset: usize,
    index: usize,
}

impl<'a> ValuesIter<'a> {
    pub fn new(bits: &'a [OptBool], num_of_bits: &'a [u32]) -> Self {
        Self {
            bits,
            num_of_bits,
            offset: 0,
            index: 0,
        }
    }
}

impl<'a> Iterator for ValuesIter<'a> {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.num_of_bits.len() {
            return None;
        }

        let nb = self.num_of_bits[self.index] as usize;
        // Clamp the slice to the available bits so the iterator always yields exactly
        // num_of_bits.len() items, honoring the ExactSizeIterator contract even on
        // truncated input.
        let start = self.offset.min(self.bits.len());
        let end = (self.offset + nb).min(self.bits.len());
        let value = to_value(&self.bits[start..end]);
        self.offset += nb;
        self.index += 1;
        Some(value)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.num_of_bits.len().saturating_sub(self.index);
        (remaining, Some(remaining))
    }
}

impl<'a> ExactSizeIterator for ValuesIter<'a> {}

/// Reconstruct the value represented by the bits as encoded by
/// [crate::ldd_to_bdd]. So most significant bit first.
pub fn to_value(bits: &[OptBool]) -> u64 {
    let mut value = 0u64;
    for (bit_pos, bit) in bits.iter().rev().enumerate() {
        if *bit == OptBool::True {
            // Bits beyond the 64-bit range cannot be represented in a u64; ignore them
            // rather than overflowing the shift (checked_shl yields None for bit_pos >= 64).
            if let Some(mask) = 1u64.checked_shl(bit_pos as u32) {
                value |= mask;
            }
        }
    }

    value
}

/// Helper function for compute_highest
fn compute_highest_rec(result: &mut [u32], set: &LDDFunction, depth: usize) {
    let (value, down, right) = match set.node() {
        // Terminals (empty set / empty vector) have no contribution.
        None => return,
        Some(node) => node,
    };

    compute_highest_rec(result, &right, depth);
    compute_highest_rec(result, &down, depth + 1);

    result[depth] = result[depth].max(value);
}

/// Calculate minimum bits needed to represent the value
/// Use 1 bit if value is 0 to ensure at least 1 bit is written
pub fn required_bits(value: u32) -> u32 {
    (u32::BITS - value.leading_zeros()).max(1)
}

/// Calculate minimum bits needed to represent the value
pub fn required_bits_64(value: u64) -> u32 {
    (u64::BITS - value.leading_zeros()).max(1)
}

/// Computes the number of bits required to represent the highest value at each layer.
pub fn compute_bits(highest: &[u32]) -> Vec<u32> {
    highest.iter().map(|&h| required_bits(h)).collect()
}

#[cfg(test)]
mod tests {
    use oxidd::Manager;
    use oxidd::ManagerRef;
    use oxidd::ldd::LDDFunction;

    use merc_utilities::random_test;

    use crate::BDD_CACHE_CAPACITY;
    use crate::BDD_NODE_CAPACITY;
    use crate::FormatConfigSet;
    use crate::LDD_CACHE_CAPACITY;
    use crate::LDD_NODE_CAPACITY;
    use crate::LddDisplay;
    use crate::bdd_to_ldd;
    use crate::compute_bits;
    use crate::compute_highest;
    use crate::from_iter;
    use crate::ldd_to_bdd;
    use crate::random_vector_set;
    use crate::required_bits;

    #[test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_random_compute_highest() {
        random_test(100, |rng| {
            let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
            let set = random_vector_set(rng, 4, 3, 5);
            let ldd = from_iter(&manager, set.iter());
            println!("LDD: {}", LddDisplay::new(&ldd));

            let highest = compute_highest(&manager, &ldd);
            println!("Highest: {:?}", highest);
            for (i, h) in highest.iter().enumerate() {
                // Determine the highest value for every vector
                for value in set.iter() {
                    assert!(
                        *h >= value[i],
                        "The highest value for depth {} is {}, but vector has value {}",
                        i,
                        h,
                        value[i]
                    );
                }
            }

            let bits = compute_bits(&highest);
            println!("Bits: {:?}", bits);

            for (i, b) in bits.iter().enumerate() {
                let expected_bits = required_bits(highest[i]);
                assert_eq!(
                    *b, expected_bits,
                    "The number of bits for depth {} is {}, but expected {}",
                    i, b, expected_bits
                );
            }
        })
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_random_ldd_to_bdd() {
        random_test(100, |rng| {
            let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
            let set = random_vector_set(rng, 50, 3, 5);

            let ldd = from_iter(&manager, set.iter());
            println!("LDD: {}", LddDisplay::new(&ldd));

            let highest = compute_highest(&manager, &ldd);
            let bits = compute_bits(&highest);
            let bits_dd = manager
                .with_manager_shared(|m| LDDFunction::singleton(m, &bits))
                .unwrap();

            let manager_ref = oxidd::bdd::new_manager(BDD_NODE_CAPACITY, BDD_CACHE_CAPACITY, 1);

            let total_bits: u32 = bits.iter().sum();
            println!("Total bits: {}", total_bits);
            println!("Bits per layer: {:?}", bits);
            let vars = manager_ref.with_manager_exclusive(|manager| manager.add_vars(total_bits).collect::<Vec<_>>());
            let bdd = ldd_to_bdd(&manager, &manager_ref, &ldd, &bits_dd, &vars).unwrap();
            println!("resulting BDD: {}", FormatConfigSet(&bdd));
            let resulting_ldd = bdd_to_ldd(&manager, &manager_ref, &bdd, &vars, &bits, 0, 0).unwrap();

            println!("resulting LDD: {}", LddDisplay::new(&resulting_ldd));
            assert!(ldd == resulting_ldd, "Converted LDD does not match original");
        });
    }
}
