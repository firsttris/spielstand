//! Logins and pairing attempts are counted the moment they start; past a limit within a
//! minute, further attempts are refused before they are checked. Counting at the start (not
//! after a failure) also holds against many guesses sent at once. A successful attempt does
//! not count.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// The next attempt's id, and the attempts still inside the window (id, when).
type Counted = (u64, VecDeque<(u64, Instant)>);

#[derive(Clone)]
pub struct Throttle {
    max: usize,
    window: Duration,
    attempts: Arc<Mutex<Counted>>,
}

/// One counted attempt; `succeeded` takes it back.
pub struct Attempt {
    id: u64,
    throttle: Throttle,
}

impl Attempt {
    pub fn succeeded(self) {
        let mut guard = self.throttle.attempts.lock().unwrap();
        guard.1.retain(|(id, _)| *id != self.id);
    }
}

impl Throttle {
    pub fn new(max: usize, window: Duration) -> Self {
        Self {
            max,
            window,
            attempts: Arc::default(),
        }
    }

    /// Counts an attempt; `None` while too many were made recently.
    pub fn attempt(&self, now: Instant) -> Option<Attempt> {
        let mut guard = self.attempts.lock().unwrap();
        let (next, list) = &mut *guard;
        while list
            .front()
            .is_some_and(|(_, t)| now.duration_since(*t) > self.window)
        {
            list.pop_front();
        }
        if list.len() >= self.max {
            return None;
        }
        *next += 1;
        list.push_back((*next, now));
        Some(Attempt {
            id: *next,
            throttle: self.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempts_are_refused_past_the_limit_until_the_window_passed() {
        let t = Throttle::new(3, Duration::from_secs(60));
        let start = Instant::now();
        let held: Vec<Attempt> = (0..3).map(|_| t.attempt(start).unwrap()).collect();
        assert!(t.attempt(start + Duration::from_secs(59)).is_none());
        assert!(t.attempt(start + Duration::from_secs(61)).is_some());
        drop(held);
    }

    #[test]
    fn successful_attempts_do_not_count() {
        let t = Throttle::new(2, Duration::from_secs(60));
        let now = Instant::now();
        for _ in 0..10 {
            t.attempt(now).unwrap().succeeded();
        }
        assert!(t.attempt(now).is_some());
    }
}
