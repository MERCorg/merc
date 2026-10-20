use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::io::Write;
use std::time::Instant;

use log::debug;

/// A timing object to measure the time of different parts of the program. This
/// is useful for debugging and profiling.
#[derive(Default)]
pub struct Timing {
    /// Stores the results of finished timers.
    results: RefCell<Vec<(String, f32)>>,
}

/// Aggregated timing summary for a named timer.
struct Aggregate {
    name: String,
    min: f32,
    max: f32,
    total: f32,
    avg: f32,
    count: usize,
}

impl Timing {
    /// Creates a new timing object to track timers.
    #[must_use]
    pub fn new() -> Self {
        Self {
            results: RefCell::new(Vec::new()),
        }
    }

    /// Measures the time taken for the given function and registers it under the given name.
    pub fn measure<F, O>(&self, name: &str, function: F) -> O
    where
        F: FnOnce() -> O,
    {
        let start = Instant::now();
        let result = function();

        let time = start.elapsed().as_secs_f32();
        debug!("Time {}: {:.3}s", name, time);

        // Register the result.
        self.results.borrow_mut().push((name.to_string(), time));
        result
    }

    /// Aggregate results by name and compute (min, max, avg, count, total) for each.
    fn aggregate_results(&self) -> Vec<Aggregate> {
        let mut map: HashMap<String, Aggregate> = HashMap::new();
        for (name, time) in self.results.borrow().iter() {
            map.entry(name.clone())
                .and_modify(|ag| {
                    ag.count += 1;
                    ag.total += *time;
                    ag.min = ag.min.min(*time);
                    ag.max = ag.max.max(*time);
                })
                .or_insert(Aggregate {
                    name: name.clone(),
                    min: *time,
                    max: *time,
                    total: *time,
                    avg: 0.0,
                    count: 1,
                });
        }

        // Compute the averages and sort by name.
        let mut out: Vec<Aggregate> = map
            .into_values()
            .map(|mut ag| {
                // `count` is always at least 1 (set to 1 on insertion).
                ag.avg = ag.total / (ag.count as f32);
                ag
            })
            .collect();

        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Prints all the finished timers aggregated by name (total first; omit metrics when n == 1).
    pub fn print(&self) {
        for ag in self.aggregate_results() {
            if ag.count == 1 {
                eprintln!("Time {}: {:.3}s", ag.name, ag.total);
            } else {
                eprintln!(
                    "Time {}: {:.3}s, min: {:.3}s, max: {:.3}s, avg: {:.3}s, n: {}",
                    ag.name, ag.total, ag.min, ag.max, ag.avg, ag.count
                );
            }
        }
    }

    /// Writes a YAML report of the finished timers to the given writer.
    pub fn print_yaml<W: Write>(&self, tool_name: &str, writer: &mut W) -> io::Result<()> {
        writeln!(writer, "- tool: {tool_name}")?;
        writeln!(writer, "  timing:")?;

        for ag in self.aggregate_results() {
            // Quote the timer name so names containing YAML-significant
            // characters (`:`, `#`, leading spaces, ...) stay valid YAML.
            writeln!(writer, "    \"{}\":", yaml_escape(&ag.name))?;
            writeln!(writer, "      total: {total:.3}s", total = ag.total)?;
            if ag.count > 1 {
                writeln!(writer, "      count: {}", ag.count)?;
                writeln!(writer, "      min: {min:.3}s", min = ag.min)?;
                writeln!(writer, "      max: {max:.3}s", max = ag.max)?;
                writeln!(writer, "      avg: {avg:.3}s", avg = ag.avg)?;
            }
        }
        Ok(())
    }
}

/// Escapes a string for use inside a YAML double-quoted scalar.
fn yaml_escape(name: &str) -> String {
    name.replace('\\', "\\\\").replace('"', "\\\"")
}
