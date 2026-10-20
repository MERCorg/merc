use std::fmt;
use std::sync::Arc;

use merc_unsafety::ShardedHashMap;
use rustc_hash::FxBuildHasher;

use merc_utilities::MercError;
use merc_utilities::ShardedCounter;

use crate::LPS;
use crate::OwnedStateEffect;
use crate::SequenceForest;
use crate::SequenceForestContext;
use crate::StateEffect;
use crate::Summand;
use crate::Tree;

/// Controls the enumeration caching behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum CachingStrategy {
    /// No caching; every enumeration is performed from scratch.
    None,
    /// Each summand maintains its own independent cache.
    Local,
}

/// Number of shards in each summand's local cache.
const CACHE_SHARDS: usize = 16;

/// A wrapper around an [`LPS`] that caches summand enumeration results.
///
/// The cache key for each summand is formed by projecting the state vector onto
/// the summand's read positions. The cache is thread-safe, and local context is
/// used for the scratch buffers required to project the state vector and
/// reconstruct next states from cached values.
pub struct CacheLPS<P: LPS> {
    inner: Arc<P>,
    summands: Vec<CacheSummandWrapper<P>>,
}

/// A single cached enumeration result keyed by the read-position projection.
///
/// Stored as a bare element in a summand's cache table. The results indicate the
/// transition label and the captured write-position values for each next state.
struct CacheEntry<L> {
    /// Hash-consed projection of the source state onto the read positions.
    key: Tree,
    /// Captured `(label, write-position values tree)` pairs for the key.
    results: Vec<(L, Tree)>,
}

/// Per-thread reusable scratch buffers for enumeration.
pub struct CacheContext<P: LPS> {
    /// Buffer projecting the state vector onto read (then write) positions.
    key_buf: Vec<P::Value>,
    /// Buffer reconstructing a next-state from a cached write-position tree.
    replay_buf: Vec<P::Value>,
    /// Context reused when interning into the forest.
    forest_context: SequenceForestContext,
    /// Enumeration context for the wrapped inner summand (cache misses).
    inner: <P::Summand as Summand>::Context,
}

/// Thin metadata wrapper for a single cached summand.
pub struct CacheSummandWrapper<P: LPS> {
    /// Index of the summand in the LPS, used for cache lookups.
    index: usize,

    /// Positions in the state vector that are read by this summand; these form the cache key.
    read_positions: Vec<usize>,
    /// How the inner summand's next states relate to the source state, which
    /// decides whether cache values hold write positions or whole vectors.
    effect: OwnedStateEffect,

    /// Caching strategy in effect for this summand.
    strategy: CachingStrategy,

    /// This summand's local enumeration cache, keyed by the read-position
    /// projection tree. One independent cache per summand, as intended by the
    /// `Local` strategy.
    cache: ShardedHashMap<CacheEntry<P::Label>, FxBuildHasher>,

    /// Hash-consed forest holding the keys and captured values. Shared across
    /// all summands so equal sequences are stored once.
    forest: Arc<SequenceForest<P::Value, 2>>,

    /// Shared reference to the inner LPS for delegating cache misses.
    inner: Arc<P>,

    /// Number of enumerations served from the cache. Only updated when the
    /// `metrics` feature is enabled; otherwise it stays at zero.
    hits: ShardedCounter,
    /// Number of enumerations delegated to the inner summand. Only updated when
    /// the `metrics` feature is enabled; otherwise it stays at zero.
    misses: ShardedCounter,
}

impl<P: LPS> CacheLPS<P> {
    pub fn new(inner: P, strategy: CachingStrategy) -> Self {
        let inner = Arc::new(inner);
        let forest = Arc::new(SequenceForest::new());

        let summands: Vec<_> = inner
            .summands()
            .iter()
            .enumerate()
            .map(|(i, s)| CacheSummandWrapper {
                index: i,
                read_positions: s.read_positions().to_vec(),
                effect: s.effect().to_owned(),
                strategy,
                cache: ShardedHashMap::with_shards(CACHE_SHARDS),
                forest: Arc::clone(&forest),
                inner: Arc::clone(&inner),
                hits: ShardedCounter::new(),
                misses: ShardedCounter::new(),
            })
            .collect();

        CacheLPS { inner, summands }
    }

