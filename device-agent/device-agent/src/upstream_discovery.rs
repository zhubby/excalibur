use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Error, anyhow, bail};
use log::{info, warn};
use reqwest::Client;
use tailscale_localapi::{BackendState, LocalApi, Status};
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::device_agent_config::{Config, DeviceConfig, UpstreamDiscoveryConfig};

const DEFAULT_TAILSCALE_SOCKET_PATHS: &[&str] =
    &["/var/run/tailscale/tailscaled.sock", "/run/tailscale/tailscaled.sock"];

#[derive(Debug, thiserror::Error)]
enum DiscoveryError {
    #[error("multiple Tailscale peers with tag {server_tag} passed probes: {hostnames}")]
    MultipleCandidates { server_tag: String, hostnames: String },
    #[error(transparent)]
    Other(#[from] Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiscoveryCandidate {
    hostname: String,
    dns_name: String,
    public_key: String,
    tailscale_ips: Vec<IpAddr>,
    tags: Vec<String>,
    online: bool,
    api_ready: bool,
    mqtt_ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiscoveredUpstream {
    broker: String,
    port: u16,
    hostname: String,
    dns_name: String,
}

pub(crate) fn has_static_upstream(device_config: &DeviceConfig) -> bool {
    !device_config.broker.trim().is_empty() && device_config.port != 0
}

pub(crate) fn resolve_upstream(
    config: &Config,
    mut device_config: DeviceConfig,
) -> Result<DeviceConfig, Error> {
    if !config.upstream_discovery.enabled {
        if has_static_upstream(&device_config) {
            return Ok(device_config);
        }
        bail!("auth JSON broker and port are required when upstream discovery is disabled");
    }

    match discover_upstream(&config.upstream_discovery, &device_config) {
        Ok(upstream) => {
            info!(
                "discovered Excalibur upstream via Tailscale: {} ({}) at {}:{}",
                upstream.hostname, upstream.dns_name, upstream.broker, upstream.port
            );
            device_config.broker = upstream.broker;
            device_config.port = upstream.port;
            Ok(device_config)
        }
        Err(error @ DiscoveryError::MultipleCandidates { .. }) => Err(Error::new(error)),
        Err(error) if has_static_upstream(&device_config) => {
            warn!("Tailscale upstream discovery failed; falling back to auth JSON broker: {error}");
            Ok(device_config)
        }
        Err(error) => Err(Error::new(error).context(
            "Tailscale upstream discovery failed and auth JSON does not contain broker/port fallback",
        )),
    }
}

fn discover_upstream(
    discovery_config: &UpstreamDiscoveryConfig,
    device_config: &DeviceConfig,
) -> Result<DiscoveredUpstream, DiscoveryError> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .context("failed to create Tailscale discovery runtime")?;

    rt.block_on(async {
        let status = tailscale_status(discovery_config).await?;
        ensure_tailscale_running(&status)?;
        let port = discovery_mqtt_port(discovery_config, device_config);
        let candidates = probe_candidates(
            &status,
            status.self_status.public_key.as_str(),
            discovery_config,
            port,
        )
        .await?;
        select_candidate(
            &candidates,
            status.self_status.public_key.as_str(),
            discovery_config.server_tag.as_str(),
            port,
        )
    })
}

fn discovery_mqtt_port(
    discovery_config: &UpstreamDiscoveryConfig,
    device_config: &DeviceConfig,
) -> u16 {
    if device_config.authentication.is_some() {
        discovery_config.mqtt_tls_port
    } else {
        discovery_config.mqtt_plaintext_port
    }
}

async fn tailscale_status(discovery_config: &UpstreamDiscoveryConfig) -> Result<Status, Error> {
    let socket_paths = socket_paths(discovery_config);
    let timeout_duration = Duration::from_millis(discovery_config.probe_timeout_ms);
    let mut last_error = None;
    for socket_path in socket_paths {
        let api = LocalApi::new_with_socket_path(&socket_path);
        match timeout(timeout_duration, api.status()).await {
            Ok(Ok(status)) => return Ok(status),
            Ok(Err(error)) => {
                last_error = Some(format!("{}: {error}", socket_path.display()));
            }
            Err(_) => {
                last_error = Some(format!(
                    "{}: timed out after {} ms",
                    socket_path.display(),
                    discovery_config.probe_timeout_ms
                ));
            }
        }
    }

    bail!(
        "could not read tailscaled LocalAPI status{}",
        last_error.map(|error| format!(" ({error})")).unwrap_or_default()
    )
}

fn socket_paths(discovery_config: &UpstreamDiscoveryConfig) -> Vec<PathBuf> {
    match &discovery_config.socket_path {
        Some(path) => vec![path.clone()],
        None => DEFAULT_TAILSCALE_SOCKET_PATHS.iter().map(PathBuf::from).collect(),
    }
}

fn ensure_tailscale_running(status: &Status) -> Result<(), Error> {
    match &status.backend_state {
        BackendState::Running => Ok(()),
        _ => bail!("tailscaled backend is not running"),
    }
}

async fn probe_candidates(
    status: &Status,
    local_public_key: &str,
    discovery_config: &UpstreamDiscoveryConfig,
    mqtt_port: u16,
) -> Result<Vec<DiscoveryCandidate>, Error> {
    let timeout_duration = Duration::from_millis(discovery_config.probe_timeout_ms);
    let client = Client::builder()
        .timeout(timeout_duration)
        .build()
        .context("failed to build Tailscale discovery HTTP client")?;
    let mut candidates = Vec::with_capacity(status.peer.len());

    for peer in status.peer.values() {
        if !peer.online
            || peer.public_key == local_public_key
            || !peer.tags.iter().any(|tag| tag == &discovery_config.server_tag)
        {
            continue;
        }

        let Some(ip) = preferred_ipv4(&peer.tailscale_ips) else {
            candidates.push(DiscoveryCandidate {
                hostname: peer.hostname.clone(),
                dns_name: peer.dnsname.clone(),
                public_key: peer.public_key.clone(),
                tailscale_ips: peer.tailscale_ips.clone(),
                tags: peer.tags.clone(),
                online: peer.online,
                api_ready: false,
                mqtt_ready: false,
            });
            continue;
        };

        let api_ready =
            probe_api_ready(&client, ip, discovery_config.api_ready_port).await.unwrap_or(false);
        let mqtt_ready = probe_mqtt(ip, mqtt_port, timeout_duration).await.unwrap_or(false);
        candidates.push(DiscoveryCandidate {
            hostname: peer.hostname.clone(),
            dns_name: peer.dnsname.clone(),
            public_key: peer.public_key.clone(),
            tailscale_ips: peer.tailscale_ips.clone(),
            tags: peer.tags.clone(),
            online: peer.online,
            api_ready,
            mqtt_ready,
        });
    }

    Ok(candidates)
}

async fn probe_api_ready(client: &Client, ip: IpAddr, port: u16) -> Result<bool, Error> {
    let response = client.get(format!("http://{ip}:{port}/ready")).send().await?;
    Ok(response.status().is_success())
}

async fn probe_mqtt(ip: IpAddr, port: u16, timeout_duration: Duration) -> Result<bool, Error> {
    let addr = SocketAddr::new(ip, port);
    Ok(timeout(timeout_duration, TcpStream::connect(addr)).await?.is_ok())
}

fn select_candidate(
    candidates: &[DiscoveryCandidate],
    local_public_key: &str,
    server_tag: &str,
    mqtt_port: u16,
) -> Result<DiscoveredUpstream, DiscoveryError> {
    let valid_candidates: Vec<&DiscoveryCandidate> = candidates
        .iter()
        .filter(|candidate| candidate.online)
        .filter(|candidate| candidate.public_key != local_public_key)
        .filter(|candidate| candidate.tags.iter().any(|tag| tag == server_tag))
        .filter(|candidate| candidate.api_ready && candidate.mqtt_ready)
        .filter(|candidate| preferred_ipv4(&candidate.tailscale_ips).is_some())
        .collect();

    match valid_candidates.as_slice() {
        [candidate] => {
            let ip = preferred_ipv4(&candidate.tailscale_ips).expect("candidate has IPv4");
            Ok(DiscoveredUpstream {
                broker: ip.to_string(),
                port: mqtt_port,
                hostname: candidate.hostname.clone(),
                dns_name: candidate.dns_name.clone(),
            })
        }
        [] => Err(anyhow!(
            "no online Tailscale peer with tag {server_tag} passed API and MQTT probes"
        )
        .into()),
        many => Err(DiscoveryError::MultipleCandidates {
            server_tag: server_tag.to_owned(),
            hostnames: many
                .iter()
                .map(|candidate| candidate.hostname.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        }),
    }
}

fn preferred_ipv4(ips: &[IpAddr]) -> Option<IpAddr> {
    ips.iter().copied().find(IpAddr::is_ipv4)
}

#[cfg(test)]
mod tests {
    use super::{
        DiscoveryCandidate, discovery_mqtt_port, probe_candidates, resolve_upstream,
        select_candidate,
    };
    use crate::device_agent_config::{
        Authentication, Config, DeviceConfig, UpstreamDiscoveryConfig,
    };
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    #[cfg(unix)]
    use std::os::unix::net::UnixListener;
    use std::path::Path;
    use std::thread;

    fn candidate(hostname: &str) -> DiscoveryCandidate {
        DiscoveryCandidate {
            hostname: hostname.to_owned(),
            dns_name: format!("{hostname}.tailnet.example."),
            public_key: format!("nodekey:{hostname}"),
            tailscale_ips: vec![
                IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1)),
                IpAddr::V6(Ipv6Addr::LOCALHOST),
            ],
            tags: vec!["tag:excalibur-server".to_owned()],
            online: true,
            api_ready: true,
            mqtt_ready: true,
        }
    }

