use std::{
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, bail};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

const DEFAULT_IO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NatsUrl {
    pub host: String,
    pub port: u16,
}

impl NatsUrl {
    pub fn parse(input: &str) -> anyhow::Result<Self> {
        let without_scheme = input
            .strip_prefix("nats://")
            .ok_or_else(|| anyhow::anyhow!("NATS_URL must start with nats://"))?;
        let authority = without_scheme
            .split('/')
            .next()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("NATS_URL is missing host"))?;
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => {
                let port = port
                    .parse()
                    .with_context(|| format!("NATS_URL port is invalid: {port}"))?;
                (host.to_owned(), port)
            }
            None => (authority.to_owned(), 4222),
        };
        if host.trim().is_empty() {
            bail!("NATS_URL is missing host");
        }
        Ok(Self { host, port })
    }

    fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Debug, Clone)]
pub struct NatsClient {
    url: NatsUrl,
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JetStreamPubAck {
    pub stream: String,
    pub sequence: u64,
}

impl NatsClient {
    pub fn new(url: impl AsRef<str>, name: impl Into<String>) -> anyhow::Result<Self> {
        Ok(Self {
            url: NatsUrl::parse(url.as_ref())?,
            name: name.into(),
        })
    }

    pub async fn publish(&self, subject: &str, payload: &[u8]) -> anyhow::Result<()> {
        validate_subject(subject)?;
        tokio::time::timeout(DEFAULT_IO_TIMEOUT, async {
            let mut connection = NatsConnection::connect(&self.url, &self.name).await?;
            connection.publish(subject, payload).await?;
            connection.flush().await
        })
        .await
        .context("timed out publishing to NATS")?
    }

    pub async fn publish_jetstream(
        &self,
        subject: &str,
        payload: &[u8],
        timeout: Duration,
    ) -> anyhow::Result<JetStreamPubAck> {
        let response = self.request(subject, payload, timeout).await?;
        parse_jetstream_pub_ack(&response.payload)
    }

    pub async fn ack(&self, ack_subject: &str) -> anyhow::Result<()> {
        validate_subject(ack_subject)?;
        tokio::time::timeout(DEFAULT_IO_TIMEOUT, async {
            let mut connection = NatsConnection::connect(&self.url, &self.name).await?;
            connection.publish(ack_subject, b"+ACK").await?;
            connection.flush().await
        })
        .await
        .context("timed out acknowledging NATS message")?
    }

    pub async fn request(
        &self,
        subject: &str,
        payload: &[u8],
        timeout: Duration,
    ) -> anyhow::Result<NatsMessage> {
        validate_subject(subject)?;
        tokio::time::timeout(timeout, async {
            let mut connection = NatsConnection::connect(&self.url, &self.name).await?;
            let inbox = next_inbox_subject();
            connection
                .write_raw(format!("SUB {inbox} 1\r\n").as_bytes())
                .await?;
            connection
                .publish_with_reply(subject, &inbox, payload)
                .await?;
            connection.write_raw(b"PING\r\n").await?;
            loop {
                let message = connection.next_message().await?;
                if message.subject == inbox {
                    return Ok(message);
                }
            }
        })
        .await
        .context("timed out waiting for NATS request reply")?
    }

