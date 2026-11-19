use std::time::{Duration, SystemTime};

pub trait Clock {
    fn now(&self) -> SystemTime;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub minutes: u32,
    pub started_at: SystemTime,
    pub ends_at: SystemTime,
    pub passes_used: u32,
    pub pass_until: Option<SystemTime>,
}

#[derive(Debug, PartialEq)]
pub struct Status {
    pub left: Duration,
    pub pass_left: Option<Duration>,
    pub pass_just_ended: bool,
}

#[derive(Debug, PartialEq)]
pub enum Tick {
    Idle,
    Running(Status),
    Finished(Session),
}

#[derive(Debug, PartialEq)]
pub enum PassError {
    NoSession,
    NoneLeft,
}

pub struct Timer<C: Clock> {
    clock: C,
    minute: Duration,
    session: Option<Session>,
}

impl<C: Clock> Timer<C> {
    pub fn new(clock: C, minute: Duration) -> Self {
        Self { clock, minute, session: None }
    }

    pub fn start(&mut self, minutes: u32) {
        let now = self.clock.now();
        self.session = Some(Session {
            minutes,
            started_at: now,
            ends_at: now + self.minute * minutes,
            passes_used: 0,
            pass_until: None,
        });
    }

    pub fn stop(&mut self) -> Option<Session> {
        self.session.take()
    }

    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    pub fn is_running(&self) -> bool {
        self.session.is_some()
    }

    pub fn is_blocking(&self) -> bool {
        self.is_running() && self.pass_left().is_none()
    }

    pub fn remaining(&self) -> Option<Duration> {
        let s = self.session.as_ref()?;
        Some(until(s.ends_at, self.clock.now()))
    }

    pub fn pass_left(&self) -> Option<Duration> {
        let end = self.session.as_ref()?.pass_until?;
        let left = until(end, self.clock.now());
        (!left.is_zero()).then_some(left)
    }

    pub fn elapsed_minutes(&self, s: &Session) -> u32 {
        let end = self.clock.now().min(s.ends_at);
        let secs = end.duration_since(s.started_at).unwrap_or_default().as_secs_f64();
        (secs / self.minute.as_secs_f64()).round() as u32
    }

    pub fn grant_pass(&mut self, minutes: u32, max: u32) -> Result<(), PassError> {
        let now = self.clock.now();
        let minute = self.minute;
        let s = self.session.as_mut().ok_or(PassError::NoSession)?;
        if s.passes_used >= max {
            return Err(PassError::NoneLeft);
        }
        s.passes_used += 1;
        s.pass_until = Some(now + minute * minutes);
        Ok(())
    }

    pub fn tick(&mut self) -> Tick {
        let now = self.clock.now();
        let Some(s) = self.session.as_mut() else { return Tick::Idle };

        if until(s.ends_at, now).is_zero() {
            return Tick::Finished(self.session.take().unwrap());
        }

        let mut pass_just_ended = false;
        if let Some(end) = s.pass_until
            && until(end, now).is_zero()
        {
            s.pass_until = None;
            pass_just_ended = true;
        }

        Tick::Running(Status {
            left: until(s.ends_at, now),
            pass_left: s.pass_until.map(|end| until(end, now)),
            pass_just_ended,
        })
    }
}

fn until(t: SystemTime, now: SystemTime) -> Duration {
    t.duration_since(now).unwrap_or(Duration::ZERO)
}