    /// Returns the wrapped LPS, so callers can reach capabilities that this
    /// wrapper forwards rather than implements.
    pub fn inner(&self) -> &P {
        &self.inner
    }

    /// Collects per-summand cache metrics for this [`CacheLPS`].
    ///
    /// The returned [`CacheMetrics`] implements [`fmt::Display`] for a
    /// human-readable summary of cache hits, misses and occupancy.
    pub fn metrics(&self) -> CacheMetrics {
        let summands = self
            .summands
            .iter()
            .map(|s| {
                let entries = match s.strategy {
                    CachingStrategy::None => 0,
                    CachingStrategy::Local => s.cache.len(),
                };

                SummandCacheMetrics {
                    index: s.index,
                    strategy: s.strategy,
                    hits: s.hits.get(),
                    misses: s.misses.get(),
                    entries,
                }
            })
            .collect();

        CacheMetrics { summands }
    }
}

/// Cache metrics for a single summand.
#[derive(Clone, Copy, Debug)]
pub struct SummandCacheMetrics {
    /// Index of the summand in the LPS.
    pub index: usize,
    /// Caching strategy in effect for this summand.
    pub strategy: CachingStrategy,
    /// Number of enumerations served from the cache.
    pub hits: u64,
    /// Number of enumerations delegated to the inner summand.
    pub misses: u64,
    /// Number of distinct keys currently stored for this summand.
    pub entries: usize,
}

impl SummandCacheMetrics {
    /// Total number of enumeration lookups.
    pub fn lookups(&self) -> u64 {
        self.hits + self.misses
    }

    /// Fraction of lookups served from the cache, in `[0.0, 1.0]`.
    ///
    /// Returns `0.0` when there were no lookups.
    pub fn hit_rate(&self) -> f64 {
        let lookups = self.lookups();
        if lookups == 0 {
            0.0
        } else {
            self.hits as f64 / lookups as f64
        }
    }
}

/// Aggregated cache metrics for every summand of a [`CacheLPS`].
#[derive(Clone, Debug)]
pub struct CacheMetrics {
    /// Per-summand metrics, ordered by summand index.
    pub summands: Vec<SummandCacheMetrics>,
}

impl CacheMetrics {
    /// Total number of cache hits across all summands.
    pub fn total_hits(&self) -> u64 {
        self.summands.iter().map(|s| s.hits).sum()
    }

    /// Total number of cache misses across all summands.
    pub fn total_misses(&self) -> u64 {
        self.summands.iter().map(|s| s.misses).sum()
    }

    /// Total number of cached entries across all summands.
    pub fn total_entries(&self) -> usize {
        self.summands.iter().map(|s| s.entries).sum()
    }

    /// Fraction of lookups served from the cache across all summands.
    ///
    /// Returns `0.0` when there were no lookups.
    pub fn hit_rate(&self) -> f64 {
        let hits = self.total_hits();
        let lookups = hits + self.total_misses();
        if lookups == 0 {
            0.0
        } else {
            hits as f64 / lookups as f64
        }
    }
}

impl fmt::Display for CacheMetrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if cfg!(not(feature = "metrics")) {
            return write!(f, "enable the 'metrics' feature to see cache metrics");
        }

        writeln!(f, "summand cache metrics:")?;
        writeln!(
            f,
            "  {:>7}  {:>8}  {:>10}  {:>10}  {:>8}  {:>8}",
            "summand", "strategy", "hits", "misses", "hit%", "entries"
        )?;
        for s in &self.summands {
            let strategy = match s.strategy {
                CachingStrategy::None => "none",
                CachingStrategy::Local => "local",
            };
            writeln!(
                f,
                "  {:>7}  {:>8}  {:>10}  {:>10}  {:>7.1}%  {:>8}",
                s.index,
                strategy,
                s.hits,
                s.misses,
                s.hit_rate() * 100.0,
                s.entries
            )?;
        }
        write!(
            f,
            "  total: {} hits, {} misses ({:.1}% hit rate), {} entries",
            self.total_hits(),
            self.total_misses(),
            self.hit_rate() * 100.0,
            self.total_entries()
        )
    }
}

