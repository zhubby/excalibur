use std::thread::spawn;

use device_agent::{
    Action, ActionResponse, base::bridge::BridgeTx, collector::script_runner::ScriptRunner,
};
use flume::bounded;
use serde_json::json;

#[test]
fn empty_payload() {
    let (tx, _) = bounded(2);
    let (inner, status_rx) = bounded(2);
    let bridge_tx = BridgeTx { data_tx: tx, status_tx: inner };

    let (actions_tx, actions_rx) = bounded(1);
    let script_runner = ScriptRunner::new(actions_rx, bridge_tx);
    spawn(move || script_runner.start().unwrap());

    actions_tx
        .send(Action { action_id: "1".to_string(), name: "test".to_string(), payload: json!("") })
        .unwrap();

    let ActionResponse { state, errors, .. } = status_rx.recv().unwrap();
    assert_eq!(state, "Failed");
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("Failed to deserialize action payload"));
    assert!(errors[0].contains("invalid type: string"));
}

#[test]
fn missing_path() {
    let (tx, _) = bounded(2);
    let (inner, status_rx) = bounded(2);
    let bridge_tx = BridgeTx { data_tx: tx, status_tx: inner };

    let (actions_tx, actions_rx) = bounded(1);
    let script_runner = ScriptRunner::new(actions_rx, bridge_tx);

    spawn(move || script_runner.start().unwrap());

    actions_tx
        .send(Action {
            action_id: "1".to_string(),
            name: "test".to_string(),
            payload: json!({
                "url": "...",
                "content_length": 0,
                "file_name": "..."
            }),
        })
        .unwrap();

    let ActionResponse { state, errors, .. } = status_rx.recv().unwrap();
    assert_eq!(state, "Failed");
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("Action payload doesn't contain path for script execution"));
    assert!(errors[0].contains("\"url\""));
}
