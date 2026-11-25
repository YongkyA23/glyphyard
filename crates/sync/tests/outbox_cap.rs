//! The outbox cap, in its own test binary: the trim is skipped while any client in the process flushes
//! (a process-wide lock), and the tests in `client.rs` flush in parallel.

use chrono::Utc;
use typeshelf_protocol::{Event, TimedEvent};
use typeshelf_sync::{Config, SyncClient};

fn event(changes: u32) -> TimedEvent {
    TimedEvent { at: Utc::now(), event: Event::OrganizeApplied { changes } }
}

/// The outbox keeps the newest 10,000 events; older ones are dropped.
#[tokio::test]
async fn the_outbox_is_capped() {
    let dir = tempfile::tempdir().unwrap();
    let outbox = dir.path().join("outbox.jsonl");
    let client = SyncClient::new(Config {
        server_url: "http://127.0.0.1:9".into(),
        install_key: "k".into(),
        token: None,
        outbox_path: outbox.clone(),
    });
    client.queue_events((0..10_000).map(event).collect()).await.unwrap();
    client.queue_events(vec![event(10_000), event(10_001)]).await.unwrap();
    let left: Vec<u32> = std::fs::read_to_string(&outbox)
        .unwrap()
        .lines()
        .map(|l| match serde_json::from_str::<TimedEvent>(l).unwrap().event {
            Event::OrganizeApplied { changes } => changes,
            e => panic!("{e:?}"),
        })
        .collect();
    assert_eq!(left.len(), 10_000);
    assert_eq!((left[0], left[9_999]), (2, 10_001), "the two oldest went");
}