    pub async fn subscribe(
        &self,
        subject: &str,
        queue_group: Option<&str>,
    ) -> anyhow::Result<NatsSubscription> {
        validate_subject(subject)?;
        if let Some(queue_group) = queue_group {
            validate_subject(queue_group)?;
        }
        tokio::time::timeout(DEFAULT_IO_TIMEOUT, async {
            let mut connection = NatsConnection::connect(&self.url, &self.name).await?;
            let sid = "1";
            let command = match queue_group {
                Some(queue_group) => format!("SUB {subject} {queue_group} {sid}\r\n"),
                None => format!("SUB {subject} {sid}\r\n"),
            };
            connection.write_raw(command.as_bytes()).await?;
            connection.flush().await?;
            Ok(NatsSubscription { connection })
        })
        .await
        .context("timed out subscribing to NATS")?
    }
}

pub struct NatsSubscription {
    connection: NatsConnection,
}

impl NatsSubscription {
    pub async fn next_message(&mut self) -> anyhow::Result<NatsMessage> {
        self.connection.next_message().await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NatsMessage {
    pub subject: String,
    pub reply: Option<String>,
    pub payload: Vec<u8>,
}

struct NatsConnection {
    reader: BufReader<TcpStream>,
}

impl NatsConnection {
    async fn connect(url: &NatsUrl, name: &str) -> anyhow::Result<Self> {
        let stream = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(url.addr()))
            .await
            .context("timed out connecting to NATS")?
            .context("failed to connect to NATS")?;
        let mut connection = Self {
            reader: BufReader::new(stream),
        };
        let mut line = String::new();
        connection
            .reader
            .read_line(&mut line)
            .await
            .context("failed reading NATS INFO")?;
        if !line.starts_with("INFO ") {
            bail!("expected NATS INFO line, got: {}", line.trim());
        }
        connection
            .write_raw(format!("CONNECT {{\"name\":\"{name}\",\"verbose\":false,\"pedantic\":false,\"lang\":\"rust\",\"version\":\"0.1\",\"protocol\":1}}\r\n").as_bytes())
            .await?;
        Ok(connection)
    }

    async fn publish(&mut self, subject: &str, payload: &[u8]) -> anyhow::Result<()> {
        self.write_raw(format!("PUB {subject} {}\r\n", payload.len()).as_bytes())
            .await?;
        self.write_raw(payload).await?;
        self.write_raw(b"\r\n").await
    }

    async fn publish_with_reply(
        &mut self,
        subject: &str,
        reply: &str,
        payload: &[u8],
    ) -> anyhow::Result<()> {
        self.write_raw(format!("PUB {subject} {reply} {}\r\n", payload.len()).as_bytes())
            .await?;
        self.write_raw(payload).await?;
        self.write_raw(b"\r\n").await
    }

    async fn flush(&mut self) -> anyhow::Result<()> {
        self.write_raw(b"PING\r\n").await?;
        loop {
            let line = self.read_control_line().await?;
            match line.as_str() {
                "PONG" => return Ok(()),
                "+OK" => {}
                line if line.starts_with("-ERR") => bail!("NATS error during flush: {line}"),
                _ => {}
            }
        }
    }

    async fn next_message(&mut self) -> anyhow::Result<NatsMessage> {
        loop {
            let line = self.read_control_line().await?;
            if line == "PING" {
                self.write_raw(b"PONG\r\n").await?;
                continue;
            }
            if line == "PONG" || line == "+OK" || line.starts_with("INFO ") {
                continue;
            }
            if line.starts_with("-ERR") {
                bail!("NATS error: {line}");
            }
            if let Some(message) = parse_msg_line(&line)? {
                let mut payload = vec![0u8; message.len];
                self.reader
                    .read_exact(&mut payload)
                    .await
                    .context("failed reading NATS MSG payload")?;
                let mut crlf = [0u8; 2];
                self.reader
                    .read_exact(&mut crlf)
                    .await
                    .context("failed reading NATS MSG terminator")?;
                if crlf != *b"\r\n" {
                    bail!("invalid NATS MSG terminator");
                }
                return Ok(NatsMessage {
                    subject: message.subject,
                    reply: message.reply,
                    payload,
                });
            }
        }
    }

    async fn read_control_line(&mut self) -> anyhow::Result<String> {
        let mut line = String::new();
        let bytes = self
            .reader
            .read_line(&mut line)
            .await
            .context("failed reading NATS control line")?;
        if bytes == 0 {
            bail!("NATS connection closed");
        }
        Ok(line.trim_end_matches(['\r', '\n']).to_owned())
    }

    async fn write_raw(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.reader
            .get_mut()
            .write_all(bytes)
            .await
            .context("failed writing to NATS")
    }
}

struct MsgFrame {
    subject: String,
    reply: Option<String>,
    len: usize,
}

fn parse_msg_line(line: &str) -> anyhow::Result<Option<MsgFrame>> {
    let parts = line.split_whitespace().collect::<Vec<_>>();
    if parts.first() != Some(&"MSG") {
        return Ok(None);
    }
    if parts.len() != 4 && parts.len() != 5 {
        bail!("invalid NATS MSG line: {line}");
    }
    let len_index = parts.len() - 1;
    let len = parts[len_index]
        .parse()
        .with_context(|| format!("invalid NATS MSG length: {}", parts[len_index]))?;
    Ok(Some(MsgFrame {
        subject: parts[1].to_owned(),
        reply: if parts.len() == 5 {
            Some(parts[3].to_owned())
        } else {
            None
        },
        len,
    }))
}

fn validate_subject(subject: &str) -> anyhow::Result<()> {
    if subject.trim().is_empty()
        || subject
            .chars()
            .any(|ch| ch.is_ascii_whitespace() || ch == '\r' || ch == '\n')
    {
        bail!("invalid NATS subject");
    }
    Ok(())
}

fn next_inbox_subject() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("_INBOX.excalibur.{}.{}.{}", process::id(), now, sequence)
}

fn parse_jetstream_pub_ack(payload: &[u8]) -> anyhow::Result<JetStreamPubAck> {
    let value = serde_json::from_slice::<serde_json::Value>(payload)
        .context("JetStream publish ack was not JSON")?;
    if let Some(error) = value.get("error") {
        let description = error
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown JetStream publish error");
        bail!("JetStream publish failed: {description}");
    }
    let stream = value
        .get("stream")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("JetStream publish ack is missing stream"))?
        .to_owned();
    let sequence = value
        .get("seq")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("JetStream publish ack is missing seq"))?;
    Ok(JetStreamPubAck { stream, sequence })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nats_url_defaults_port() {
        assert_eq!(
            NatsUrl::parse("nats://nats").unwrap(),
            NatsUrl {
                host: "nats".to_owned(),
                port: 4222
            }
        );
    }

    #[test]
    fn parses_msg_line_with_reply_subject() {
        let frame = parse_msg_line("MSG excalibur.telemetry 1 _INBOX.1 42")
            .unwrap()
            .unwrap();

        assert_eq!(frame.subject, "excalibur.telemetry");
        assert_eq!(frame.reply.as_deref(), Some("_INBOX.1"));
        assert_eq!(frame.len, 42);
    }

    #[test]
    fn rejects_subject_with_whitespace() {
        assert!(validate_subject("bad subject").is_err());
    }

    #[test]
    fn parses_jetstream_publish_ack() {
        let ack = parse_jetstream_pub_ack(br#"{"stream":"EXCALIBUR_TELEMETRY","seq":42}"#).unwrap();

        assert_eq!(
            ack,
            JetStreamPubAck {
                stream: "EXCALIBUR_TELEMETRY".to_owned(),
                sequence: 42
            }
        );
    }

    #[test]
    fn rejects_jetstream_publish_error_ack() {
        let error = parse_jetstream_pub_ack(
            br#"{"error":{"code":503,"description":"stream unavailable"}}"#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("JetStream publish failed"));
    }
}
