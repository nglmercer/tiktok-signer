//! Broker compatibility wrapper; identity acquisition is shared with native.
use crate::metrics::Metrics;
use std::{sync::atomic::Ordering, time::Duration};
pub use ttl_live_discovery::identity::{GuestIdentity, IdentityError};
pub async fn bootstrap_guest_identity(
    user_agent: &str,
    timeout: Duration,
    metrics: Option<&Metrics>,
) -> Result<GuestIdentity, IdentityError> {
    if let Some(m) = metrics {
        m.guest_bootstrap_total.fetch_add(1, Ordering::Relaxed);
    }
    let result = ttl_live_discovery::identity::bootstrap_guest_identity(user_agent, timeout).await;
    if result.is_err() {
        if let Some(m) = metrics {
            m.guest_bootstrap_failures_total
                .fetch_add(1, Ordering::Relaxed);
        }
    }
    result
}
