#!/bin/bash

set -xe

source qa-scripts/.env

printf "$(cat << EOF
[simulator]
actions = []
gps_paths = "./paths/"

[streams.bms]
topic = "v1/p/{project_id}/d/{device_id}/telemetry/bms"
flush_period = 2

[streams.imu]
topic = "v1/p/{project_id}/d/{device_id}/telemetry/imu"
batch_size = 10
EOF
)" > devices/streams.toml
docker cp devices/streams.toml simulator:/usr/share/bytebeam/device_agent/devices/streams.toml

docker exec -it simulator device_agent -a /usr/share/bytebeam/device_agent/devices/device_$DEVICE_ID.json -c /usr/share/bytebeam/device_agent/devices/streams.toml -vv -m device_agent::base::bridge
