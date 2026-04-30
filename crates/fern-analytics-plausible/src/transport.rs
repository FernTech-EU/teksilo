//! HTTP transport via `ureq`.
//!
//! Synchronous send-and-await. Always called from the worker thread.
//! Errors are reported back via `SendOutcome` so the worker can
//! decide whether to retry, drop, or surface the failure.

use std::time::Duration;

use crate::config::PlausibleConfig;
use crate::wire::PlausibleEvent;

#[derive(Debug)]
pub enum SendOutcome {
    /// Plausible accepted the event (HTTP 200/202).
    Accepted,
    /// Recoverable — retry with exponential backoff. Includes
    /// network errors, 5xx, and 429.
    Retry(String),
    /// Unrecoverable — drop the event. Includes 4xx other than 429
    /// (malformed payload, wrong domain, etc.).
    Drop(String),
}

pub fn send_event(
    agent: &ureq::Agent,
    config: &PlausibleConfig,
    event: &PlausibleEvent<'_>,
) -> SendOutcome {
    let body = match serde_json::to_string(event) {
        Ok(s) => s,
        Err(e) => return SendOutcome::Drop(format!("serialize failed: {e}")),
    };

    let result = agent
        .post(&config.endpoint)
        .header("Content-Type", "application/json")
        .header("User-Agent", &config.user_agent)
        .send(body.as_bytes());

    match result {
        Ok(response) => {
            let status = response.status().as_u16();
            if (200..300).contains(&status) {
                SendOutcome::Accepted
            } else if status == 429 || (500..600).contains(&status) {
                SendOutcome::Retry(format!("HTTP {status}"))
            } else {
                SendOutcome::Drop(format!("HTTP {status}"))
            }
        }
        Err(ureq::Error::StatusCode(code)) => {
            if code == 429 || (500..600).contains(&code) {
                SendOutcome::Retry(format!("HTTP {code}"))
            } else {
                SendOutcome::Drop(format!("HTTP {code}"))
            }
        }
        Err(e) => SendOutcome::Retry(format!("transport: {e}")),
    }
}

/// Build the shared `ureq::Agent`. One per adapter — connection pool
/// is reused across requests by the same worker thread.
pub fn build_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into()
}

/// Backoff helper: doubles after each failure, capped at `max`.
pub fn next_backoff(current: Duration, max: Duration) -> Duration {
    let next = current.saturating_mul(2);
    if next > max { max } else { next }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_backoff_doubles_then_caps() {
        let max = Duration::from_secs(60);
        let d0 = Duration::from_secs(1);
        let d1 = next_backoff(d0, max);
        assert_eq!(d1, Duration::from_secs(2));
        let d2 = next_backoff(d1, max);
        assert_eq!(d2, Duration::from_secs(4));
        // Skip ahead — should saturate.
        let huge = Duration::from_secs(300);
        assert_eq!(next_backoff(huge, max), max);
    }
}
