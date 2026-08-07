// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! HTTP transport — ureq, sync, called only from the worker thread.

use std::time::Duration;

use crate::config::OtlpConfig;

#[derive(Debug)]
pub enum SendOutcome {
    Accepted,
    Retry(String),
    Drop(String),
}

pub fn send_batch(
    agent: &ureq::Agent,
    config: &OtlpConfig,
    body: &serde_json::Value,
) -> SendOutcome {
    let payload = match serde_json::to_string(body) {
        Ok(s) => s,
        Err(e) => return SendOutcome::Drop(format!("serialize failed: {e}")),
    };

    let mut req = agent
        .post(&config.endpoint)
        .header("Content-Type", "application/json")
        .header("User-Agent", &config.user_agent);
    for (name, value) in &config.headers {
        req = req.header(name.as_str(), value.as_str());
    }
    let result = req.send(payload.as_bytes());

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

pub fn build_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into()
}

pub fn next_backoff(current: Duration, max: Duration) -> Duration {
    let next = current.saturating_mul(2);
    if next > max { max } else { next }
}
