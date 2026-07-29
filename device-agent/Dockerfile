FROM rust:alpine as builder

RUN apk add build-base openssl-dev
WORKDIR "/usr/share/bytebeam/device_agent"

COPY device-agent/ device_agent
COPY storage/ storage
COPY tools/utils/ tools/utils
COPY tools/system-stats/ tools/system-stats
COPY Cargo.* ./
COPY .git/ .git

RUN mkdir -p /usr/share/bytebeam/device_agent/bin
RUN cargo build --release
RUN cp target/release/device_agent /usr/share/bytebeam/device_agent/bin/

###################################################################################################

FROM alpine:latest

RUN apk add runit bash curl coreutils
WORKDIR "/usr/share/bytebeam/device_agent"

RUN mkdir -p /usr/share/bytebeam/device_agent
COPY --from=builder /usr/share/bytebeam/device_agent/bin /usr/bin
COPY runit/ /etc/runit

CMD ["/usr/bin/runsvdir", "/etc/runit"]

COPY scripts/paths/ paths
COPY simulator.sh .

