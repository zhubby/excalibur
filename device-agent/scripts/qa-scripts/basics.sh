#!/bin/bash

set -xe

source qa-scripts/.env

printf "$(cat << EOF
[console]
enabled = true
port = 3333
EOF
)" > devices/basics.toml
docker cp devices/basics.toml simulator:/usr/share/bytebeam/device_agent/devices/basics.toml

docker exec -it simulator device_agent -a /usr/share/bytebeam/device_agent/devices/device_$DEVICE_ID.json -c /usr/share/bytebeam/device_agent/devices/basics.toml -vv

# from separate terminal run the following to trigger minimum log level change(show debug logs of rumqttc)
# docker exec -it simulator curl -X POST -H "Content-Type: text/plain" -d "device_agent=info,rumqtt=debug" http://localhost:3333/logs
