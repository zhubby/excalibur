use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

use chrono::{DateTime, Utc};
use flume::Receiver;
use futures_util::{SinkExt, StreamExt};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::device_agent_config::RemoteShellConfig;
use crate::{Action, ActionResponse, base::bridge::BridgeTx};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("invalid remote shell payload: {0}")]
    InvalidPayload(String),
    #[error("remote shell payload error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("remote shell websocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("remote shell pty error: {0}")]
    Pty(#[from] anyhow::Error),
    #[error("remote shell io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("remote shell task error: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("remote shell session expired")]
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteShellOpenPayload {
    pub session_id: String,
    pub websocket_url: String,
    pub expires_at: DateTime<Utc>,
}

impl RemoteShellOpenPayload {
    pub fn validate(&self) -> Result<(), Error> {
        if self.session_id.trim().is_empty() {
            return Err(Error::InvalidPayload("session_id is required".to_owned()));
        }
        if !(self.websocket_url.starts_with("ws://") || self.websocket_url.starts_with("wss://")) {
            return Err(Error::InvalidPayload("websocket_url must use ws:// or wss://".to_owned()));
        }
        if self.expires_at <= Utc::now() {
            return Err(Error::Expired);
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct RemoteShellClient {
    config: RemoteShellConfig,
    actions_rx: Receiver<Action>,
    bridge: BridgeTx,
}

impl RemoteShellClient {
    pub fn new(config: RemoteShellConfig, actions_rx: Receiver<Action>, bridge: BridgeTx) -> Self {
        Self { config, actions_rx, bridge }
    }

    #[tokio::main(flavor = "current_thread")]
    pub async fn start(self) {
        while let Ok(action) = self.actions_rx.recv_async().await {
            let session = self.clone();
            tokio::spawn(async move {
                session
                    .bridge
                    .send_action_response(ActionResponse::progress(
                        &action.action_id,
                        "ShellConnecting",
                        5,
                    ))
                    .await;

                match session.session(&action).await {
                    Ok(()) => {
                        log::info!("remote shell session finished");
                        session
                            .bridge
                            .send_action_response(ActionResponse::success(&action.action_id))
                            .await;
                    }
                    Err(error) => {
                        log::error!("remote shell session ended with an error: {error:?}");
                        session
                            .bridge
                            .send_action_response(ActionResponse::failure(
                                &action.action_id,
                                error.to_string(),
                            ))
                            .await;
                    }
                }
            });
        }
    }

    async fn session(&self, action: &Action) -> Result<(), Error> {
        let payload: RemoteShellOpenPayload = action.payload_as()?;
        payload.validate()?;

        let (ws_stream, _) = connect_async(&payload.websocket_url).await?;
        let (mut ws_tx, mut ws_rx) = ws_stream.split();
        let mut pty = PtySession::spawn(self.config.shell.clone())?;
        self.bridge
            .send_action_response(ActionResponse::progress(&action.action_id, "ShellActive", 10))
            .await;

        let expires_after =
            (payload.expires_at - Utc::now()).to_std().map_err(|_| Error::Expired)?;
        let expiry = tokio::time::sleep(expires_after);
        tokio::pin!(expiry);

        loop {
            tokio::select! {
                _ = &mut expiry => {
                    pty.kill();
                    return Err(Error::Expired);
                }
                output = pty.output_rx.recv() => {
                    match output {
                        Some(bytes) => ws_tx.send(Message::Binary(bytes.into())).await?,
                        None => break,
                    }
                }
                message = ws_rx.next() => {
                    match message {
                        Some(Ok(Message::Text(text))) => {
                            pty.write(text.to_string().into_bytes()).await?;
                        }
                        Some(Ok(Message::Binary(bytes))) => {
                            pty.write(bytes.to_vec()).await?;
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            break;
                        }
                        Some(Ok(Message::Ping(bytes))) => {
                            ws_tx.send(Message::Pong(bytes)).await?;
                        }
                        Some(Ok(Message::Pong(_))) => {}
                        Some(Ok(Message::Frame(_))) => {}
                        Some(Err(error)) => return Err(Error::WebSocket(error)),
                    }
                }
            }
        }

        pty.kill();
        let _ = ws_tx.send(Message::Close(None)).await;
        Ok(())
    }
}

struct PtySession {
    output_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    input: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl PtySession {
    fn spawn(shell: PathBuf) -> Result<Self, Error> {
        let pty_system = native_pty_system();
        let pair =
            pty_system.openpty(PtySize { rows: 30, cols: 120, pixel_width: 0, pixel_height: 0 })?;
        let shell = shell.to_string_lossy().to_string();
        let command = CommandBuilder::new(shell);
        let child = pair.slave.spawn_command(command)?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let input = Arc::new(Mutex::new(pair.master.take_writer()?));
        let (output_tx, output_rx) = mpsc::unbounded_channel();
        thread::Builder::new().name("Remote Shell PTY Reader".to_owned()).spawn(move || {
            let mut buffer = [0_u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        if output_tx.send(buffer[..read].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        log::warn!("remote shell pty read failed: {error}");
                        break;
                    }
                }
            }
        })?;

        Ok(Self { output_rx, input, child })
    }

    async fn write(&self, bytes: Vec<u8>) -> Result<(), Error> {
        let input = self.input.clone();
        tokio::task::spawn_blocking(move || {
            let mut input = input
                .lock()
                .map_err(|_| std::io::Error::other("remote shell pty input lock poisoned"))?;
            input.write_all(&bytes)?;
            input.flush()
        })
        .await??;
        Ok(())
    }

    fn kill(&mut self) {
        let _ = self.child.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_shell_payload_validation_rejects_missing_fields() {
        let payload = RemoteShellOpenPayload {
            session_id: "".to_owned(),
            websocket_url: "ws://localhost:8080/session".to_owned(),
            expires_at: Utc::now() + chrono::Duration::minutes(5),
        };

        assert!(matches!(payload.validate(), Err(Error::InvalidPayload(_))));
    }

    #[test]
    fn remote_shell_payload_validation_rejects_non_ws_urls() {
        let payload = RemoteShellOpenPayload {
            session_id: "session-1".to_owned(),
            websocket_url: "http://localhost:8080/session".to_owned(),
            expires_at: Utc::now() + chrono::Duration::minutes(5),
        };

        assert!(matches!(payload.validate(), Err(Error::InvalidPayload(_))));
    }

    #[test]
    fn remote_shell_payload_validation_rejects_expired_sessions() {
        let payload = RemoteShellOpenPayload {
            session_id: "session-1".to_owned(),
            websocket_url: "ws://localhost:8080/session".to_owned(),
            expires_at: Utc::now() - chrono::Duration::seconds(1),
        };

        assert!(matches!(payload.validate(), Err(Error::Expired)));
    }
}
