//! Background work that must not be felt: it works in short slices and rests in between, so it
//! never takes more than a set share of one core, however much there is to do.

use std::time::{Duration, Instant};

/// How long the work goes on before it rests.
const SLICE: Duration = Duration::from_millis(10);

pub struct Pace {
    /// Rest per unit of work; 9 means a tenth of one core.
    rest: u32,
    since: Instant,
}

impl Pace {
    /// At most `percent` of one core.
    pub fn percent(percent: u32) -> Self {
        Self {
            rest: (100 / percent.clamp(1, 100)) - 1,
            since: Instant::now(),
        }
    }

    /// How long to rest after working for `worked`; nothing until a slice is used up.
    fn rest_after(&self, worked: Duration) -> Option<Duration> {
        (worked >= SLICE).then(|| worked * self.rest)
    }

    /// Called between pieces of work, on a thread that may block.
    pub fn breathe(&mut self) {
        if let Some(rest) = self.rest_after(self.since.elapsed()) {
            std::thread::sleep(rest);
            self.since = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tenth_of_a_core_rests_nine_times_as_long_as_it_works() {
        let pace = Pace::percent(10);
        assert_eq!(pace.rest_after(Duration::from_millis(5)), None);
        assert_eq!(
            pace.rest_after(Duration::from_millis(20)),
            Some(Duration::from_millis(180))
        );
    }

    #[test]
    fn busy_work_stays_within_its_share() {
        let mut pace = Pace::percent(20);
        let start = Instant::now();
        let mut worked = Duration::ZERO;
        while start.elapsed() < Duration::from_millis(600) {
            let slice = Instant::now();
            while slice.elapsed() < Duration::from_millis(2) {
                std::hint::spin_loop();
            }
            worked += slice.elapsed();
            pace.breathe();
        }
        let share = worked.as_secs_f64() / start.elapsed().as_secs_f64();
        assert!(share < 0.35, "took {share:.2} of a core");
    }
}
