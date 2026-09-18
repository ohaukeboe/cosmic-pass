//! One-time code countdown math.

use secrecy::SecretString;

/// `pass-cli` does not report the period; standard TOTP uses 30 seconds.
pub const DEFAULT_PERIOD: u32 = 30;

/// A one-time code shown in the detail pane.
#[derive(Debug, Clone)]
pub struct TotpDisplay {
    pub field: String,
    pub code: SecretString,
    pub period: u32,
    /// Unix second at which the code expires.
    pub valid_until: i64,
}

impl TotpDisplay {
    pub fn new(field: String, code: SecretString, now: i64) -> Self {
        Self {
            field,
            code,
            period: DEFAULT_PERIOD,
            valid_until: valid_until(now, DEFAULT_PERIOD),
        }
    }

    /// Seconds left, counting down from `period` to 1.
    pub fn remaining(&self, now: i64) -> u32 {
        u32::try_from(self.valid_until.saturating_sub(now).max(0)).unwrap_or(self.period)
    }

    pub fn needs_refresh(&self, now: i64) -> bool {
        now >= self.valid_until
    }
}

/// The next multiple of `period` strictly after `now`.
pub fn valid_until(now: i64, period: u32) -> i64 {
    let period = i64::from(period.max(1));
    (now.div_euclid(period) + 1) * period
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_until_is_next_period_boundary() {
        assert_eq!(valid_until(0, 30), 30);
        assert_eq!(valid_until(29, 30), 30);
        assert_eq!(valid_until(30, 30), 60);
        assert_eq!(valid_until(1_000_001, 30), 1_000_020);
        assert_eq!(valid_until(-1, 30), 0);
    }

    #[test]
    fn remaining_counts_down() {
        let t = TotpDisplay::new("totp_uri".into(), SecretString::from("1"), 30);
        assert_eq!(t.period, DEFAULT_PERIOD);
        assert_eq!(t.valid_until, 60);
        assert_eq!(t.remaining(30), 30);
        assert_eq!(t.remaining(31), 29);
        assert_eq!(t.remaining(59), 1);
        assert_eq!(t.remaining(60), 0);
        assert_eq!(t.remaining(99), 0);
    }

    #[test]
    fn refresh_when_expired() {
        let t = TotpDisplay::new("f".into(), SecretString::from("1"), 45);
        assert!(!t.needs_refresh(59));
        assert!(t.needs_refresh(60));
        assert!(t.needs_refresh(61));
    }
}