impl<P: LPS> LPS for CacheLPS<P> {
    type Value = P::Value;
    type Label = P::Label;
    type StateInfo = P::StateInfo;
    type Summand = CacheSummandWrapper<P>;

    const HAS_LABELS: bool = P::HAS_LABELS;

    fn initial_state(&self) -> Vec<Self::Value> {
        self.inner.initial_state()
    }

    fn summands(&self) -> &[Self::Summand] {
        &self.summands
    }

    fn create_context(&self) -> <Self::Summand as Summand>::Context {
        CacheContext {
            key_buf: Vec::new(),
            replay_buf: Vec::new(),
            forest_context: SequenceForestContext::new(),
            inner: self.inner.create_context(),
        }
    }

    fn prepare<'a>(
        &'a self,
        context: &mut <Self::Summand as Summand>::Context,
        state: &'a [Self::Value],
    ) -> impl Iterator<Item = usize> + 'a {
        // The cache wraps each inner summand one-for-one in order, so the inner
        // summand indices map directly onto `self.summands`.
        self.inner.prepare(&mut context.inner, state)
    }

    fn state_info(&self, state: &[Self::Value], context: &<Self::Summand as Summand>::Context) -> Self::StateInfo {
        self.inner.state_info(state, &context.inner)
    }
}

impl<P: LPS> CacheSummandWrapper<P> {
    /// Replays a cache hit for a summand with a positional effect.
    ///
    /// # Details
    ///
    /// Each cached tree holds only the values at the write positions.
    /// The remaining positions are passed through unchanged, so they
    /// must be taken from the *live* source state rather than the state
    /// for which the entry was originally computed.
    fn replay_partial<F>(
        &self,
        context: &mut CacheContext<P>,
        state: &[P::Value],
        write_positions: &[usize],
        results: &[(P::Label, Tree)],
        report: &mut F,
    ) -> Result<(), MercError>
    where
        F: FnMut(&P::Label, &[P::Value]) -> Result<(), MercError>,
    {
        let replay_buf = &mut context.replay_buf;
        for (label, write_tree) in results {
            replay_buf.clear();
            replay_buf.extend_from_slice(state);
            for (&pos, value) in write_positions.iter().zip(self.forest.iter(*write_tree)) {
                replay_buf[pos] = value;
            }
            report(label, replay_buf)?;
        }
        Ok(())
    }

    /// Replays a cache hit for a summand with an opaque effect.
    fn replay_full<F>(
        &self,
        context: &mut CacheContext<P>,
        results: &[(P::Label, Tree)],
        report: &mut F,
    ) -> Result<(), MercError>
    where
        F: FnMut(&P::Label, &[P::Value]) -> Result<(), MercError>,
    {
        let replay_buf = &mut context.replay_buf;
        for (label, full_tree) in results {
            replay_buf.clear();
            replay_buf.extend(self.forest.iter(*full_tree));
            report(label, replay_buf)?;
        }
        Ok(())
    }
}

impl<P: LPS> Summand for CacheSummandWrapper<P> {
    type Value = P::Value;
    type Label = P::Label;
    type Context = CacheContext<P>;