    fn peer_status_json(hostname: &str, public_key: &str, ip: &str, tags: &str) -> String {
        format!(
            r#"{{
                "ID": "{hostname}",
                "PublicKey": "{public_key}",
                "HostName": "{hostname}",
                "DNSName": "{hostname}.tailnet.example.",
                "OS": "linux",
                "UserID": 1,
                "TailscaleIPs": ["{ip}"],
                "Tags": {tags},
                "Addrs": [],
                "CurAddr": "",
                "Relay": "",
                "RxBytes": 0,
                "TxBytes": 0,
                "Created": "2026-08-03T00:00:00Z",
                "LastWrite": "2026-08-03T00:00:00Z",
                "LastSeen": "2026-08-03T00:00:00Z",
                "LastHandshake": "2026-08-03T00:00:00Z",
                "Online": true,
                "ExitNode": false,
                "ExitNodeOption": false,
                "Active": true,
                "PeerAPIURL": [],
                "InNetworkMap": true,
                "InMagicSock": true,
                "InEngine": true
            }}"#
        )
    }

    fn running_status_json(peer_json: &str) -> String {
        running_status_json_with_peers(&format!(r#""nodekey:server": {peer_json}"#))
    }

    fn running_status_json_with_peers(peer_entries: &str) -> String {
        let self_json = peer_status_json("agent", "nodekey:agent", "100.64.0.2", "[]");
        format!(
            r#"{{
                "Version": "1.0.0",
                "BackendState": "Running",
                "AuthURL": "",
                "TailscaleIPs": ["100.64.0.2"],
                "Self": {self_json},
                "Health": [],
                "CurrentTailnet": null,
                "CertDomains": [],
                "Peer": {{
                    {peer_entries}
                }},
                "User": {{}}
            }}"#
        )
    }

    fn spawn_ready_server() -> u16 {
        spawn_ready_server_with_connections(1)
    }

    fn spawn_ready_server_with_connections(connections: usize) -> u16 {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            for _ in 0..connections {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut request = [0; 1024];
                let _ = stream.read(&mut request);
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
            }
        });
        port
    }

    fn spawn_tcp_probe_server() -> u16 {
        spawn_tcp_probe_server_with_connections(1)
    }

    fn spawn_tcp_probe_server_with_connections(connections: usize) -> u16 {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            for _ in 0..connections {
                if listener.accept().is_err() {
                    return;
                }
            }
        });
        port
    }

    #[cfg(unix)]
    fn spawn_localapi_status_server(socket_path: &Path, status_json: String) {
        let listener = UnixListener::bind(socket_path).unwrap();
        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                status_json.len(),
                status_json
            );
            let _ = stream.write_all(response.as_bytes());
        });
    }

    #[cfg(unix)]
    fn spawn_stalling_localapi_status_server(socket_path: &Path) {
        let listener = UnixListener::bind(socket_path).unwrap();
        thread::spawn(move || {
            let Ok((_stream, _)) = listener.accept() else {
                return;
            };
            thread::sleep(std::time::Duration::from_secs(5));
        });
    }

    #[test]
    fn selects_single_online_tagged_reachable_peer() {
        let selected =
            select_candidate(&[candidate("server")], "nodekey:agent", "tag:excalibur-server", 1883)
                .unwrap();

        assert_eq!(selected.broker, "100.64.0.1");
        assert_eq!(selected.port, 1883);
        assert_eq!(selected.hostname, "server");
    }

    #[test]
    fn probes_and_selects_single_online_tagged_reachable_peer() {
        let api_ready_port = spawn_ready_server();
        let mqtt_port = spawn_tcp_probe_server();
        let discovery_config = UpstreamDiscoveryConfig {
            api_ready_port,
            probe_timeout_ms: 1000,
            ..Default::default()
        };
        let peer_json = peer_status_json(
            "server",
            "nodekey:server",
            "127.0.0.1",
            r#"["tag:excalibur-server"]"#,
        );
        let status = serde_json::from_str(&running_status_json(&peer_json)).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .unwrap();

        let candidates = rt
            .block_on(probe_candidates(&status, "nodekey:agent", &discovery_config, mqtt_port))
            .unwrap();
        let selected =
            select_candidate(&candidates, "nodekey:agent", "tag:excalibur-server", mqtt_port)
                .unwrap();

        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].api_ready);
        assert!(candidates[0].mqtt_ready);
        assert_eq!(selected.broker, "127.0.0.1");
        assert_eq!(selected.port, mqtt_port);
        assert_eq!(selected.hostname, "server");
    }

    #[cfg(unix)]
    #[test]
    fn resolves_upstream_from_localapi_status_without_static_broker() {
        let temp_dir = tempdir::TempDir::new("device_agent").unwrap();
        let socket_path = temp_dir.path().join("tailscaled.sock");
        let api_ready_port = spawn_ready_server();
        let mqtt_port = spawn_tcp_probe_server();
        let peer_json = peer_status_json(
            "server",
            "nodekey:server",
            "127.0.0.1",
            r#"["tag:excalibur-server"]"#,
        );
        spawn_localapi_status_server(&socket_path, running_status_json(&peer_json));
        let mut config = Config::default();
        config.upstream_discovery.socket_path = Some(socket_path);
        config.upstream_discovery.api_ready_port = api_ready_port;
        config.upstream_discovery.mqtt_plaintext_port = mqtt_port;
        config.upstream_discovery.probe_timeout_ms = 1000;
        let device_config = DeviceConfig {
            project_id: "project-a".to_owned(),
            device_id: "device-1".to_owned(),
            ..Default::default()
        };

        let resolved = resolve_upstream(&config, device_config).unwrap();

        assert_eq!(resolved.broker, "127.0.0.1");
        assert_eq!(resolved.port, mqtt_port);
    }

    #[cfg(unix)]
    #[test]
    fn multiple_valid_peers_do_not_fall_back_to_static_upstream() {
        let temp_dir = tempdir::TempDir::new("device_agent").unwrap();
        let socket_path = temp_dir.path().join("tailscaled.sock");
        let api_ready_port = spawn_ready_server_with_connections(2);
        let mqtt_port = spawn_tcp_probe_server_with_connections(2);
        let server_a = peer_status_json(
            "server-a",
            "nodekey:server-a",
            "127.0.0.1",
            r#"["tag:excalibur-server"]"#,
        );
        let server_b = peer_status_json(
            "server-b",
            "nodekey:server-b",
            "127.0.0.1",
            r#"["tag:excalibur-server"]"#,
        );
        let peer_entries =
            format!(r#""nodekey:server-a": {server_a}, "nodekey:server-b": {server_b}"#);
        spawn_localapi_status_server(&socket_path, running_status_json_with_peers(&peer_entries));
        let mut config = Config::default();
        config.upstream_discovery.socket_path = Some(socket_path);
        config.upstream_discovery.api_ready_port = api_ready_port;
        config.upstream_discovery.mqtt_plaintext_port = mqtt_port;
        config.upstream_discovery.probe_timeout_ms = 1000;
        let device_config = DeviceConfig {
            project_id: "project-a".to_owned(),
            device_id: "device-1".to_owned(),
            broker: "mqtt.local".to_owned(),
            port: 1883,
            ..Default::default()
        };

        let error = resolve_upstream(&config, device_config).unwrap_err();

        assert!(error.to_string().contains("multiple Tailscale peers"));
        assert!(error.to_string().contains("server-a"));
        assert!(error.to_string().contains("server-b"));
    }

    #[cfg(unix)]
    #[test]
    fn localapi_timeout_falls_back_to_static_upstream() {
        let temp_dir = tempdir::TempDir::new("device_agent").unwrap();
        let socket_path = temp_dir.path().join("tailscaled.sock");
        spawn_stalling_localapi_status_server(&socket_path);
        let mut config = Config::default();
        config.upstream_discovery.socket_path = Some(socket_path);
        config.upstream_discovery.probe_timeout_ms = 20;
        let device_config = DeviceConfig {
            project_id: "project-a".to_owned(),
            device_id: "device-1".to_owned(),
            broker: "mqtt.local".to_owned(),
            port: 1883,
            ..Default::default()
        };

        let resolved = resolve_upstream(&config, device_config).unwrap();

        assert_eq!(resolved.broker, "mqtt.local");
        assert_eq!(resolved.port, 1883);
    }

    #[test]
    fn chooses_mqtt_port_from_authentication_mode() {
        let discovery_config = UpstreamDiscoveryConfig {
            mqtt_plaintext_port: 11883,
            mqtt_tls_port: 18883,
            ..Default::default()
        };
        let plaintext_device = DeviceConfig::default();
        let mtls_device = DeviceConfig {
            authentication: Some(Authentication {
                ca_certificate: "ca".to_owned(),
                device_certificate: "cert".to_owned(),
                device_private_key: Some("key".to_owned()),
                device_private_key_path: None,
            }),
            ..Default::default()
        };

        assert_eq!(discovery_mqtt_port(&discovery_config, &plaintext_device), 11883);
        assert_eq!(discovery_mqtt_port(&discovery_config, &mtls_device), 18883);
    }

    #[test]
    fn ignores_offline_peer() {
        let mut server = candidate("server");
        server.online = false;

        let error =
            select_candidate(&[server], "nodekey:agent", "tag:excalibur-server", 1883).unwrap_err();

        assert!(error.to_string().contains("no online Tailscale peer"));
    }

    #[test]
    fn ignores_untagged_peer() {
        let mut server = candidate("server");
        server.tags = vec![];

        let error =
            select_candidate(&[server], "nodekey:agent", "tag:excalibur-server", 1883).unwrap_err();

        assert!(error.to_string().contains("no online Tailscale peer"));
    }

    #[test]
    fn ignores_peer_that_fails_readiness_probe() {
        let mut server = candidate("server");
        server.api_ready = false;

        let error =
            select_candidate(&[server], "nodekey:agent", "tag:excalibur-server", 1883).unwrap_err();

        assert!(error.to_string().contains("passed API and MQTT probes"));
    }

    #[test]
    fn rejects_multiple_reachable_tagged_peers() {
        let error = select_candidate(
            &[candidate("server-a"), candidate("server-b")],
            "nodekey:agent",
            "tag:excalibur-server",
            1883,
        )
        .unwrap_err();

        assert!(error.to_string().contains("multiple Tailscale peers"));
    }

    #[test]
    fn ignores_local_node_even_if_tagged() {
        let server = candidate("server");

        let error = select_candidate(&[server], "nodekey:server", "tag:excalibur-server", 1883)
            .unwrap_err();

        assert!(error.to_string().contains("no online Tailscale peer"));
    }

    #[test]
    fn localapi_unavailable_requires_static_fallback() {
        let temp_dir = tempdir::TempDir::new("device_agent").unwrap();
        let mut config = Config::default();
        config.upstream_discovery.socket_path = Some(temp_dir.path().join("missing.sock"));
        let device_config = DeviceConfig {
            project_id: "project-a".to_owned(),
            device_id: "device-1".to_owned(),
            ..Default::default()
        };

        let error = resolve_upstream(&config, device_config).unwrap_err();

        assert!(error.to_string().contains("does not contain broker/port fallback"));
    }

    #[test]
    fn localapi_unavailable_falls_back_to_static_upstream() {
        let temp_dir = tempdir::TempDir::new("device_agent").unwrap();
        let mut config = Config::default();
        config.upstream_discovery.socket_path = Some(temp_dir.path().join("missing.sock"));
        let device_config = DeviceConfig {
            project_id: "project-a".to_owned(),
            device_id: "device-1".to_owned(),
            broker: "mqtt.local".to_owned(),
            port: 1883,
            ..Default::default()
        };

        let resolved = resolve_upstream(&config, device_config).unwrap();

        assert_eq!(resolved.broker, "mqtt.local");
        assert_eq!(resolved.port, 1883);
    }
}
