use std::{
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use flume::bounded;
use reqwest::ClientBuilder;
use serde_json::json;
use tempdir::TempDir;

use device_agent::{
    Action, ActionResponse,
    base::bridge::BridgeTx,
    collector::downloader::{DownloadFile, FileDownloader},
    device_agent_config::{ActionRoute, Config, DownloaderConfig},
};

const TEST_BODY: &[u8] = b"excalibur-device-agent-test";
const TEST_BODY_SHA256: &str = "d1e406242f2cee5eddd479480aa283dc1e85a8424c6c1eafb50fe0fe792bbfaf";

fn local_file_url(expected_requests: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    std::thread::spawn(move || {
        for _ in 0..expected_requests {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut request = [0; 1024];
            let _ = stream.read(&mut request);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                TEST_BODY.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(TEST_BODY);
        }
    });

    format!("http://{addr}/firmware.bin")
}

// Prepare config
fn test_config(temp_dir: &Path, test_name: &str) -> Config {
    let mut path = PathBuf::from(temp_dir);
    path.push(test_name);
    let mut config = Config::default();
    config.downloader =
        DownloaderConfig { actions: vec![ActionRoute { name: "ota.install".to_owned() }], path };

    config
}

fn recv_status(rx: &flume::Receiver<ActionResponse>) -> ActionResponse {
    rx.recv_timeout(Duration::from_secs(30)).expect("timed out waiting for downloader status")
}

#[test]
// Test file downloading capabilities of FileDownloader with a local HTTP fixture.
fn download_file() {
    let temp_dir = TempDir::new("download_file").unwrap();
    let config = test_config(temp_dir.path(), "download_file");
    let mut downloader_path = config.downloader.path.clone();
    let url = local_file_url(1);

    let (tx, _) = bounded(2);
    let (inner, status_rx) = bounded(2);
    let bridge_tx = BridgeTx { data_tx: tx, status_tx: inner };

    // Create channels to forward and push actions on
    let (download_tx, download_rx) = bounded(1);
    let (_, ctrl_rx) = bounded(1);
    let downloader = FileDownloader::with_client(
        Arc::new(config),
        ClientBuilder::new().no_proxy().build().unwrap(),
        download_rx,
        bridge_tx,
        ctrl_rx,
        Arc::new(Mutex::new(false)),
    );

    // Start FileDownloader in separate thread
    std::thread::spawn(move || downloader.start());

    // Create an OTA install action using the Excalibur command wire shape.
    let download_update = json!({
        "firmware_id": "018f4c5c-9b4d-7cc2-a62a-44590f672000",
        "component": "main",
        "version": "1.2.3",
        "signed_url": url,
        "size_bytes": TEST_BODY.len(),
        "sha256": TEST_BODY_SHA256
    });
    let mut expected_forward = DownloadFile {
        url: download_update["signed_url"].as_str().unwrap().to_owned(),
        content_length: TEST_BODY.len(),
        file_name: "main-1.2.3.bin".to_string(),
        download_path: None,
        checksum: Some(TEST_BODY_SHA256.to_owned()),
    };
    downloader_path.push("ota.install");
    downloader_path.push("main-1.2.3.bin");
    expected_forward.download_path = Some(downloader_path);
    let download_action = Action {
        action_id: "1".to_string(),
        name: "ota.install".to_string(),
        payload: download_update,
    };

    std::thread::sleep(Duration::from_millis(10));

    // Send action to FileDownloader with Sender<Action>
    download_tx.try_send(download_action).unwrap();

    // Collect action_status and ensure it is as expected
    let status = recv_status(&status_rx);
    assert_eq!(status.state, "Downloading");
    let mut progress = 0;

    // Collect and ensure forwarded action contains expected info
    loop {
        let status = recv_status(&status_rx);

        assert!(progress <= status.progress);
        progress = status.progress;

        if status.is_done() {
            let fwd_action = status.done_response.unwrap();
            let fwd = fwd_action.payload_as().unwrap();
            assert_eq!(expected_forward, fwd);
            break;
        }
    }
}

#[test]
// Once a file is downloaded FileDownloader must check it's checksum value against what is provided
fn checksum_of_file() {
    let temp_dir = TempDir::new("file_checksum").unwrap();
    let config = test_config(temp_dir.path(), "file_checksum");
    let url = local_file_url(2);

    let (tx, _) = bounded(2);
    let (inner, status_rx) = bounded(2);
    let bridge_tx = BridgeTx { data_tx: tx, status_tx: inner };

    // Create channels to forward and push action_status on
    let (download_tx, download_rx) = bounded(1);
    let (_, ctrl_rx) = bounded(1);
    let downloader = FileDownloader::with_client(
        Arc::new(config),
        ClientBuilder::new().no_proxy().build().unwrap(),
        download_rx,
        bridge_tx,
        ctrl_rx,
        Arc::new(Mutex::new(false)),
    );

    // Start FileDownloader in separate thread
    std::thread::spawn(move || downloader.start());

    std::thread::sleep(Duration::from_millis(10));

    // Correct firmware update action
    let correct_update = DownloadFile {
        url: url.clone(),
        content_length: TEST_BODY.len(),
        file_name: "logo.png".to_string(),
        download_path: None,
        checksum: Some(TEST_BODY_SHA256.to_string()),
    };
    let correct_action = Action {
        action_id: "1".to_string(),
        name: "ota.install".to_string(),
        payload: json!(correct_update),
    };

    // Send the correct action to FileDownloader
    download_tx.try_send(correct_action).unwrap();

    // Collect action_status and ensure it is as expected
    let status = recv_status(&status_rx);
    assert_eq!(status.state, "Downloading");
    let mut progress = 0;

    // Collect and ensure forwarded action contains expected info
    loop {
        let status = recv_status(&status_rx);

        assert!(progress <= status.progress);
        progress = status.progress;

        if status.is_done() {
            if status.state != "Downloaded" {
                panic!("unexpected status={status:?}")
            }
            break;
        }
    }

    // Wrong firmware update action
    let wrong_update = DownloadFile {
        url,
        content_length: TEST_BODY.len(),
        file_name: "logo.png".to_string(),
        download_path: None,
        checksum: Some("abcd1234efgh5678".to_string()),
    };
    let wrong_action = Action {
        action_id: "1".to_string(),
        name: "ota.install".to_string(),
        payload: json!(wrong_update),
    };

    // Send the wrong action to FileDownloader
    download_tx.try_send(wrong_action).unwrap();

    // Collect action_status and ensure it is as expected
    let status = recv_status(&status_rx);
    assert_eq!(status.state, "Downloading");
    let mut progress = 0;

    // Collect and ensure forwarded action contains expected info
    loop {
        let status = recv_status(&status_rx);

        assert!(progress <= status.progress);
        progress = status.progress;

        if status.is_done() {
            assert!(status.is_failed());
            assert_eq!(status.errors, vec!["Downloaded file has unexpected checksum"]);
            break;
        }
    }
}
