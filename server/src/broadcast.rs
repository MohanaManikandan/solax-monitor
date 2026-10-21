//! Pushes each new reading, and each change of poller state, to every open
//! live stream.

use std::sync::Arc;

use solax_api::LiveEvent;

use crate::api::AppState;
use crate::now;

/// Builds today's figures for the stream and sends them. With no stream
/// open, which is most of the day, nothing is built: a page that opens later
/// fetches the present itself. Returns whether anything was built.
pub async fn publish_reading(state: &AppState) -> bool {
    if state.updates.receiver_count() == 0 {
        return false;
    }
    match now::build_now(state).await {
        // An error here only means the last stream closed meanwhile.
        Ok(now) => {
            let _ = state.updates.send(Arc::new(LiveEvent::Reading(now)));
        }
        Err(error) => tracing::warn!(%error, "building the reading for the live stream failed"),
    }
    true
}

/// Tells every open stream that the poller has gone to sleep or woken up, so
/// a quiet night reads as rest rather than as a fault.
pub fn publish_poller_state(state: &AppState) {
    let _ = state.updates.send(Arc::new(LiveEvent::Poller(now::poller_state(state))));
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use solax_api::LiveEvent;

    use super::*;
    use crate::test_support::{self, SyntheticDay};

    #[tokio::test]
    async fn a_new_reading_reaches_the_stream() {
        let (_folder, state) = test_support::app_state();
        let mut receiver = state.updates.subscribe();
        let sample =
            SyntheticDay::clear(NaiveDate::from_ymd_opt(2026, 9, 20).unwrap()).samples()[84].clone();
        let ts = sample.ts;
        state.store.insert_sample(sample).await.unwrap();

        assert!(publish_reading(&state).await);

        let event = receiver.recv().await.unwrap();
        let LiveEvent::Reading(now) = event.as_ref() else {
            panic!("expected a reading event");
        };
        assert_eq!(now.reading.as_ref().unwrap().ts, ts);
    }

    #[tokio::test]
    async fn with_no_stream_open_a_reading_builds_nothing() {
        let (_folder, state) = test_support::app_state();
        let sample =
            SyntheticDay::clear(NaiveDate::from_ymd_opt(2026, 9, 20).unwrap()).samples()[84].clone();
        state.store.insert_sample(sample).await.unwrap();

        assert!(!publish_reading(&state).await);
    }

    #[tokio::test]
    async fn a_poller_change_reaches_the_stream() {
        let (_folder, state) = test_support::app_state();
        let mut receiver = state.updates.subscribe();

        publish_poller_state(&state);

        let event = receiver.recv().await.unwrap();
        assert!(matches!(event.as_ref(), LiveEvent::Poller(_)));
    }
}
