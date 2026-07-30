#!/usr/bin/env bash
set -euo pipefail

API_BASE_URL="${API_BASE_URL:-http://localhost:8080}"
EMAIL="${EXCALIBUR_LOAD_EMAIL:-load-smoke@example.com}"
PASSWORD="${EXCALIBUR_LOAD_PASSWORD:-correct horse battery staple}"
DISPLAY_NAME="${EXCALIBUR_LOAD_DISPLAY_NAME:-Load Smoke}"
DEVICE_COUNT="${EXCALIBUR_LOAD_DEVICE_COUNT:-25}"
POINTS_PER_DEVICE="${EXCALIBUR_LOAD_POINTS_PER_DEVICE:-20}"
STREAM="${EXCALIBUR_LOAD_STREAM:-device_agent_system_stats}"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

json_get() {
  node -e "const fs=require('fs'); const obj=JSON.parse(fs.readFileSync(process.argv[1], 'utf8')); console.log(obj${2});" "$1"
}

request() {
  local method="$1"
  local path="$2"
  local body="${3:-}"
  local out="$4"
  if [[ -n "$body" ]]; then
    curl -fsS -X "$method" "$API_BASE_URL$path" \
      -H "authorization: Bearer $TOKEN" \
      -H "content-type: application/json" \
      --data "$body" > "$out"
  else
    curl -fsS -X "$method" "$API_BASE_URL$path" \
      -H "authorization: Bearer $TOKEN" > "$out"
  fi
}

auth_body="$(printf '{"email":"%s","password":"%s","display_name":"%s"}' "$EMAIL" "$PASSWORD" "$DISPLAY_NAME")"
if curl -fsS -X POST "$API_BASE_URL/api/v1/auth/register" \
  -H "content-type: application/json" \
  --data "$auth_body" > "$tmpdir/auth.json"; then
  :
else
  login_body="$(printf '{"email":"%s","password":"%s"}' "$EMAIL" "$PASSWORD")"
  curl -fsS -X POST "$API_BASE_URL/api/v1/auth/login" \
    -H "content-type: application/json" \
    --data "$login_body" > "$tmpdir/auth.json"
fi

TOKEN="$(json_get "$tmpdir/auth.json" '.token')"

request POST /api/v1/orgs '{"name":"Load Smoke","slug":"load-smoke"}' "$tmpdir/org.json" || \
  request GET /api/v1/orgs "" "$tmpdir/orgs.json"
if [[ -f "$tmpdir/org.json" ]]; then
  ORG_ID="$(json_get "$tmpdir/org.json" '.id')"
else
  ORG_ID="$(node -e "const fs=require('fs'); const rows=JSON.parse(fs.readFileSync(process.argv[1], 'utf8')); console.log(rows[0].id)" "$tmpdir/orgs.json")"
fi

request POST /api/v1/projects "$(printf '{"org_id":"%s","name":"Load Smoke","slug":"load-smoke"}' "$ORG_ID")" "$tmpdir/project.json" || \
  request GET "/api/v1/projects?org_id=$ORG_ID" "" "$tmpdir/projects.json"
if [[ -f "$tmpdir/project.json" ]]; then
  PROJECT_ID="$(json_get "$tmpdir/project.json" '.id')"
else
  PROJECT_ID="$(node -e "const fs=require('fs'); const rows=JSON.parse(fs.readFileSync(process.argv[1], 'utf8')); console.log(rows[0].id)" "$tmpdir/projects.json")"
fi

start_ms="$(date +%s%3N)"
for device_index in $(seq 1 "$DEVICE_COUNT"); do
  name="load-device-$device_index"
  request POST /api/v1/devices "$(printf '{"project_id":"%s","name":"%s","metadata":{"load_smoke":true}}' "$PROJECT_ID" "$name")" "$tmpdir/device.json" || true
done

request GET "/api/v1/devices?project_id=$PROJECT_ID" "" "$tmpdir/devices.json"
node -e '
const fs = require("fs");
const devices = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
const selected = devices.filter((device) => device.metadata?.load_smoke).slice(0, Number(process.argv[2]));
fs.writeFileSync(process.argv[3], JSON.stringify(selected));
' "$tmpdir/devices.json" "$DEVICE_COUNT" "$tmpdir/selected-devices.json"

node -e '
const fs = require("fs");
const devices = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
const projectId = process.argv[2];
const stream = process.argv[3];
const pointsPerDevice = Number(process.argv[4]);
for (const device of devices) {
  const payload = [];
  for (let index = 0; index < pointsPerDevice; index += 1) {
    payload.push({
      sequence: Date.now() * 1000 + index,
      timestamp: new Date().toISOString(),
      cpu_percent: 20 + ((index + device.id.length) % 70),
      memory_mb: 256 + index,
    });
  }
  console.log(JSON.stringify({
    topic: `v1/p/${projectId}/d/${device.id}/telemetry/${stream}`,
    payload,
  }));
}
' "$tmpdir/selected-devices.json" "$PROJECT_ID" "$STREAM" "$POINTS_PER_DEVICE" > "$tmpdir/ingest.ndjson"

while IFS= read -r body; do
  request POST /api/v1/telemetry "$body" "$tmpdir/ingest-response.json"
done < "$tmpdir/ingest.ndjson"
end_ms="$(date +%s%3N)"

total_points=$((DEVICE_COUNT * POINTS_PER_DEVICE))
duration_ms=$((end_ms - start_ms))
echo "project_id=$PROJECT_ID"
echo "devices=$DEVICE_COUNT"
echo "points=$total_points"
echo "duration_ms=$duration_ms"
