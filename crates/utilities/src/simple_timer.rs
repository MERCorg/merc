use std::time::Duration;
use std::time::Instant;

/// A timer that can be started, stopped, and reset.
/// Keeps track of accumulated time when running.
#[derive(Debug)]
pub struct SimpleTimer {
    /// The instant when the timer was last started
    start: Instant,
    /// Total time accumulated from previous runs
    accumulated: Duration,
    /// Whether the timer is currently running
    running: bool,
}

impl SimpleTimer {
    /// Creates a new running timer with zero accumulated time.
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            accumulated: Duration::from_secs(0),
            running: true,
        }
    }

    /// Starts the timer if it's not already running.
    pub fn start(&mut self) {
        debug_assert!(
            !self.running,
            "Starting a timer that is already running. Call stop() first."
        );

        if !self.running {
            self.start = Instant::now();
            self.running = true;
        }
    }

    /// Stops the timer if it's running and accumulates elapsed time.
    pub fn stop(&mut self) {
        debug_assert!(self.running, "Stopping a timer that is not running.");

        if self.running {
            self.accumulated += self.start.elapsed();
            self.running = false;
        }
    }

    /// Resets the accumulated time to zero. If the timer is running, also
    /// resets the start time.
    pub fn reset(&mut self) {
        self.accumulated = Duration::from_secs(0);
        if self.running {
            self.start = Instant::now();
        }
    }

    /// Returns the total elapsed time. If running, includes time since last
    /// start.
    pub fn elapsed(&self) -> Duration {
        if self.running {
            self.accumulated + self.start.elapsed()
        } else {
            self.accumulated
        }
    }

    /// Returns whether the timer is currently running.
    pub fn running(&self) -> bool {
        self.running
    }
}

impl Default for SimpleTimer {
    /// Creates a new timer with default settings.
    /// Equivalent to `Timer::new()`.
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::thread::sleep;

    #[test]
    fn test_timer_basic() {
        let mut timer = SimpleTimer::new();
        assert!(timer.running());

        sleep(Duration::from_millis(10));
        timer.stop();

        let elapsed = timer.elapsed();
        assert!(!timer.running());
        assert!(elapsed >= Duration::from_millis(10));
    }

    #[test]
    fn test_timer_reset() {
        let mut timer = SimpleTimer::new();
        sleep(Duration::from_millis(10));
        timer.stop();

        let first_elapsed = timer.elapsed();
        timer.reset();
        assert_eq!(timer.elapsed(), Duration::from_secs(0));
        assert!(first_elapsed > timer.elapsed());
    }
}