    /// # Concurrency
    ///
    /// On a cache hit the captured results are replayed in place while this
    /// summand's cache shard is **read-locked**, so `report` runs under that
    /// lock. As a consequence `report` must not re-enter this same summand's
    /// `enumerate`, because that will deadlock.
    fn enumerate<F>(&self, context: &mut Self::Context, state: &[Self::Value], mut report: F) -> Result<(), MercError>
    where
        F: FnMut(&Self::Label, &[Self::Value]) -> Result<(), MercError>,
    {
        if self.strategy == CachingStrategy::None {
            return self.inner.summands()[self.index].enumerate(&mut context.inner, state, report);
        }

        // Project the state vector onto the read positions to form the cache
        // key, interning it into the shared forest.
        let key_tree = {
            let CacheContext {
                key_buf,
                forest_context,
                ..
            } = &mut *context;
            key_buf.clear();
            for &pos in &self.read_positions {
                key_buf.push(state[pos]);
            }
            self.forest.insert_with(key_buf, forest_context)
        };

        // This summand owns its cache, so the key is the projection tree alone.
        let hash = self.cache.hash(&key_tree);
        let eq = |entry: &CacheEntry<P::Label>| entry.key == key_tree;

        // Fast path: a present entry is replayed in place under the shard read
        // lock, without cloning the entry's captured results.
        let hit = self.cache.find_with(hash, eq, |entry| match &self.effect {
            OwnedStateEffect::Positions(positions) => {
                self.replay_partial(context, state, positions, &entry.results, &mut report)
            }
            OwnedStateEffect::Opaque => self.replay_full(context, &entry.results, &mut report),
        });

        if let Some(result) = hit {
            #[cfg(feature = "metrics")]
            self.hits.increment();
            result?;
            return Ok(());
        }

        #[cfg(feature = "metrics")]
        self.misses.increment();
        // Cache MISS: delegate to inner summand and capture results. An opaque
        // effect stores the complete next-state vector; a positional one stores
        // only the write-position values, with pass-through from the source.
        let mut captured: Vec<(P::Label, Tree)> = Vec::new();
        {
            let inner_summand = &self.inner.summands()[self.index];
            let forest = &self.forest;
            let effect = &self.effect;
            let CacheContext {
                key_buf,
                forest_context,
                inner,
                ..
            } = &mut *context;

            inner_summand.enumerate(inner, state, |label, next_state| {
                key_buf.clear();
                match effect {
                    OwnedStateEffect::Positions(positions) => {
                        debug_assert!(
                            positional_effect_holds(state, next_state, positions),
                            "summand claims StateEffect::Positions but changed a position outside it, \
                             or changed the state length; replaying this from the cache would produce \
                             a wrong next state"
                        );
                        for &pos in positions {
                            key_buf.push(next_state[pos]);
                        }
                    }
                    OwnedStateEffect::Opaque => key_buf.extend_from_slice(next_state),
                }
                let tree = forest.insert_with(key_buf, forest_context);
                captured.push((label.clone(), tree));
                report(label, next_state)
            })?;
        }

        // Publish the captured results. A concurrent thread may have inserted
        // the same key meanwhile; `insert_with` keeps the resident entry and our
        // copy is dropped. Either way the transitions above were already
        // reported.
        self.cache.insert_with(
            hash,
            eq,
            |entry| self.cache.hash(&entry.key),
            || CacheEntry {
                key: key_tree,
                results: captured,
            },
        );

        Ok(())
    }

    fn read_positions(&self) -> &[usize] {
        &self.read_positions
    }

    fn effect(&self) -> StateEffect<'_> {
        self.effect.borrow()
    }
}

/// Checks the [`StateEffect::Positions`] contract for one enumerated transition.
///
/// Only called from a `debug_assert!`, since it costs `O(state.len())` per
/// transition. A violation is otherwise silent: the transition itself is reported
/// correctly, and only a later cache *hit* replays it onto the wrong source state.
fn positional_effect_holds<V: PartialEq>(state: &[V], next_state: &[V], write_positions: &[usize]) -> bool {
    if state.len() != next_state.len() {
        return false;
    }

    (0..state.len()).all(|pos| write_positions.contains(&pos) || state[pos] == next_state[pos])
}
