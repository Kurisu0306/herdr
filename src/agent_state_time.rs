//! When an agent last changed state.
//!
//! The server stamps the wall-clock time of each state change and publishes it as a reserved
//! pane token. The client shell snapshot and `AgentInfo` are frozen or schema-tracked shapes;
//! their token maps are the per-agent channel that may grow, so older clients simply carry one
//! more token they never display.

use std::time::{SystemTime, UNIX_EPOCH};

/// Token holding the Unix time, in milliseconds, of the agent's last state change. The dot keeps
/// it out of the namespace sidebar rows and reporters can name.
pub(crate) const STATE_CHANGED_AT_TOKEN: &str = "herdr.state_changed_at";

pub(crate) fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

/// The published change time among an agent's tokens, if its server reports one.
pub(crate) fn changed_at_ms<'a>(
    mut tokens: impl Iterator<Item = (&'a str, &'a str)>,
) -> Option<u64> {
    tokens
        .find(|(name, _)| *name == STATE_CHANGED_AT_TOKEN)
        .and_then(|(_, value)| value.parse().ok())
}

/// Compact age such as `now`, `5m`, `3h`, or `2d`. A change stamped after `now_ms`, as a machine
/// with a faster clock can report, reads as `now`.
pub(crate) fn age_label(changed_at_ms: u64, now_ms: u64) -> String {
    let minutes = now_ms.saturating_sub(changed_at_ms) / 60_000;
    match minutes {
        0 => "now".to_owned(),
        1..60 => format!("{minutes}m"),
        60..1440 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1440),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: u64 = 60_000;

    #[test]
    fn age_uses_the_largest_whole_unit() {
        let now = 10 * 24 * 60 * MINUTE;
        for (elapsed, expected) in [
            (0, "now"),
            (MINUTE - 1, "now"),
            (MINUTE, "1m"),
            (59 * MINUTE, "59m"),
            (60 * MINUTE, "1h"),
            (23 * 60 * MINUTE + 59 * MINUTE, "23h"),
            (24 * 60 * MINUTE, "1d"),
            (9 * 24 * 60 * MINUTE, "9d"),
        ] {
            assert_eq!(age_label(now - elapsed, now), expected, "elapsed {elapsed}");
        }
    }

    #[test]
    fn change_from_a_faster_clock_reads_as_now() {
        assert_eq!(age_label(5 * MINUTE, MINUTE), "now");
    }

    #[test]
    fn change_time_is_read_from_the_reserved_token_only() {
        let tokens = [("summary", "12"), (STATE_CHANGED_AT_TOKEN, "1700000000000")];
        assert_eq!(
            changed_at_ms(tokens.iter().copied()),
            Some(1_700_000_000_000)
        );
        assert_eq!(changed_at_ms([("summary", "12")].into_iter()), None);
        assert_eq!(
            changed_at_ms([(STATE_CHANGED_AT_TOKEN, "soon")].into_iter()),
            None
        );
    }
}
