 #!/bin/bash

set -ex

if [[ $# -ne 1 ]]; then
    echo "Pass number of devices"
    echo "Usage: ./createvehicle 10"
    exit
fi

count=$1

for i in $(seq 1 $count)
do
    RUST_LOG=rumq_client=debug,device_agent=warn target/debug/device_agent -c config/device_agent.toml -i $i -a certs/  2>&1 | tee /tmp/device_agent-$i.txt &
done

trap 'kill $(jobs -p)' EXIT
wait
