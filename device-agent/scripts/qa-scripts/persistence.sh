#!/bin/bash

set -xe

source qa-scripts/.env

printf "$(cat << EOF
persistence_path = "/var/tmp/persistence"

[simulator]
actions = []
gps_paths = "./paths/"

[streams.motor]
topic = "v1/p/{project_id}/d/{device_id}/telemetry/gps"

[streams.bms]
topic = "v1/p/{project_id}/d/{device_id}/telemetry/bms"
persistence = { max_file_size = 0 }

[streams.imu]
topic = "v1/p/{project_id}/d/{device_id}/telemetry/imu"
persistence = { max_file_count = 3, max_file_size = 1024 }
EOF
)" > devices/persistence.toml
docker cp devices/persistence.toml simulator:/usr/share/bytebeam/device_agent/devices/persistence.toml

docker exec -it simulator device_agent -a /usr/share/bytebeam/device_agent/devices/device_$DEVICE_ID.json -c /usr/share/bytebeam/device_agent/devices/persistence.toml -vv -m device_agent::base::serializer -m storage

# Slow down mqtts
# toxiproxy-cli toxic add -n slow -t latency -a latency=100 --downstream mqtts
# Look at logs for persistence into disk in slow mode, catchup mode
# Disrupt mqtts, check logs for slow mode data loss on in-memory buffer overflow, etc.
# toxiproxy-cli delete mqtts
# Verify persistence of packets onto disk
# docker exec -it simulator tree /var/tmp/persistence
# Bring back network, check logs for back to normal mode, check platform for appropriate data retention/loss with timestamp gaps
# toxiproxy-cli new mqtts --listen 0.0.0.0:8883 --upstream $CONSOLED_DOMAIN:8883