pub fn format_remaining(d: Duration) -> String {
    let total = d.as_secs() + u64::from(d.subsec_nanos() > 0);
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

pub fn minutes_left(d: Duration, minute: Duration) -> u64 {
    d.as_millis().div_ceil(minute.as_millis().max(1)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    const MIN: Duration = Duration::from_secs(60);

    #[derive(Clone)]
    struct FakeClock(Rc<Cell<SystemTime>>);

    impl FakeClock {
        fn new() -> Self {
            Self(Rc::new(Cell::new(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000))))
        }
        fn advance(&self, secs: u64) {
            self.0.set(self.0.get() + Duration::from_secs(secs));
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> SystemTime {
            self.0.get()
        }
    }

    fn running(left_secs: u64) -> Tick {
        Tick::Running(Status {
            left: Duration::from_secs(left_secs),
            pass_left: None,
            pass_just_ended: false,
        })
    }

    #[test]
    fn idle_by_default() {
        let mut t = Timer::new(FakeClock::new(), MIN);
        assert_eq!(t.tick(), Tick::Idle);
        assert_eq!(t.remaining(), None);
        assert!(!t.is_blocking());
    }

    #[test]
    fn counts_down_and_finishes() {
        let clock = FakeClock::new();
        let mut t = Timer::new(clock.clone(), MIN);
        t.start(25);
        assert_eq!(t.tick(), running(25 * 60));

        clock.advance(10 * 60);
        assert_eq!(t.tick(), running(15 * 60));
        assert_eq!(t.elapsed_minutes(t.session().unwrap()), 10);

        clock.advance(15 * 60);
        match t.tick() {
            Tick::Finished(s) => assert_eq!(s.minutes, 25),
            other => panic!("expected Finished, got {other:?}"),
        }
        assert_eq!(t.tick(), Tick::Idle);
    }

    #[test]
    fn overshoot_still_finishes() {
        let clock = FakeClock::new();
        let mut t = Timer::new(clock.clone(), MIN);
        t.start(25);
        clock.advance(3 * 3600);
        assert!(matches!(t.tick(), Tick::Finished(_)));
    }

    #[test]
    fn stop_clears_session() {
        let mut t = Timer::new(FakeClock::new(), MIN);
        t.start(50);
        assert!(t.stop().is_some());
        assert!(!t.is_running());
        assert!(t.stop().is_none());
    }

    #[test]
    fn pass_suspends_blocking_then_expires_once() {
        let clock = FakeClock::new();
        let mut t = Timer::new(clock.clone(), MIN);
        t.start(25);
        assert!(t.is_blocking());

        t.grant_pass(2, 3).unwrap();
        assert!(!t.is_blocking());
        assert_eq!(t.pass_left(), Some(2 * MIN));

        clock.advance(60);
        match t.tick() {
            Tick::Running(s) => assert_eq!(s.pass_left, Some(MIN)),
            other => panic!("{other:?}"),
        }

        clock.advance(60);
        match t.tick() {
            Tick::Running(s) => assert!(s.pass_just_ended && s.pass_left.is_none()),
            other => panic!("{other:?}"),
        }
        assert!(t.is_blocking());
        assert!(matches!(t.tick(), Tick::Running(Status { pass_just_ended: false, .. })));
    }

    #[test]
    fn pass_limit() {
        let mut t = Timer::new(FakeClock::new(), MIN);
        assert_eq!(t.grant_pass(2, 3), Err(PassError::NoSession));
        t.start(25);
        for _ in 0..3 {
            t.grant_pass(2, 3).unwrap();
        }
        assert_eq!(t.grant_pass(2, 3), Err(PassError::NoneLeft));
        t.start(25);
        assert!(t.grant_pass(2, 3).is_ok());
    }

    #[test]
    fn fast_minutes() {
        let clock = FakeClock::new();
        let mut t = Timer::new(clock.clone(), Duration::from_secs(5));
        t.start(25);
        assert_eq!(t.remaining(), Some(Duration::from_secs(125)));
        clock.advance(125);
        assert!(matches!(t.tick(), Tick::Finished(_)));
    }

    #[test]
    fn formatting() {
        assert_eq!(format_remaining(Duration::from_secs(25 * 60)), "25:00");
        assert_eq!(format_remaining(Duration::from_millis(42 * 60_000 + 9_001)), "42:10");
        assert_eq!(format_remaining(Duration::from_secs(3900)), "1:05:00");
        assert_eq!(format_remaining(Duration::ZERO), "00:00");
        assert_eq!(minutes_left(Duration::from_secs(61), MIN), 2);
        assert_eq!(minutes_left(Duration::from_secs(60), MIN), 1);
    }
}
