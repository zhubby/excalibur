# Provisioning 与证书

Excalibur 支持两条 provisioning 路径：生产 CSR 路径和开发 dev-auth 路径。

## 生产 CSR 路径

生产设备必须在设备本地生成私钥，私钥不离开设备。

流程：

1. 后台或工厂系统调用 API 创建设备：

```http
POST /api/v1/devices
```

2. 设备本地生成 keypair：

```bash
openssl genrsa -out /etc/excalibur/device.key 4096
openssl req -new -key /etc/excalibur/device.key -out /tmp/device.csr
```

3. 设备或工厂 provisioning 服务提交 CSR：

```http
POST /api/v1/devices/{device_id}/provision/csr
```

4. API 返回 auth JSON：

```json
{
  "broker": "mqtt.local.excalibur.dev",
  "port": 8883,
  "project_id": "018f4c5c-9b4d-7cc2-a62a-44590f671001",
  "device_id": "018f4c5c-9b4d-7cc2-a62a-44590f671101",
  "certificate_id": "018f4c5c-9b4d-7cc2-a62a-44590f671201",
  "certificate_fingerprint_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "certificate_not_after": "2027-07-30T08:30:00Z",
  "authentication": {
    "ca_certificate": "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----",
    "device_certificate": "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----",
    "device_private_key_path": "/etc/excalibur/device.key"
  },
  "provisioning_mode": "Csr",
  "production": true
}
```

5. 设备将 auth JSON 保存为 `/etc/excalibur/auth.json`。
6. `device-agent` 启动后默认先通过 Tailscale 上游发现定位 Excalibur server，再使用 mTLS 连接 MQTT；如果 discovery 不可用且 auth JSON 包含 `broker`/`port`，则回退到该静态地址。

当前 API 会解析 CSR SubjectPublicKeyInfo，签发真实可解析 X.509 设备证书，计算 certificate fingerprint，并把 active certificate 记录持久化。生产必须通过 `EXCALIBUR_CA_PRIVATE_KEY_PEM` 或 Helm Secret 注入真实 CA key；只有显式设置 `EXCALIBUR_ALLOW_DEV_CA=true` 时才会使用内置 dev CA。

API 可以继续在 auth JSON 中返回 `broker` 和 `port`，用于旧 agent 或 discovery 失败时的 fallback。启用 `[upstream_discovery] enabled = true` 的新 agent 允许省略这两个字段，并会用发现到的 Tailscale IPv4 地址覆盖内存中的 broker host。

## Tailscale 上游发现要求

生产 tailnet 需要给 Excalibur server 节点配置 Tailscale tag：

```text
tag:excalibur-server
```

agent 只选择同一 tailnet 中在线、非本机、带该 tag 的 peer，并要求：

- 本机 `tailscaled` backend state 为 `Running`。
- server peer 的 `http://<tailscale-ip>:8080/ready` 返回成功。
- server peer 的 MQTT TCP port 可连接。

mTLS auth 存在时，agent 默认探测 MQTT `8883`；没有 mTLS auth 时默认探测 `1883`。若没有 tagged peer 通过探测但 auth JSON 提供了 `broker`/`port`，agent 会按旧行为连接静态 broker。若多个 tagged peer 同时通过探测，v1 会启动失败，即使存在静态 fallback 也不继续，以避免掩盖错误 tag 或 split-brain 拓扑；后续 HA/failover 再单独设计。由于 discovery 使用 Tailscale IPv4 作为 broker host，mTLS server certificate 需要包含该 IPv4 SAN，或部署侧需要提供与证书匹配的 TLS server name 策略。

## Dev auth 路径

开发和批量实验可以调用：

```http
POST /api/v1/devices/{device_id}/provision/dev-auth
```

返回值包含 inline private key：

```json
{
  "certificate_fingerprint_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
  "authentication": {
    "ca_certificate": "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----",
    "device_certificate": "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----",
    "device_private_key": "-----BEGIN PRIVATE KEY-----\n...\n-----END PRIVATE KEY-----"
  },
  "provisioning_mode": "DevGeneratedKeypair",
  "production": false
}
```

该路径不应进入生产，因为私钥由服务端生成并通过 API 返回。

## 证书生命周期

领域模型 `DeviceCertificate` 包含：

- `project_id`
- `device_id`
- `fingerprint_sha256`
- `status`: `Active`、`Revoked`、`Expired`
- `not_before`
- `not_after`

撤销接口：

```http
POST /api/v1/devices/{device_id}/certificates/{certificate_id}/revoke?project_id=...
```

生产 broker connect hook 必须检查：

- fingerprint 存在。
- certificate status 是 `Active`。
- 当前时间在 `not_before` 和 `not_after` 内。
- device status 不是 `Disabled`。
- topic project/device 与证书绑定一致。

## 文件权限建议

Linux 设备上推荐：

```text
/etc/excalibur/auth.json       root:root 0640
/etc/excalibur/device.key      root:root 0600
/etc/excalibur/device-agent.toml root:root 0644
/var/lib/excalibur-agent/      device-agent user writable
```

systemd service 应以最小权限用户运行，只给需要访问的设备文件、日志、OTA 路径和本机 tailscaled socket 授权。常见 socket 路径是 `/var/run/tailscale/tailscaled.sock` 或 `/run/tailscale/tailscaled.sock`。
