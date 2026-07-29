# API 示例

本页展示开发环境中从注册到 ingest telemetry 的最小请求链。示例使用当前 Bearer token scaffold。

## 注册

```bash
curl -s http://localhost:8080/api/v1/auth/register \
  -H 'content-type: application/json' \
  -d '{
    "email": "ops@example.com",
    "password": "correct horse battery staple",
    "display_name": "Ops"
  }'
```

响应：

```json
{
  "token": "019f...",
  "user_id": "019f..."
}
```

后续示例假设：

```bash
TOKEN=019f...
```

## 创建 org

```bash
curl -s http://localhost:8080/api/v1/orgs \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -d '{
    "name": "Northstar Mobility",
    "slug": "northstar"
  }'
```

## 创建 project

```bash
curl -s http://localhost:8080/api/v1/projects \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -d '{
    "org_id": "018f4c5c-9b4d-7cc2-a62a-44590f671000",
    "name": "Factory EV Line",
    "slug": "factory-ev-line"
  }'
```

## 创建设备

```bash
curl -s http://localhost:8080/api/v1/devices \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -d '{
    "project_id": "018f4c5c-9b4d-7cc2-a62a-44590f671001",
    "name": "press-line-a-017",
    "metadata": {
      "site": "shanghai",
      "line": "A"
    }
  }'
```

## CSR provisioning

```bash
curl -s http://localhost:8080/api/v1/devices/018f4c5c-9b4d-7cc2-a62a-44590f671101/provision/csr \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -d '{
    "project_id": "018f4c5c-9b4d-7cc2-a62a-44590f671001",
    "csr_pem": "-----BEGIN CERTIFICATE REQUEST-----\n...\n-----END CERTIFICATE REQUEST-----",
    "device_private_key_path": "/etc/excalibur/device.key"
  }'
```

## 创建 stream

```bash
curl -s http://localhost:8080/api/v1/streams \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -d '{
    "project_id": "018f4c5c-9b4d-7cc2-a62a-44590f671001",
    "name": "temperature",
    "fields": [
      { "name": "value", "field_type": "Float", "required": true }
    ]
  }'
```

## HTTP ingest 开发替身

真实设备应使用 MQTT。当前 API 提供 HTTP ingest，便于本地开发：

```bash
curl -s http://localhost:8080/api/v1/telemetry \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -d '{
    "topic": "v1/p/018f4c5c-9b4d-7cc2-a62a-44590f671001/d/018f4c5c-9b4d-7cc2-a62a-44590f671101/telemetry/temperature",
    "payload": [
      {
        "sequence": 1,
        "timestamp": "2026-07-29T08:30:00Z",
        "value": 24.6
      }
    ]
  }'
```

## 查询 telemetry

```bash
curl -s 'http://localhost:8080/api/v1/telemetry?project_id=018f4c5c-9b4d-7cc2-a62a-44590f671001&device_id=018f4c5c-9b4d-7cc2-a62a-44590f671101&stream=temperature&limit=100' \
  -H "authorization: Bearer $TOKEN"
```

## 创建 OTA action

```bash
curl -s http://localhost:8080/api/v1/actions \
  -H "authorization: Bearer $TOKEN" \
  -H 'content-type: application/json' \
  -d '{
    "project_id": "018f4c5c-9b4d-7cc2-a62a-44590f671001",
    "device_ids": ["018f4c5c-9b4d-7cc2-a62a-44590f671101"],
    "name": "ota.install",
    "payload": {
      "firmware_id": "018f4c5c-9b4d-7cc2-a62a-44590f671201",
      "component": "motor",
      "version": "3.2.1",
      "signed_url": "https://objects.example/firmware/motor-3.2.1.bin?sig=...",
      "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "size_bytes": 1048576
    }
  }'
```
