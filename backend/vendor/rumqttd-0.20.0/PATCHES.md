# Excalibur rumqttd Patch Manifest

Vendored base: `rumqttd` 0.20.0 source tree.

This copy is vendored because Excalibur needs broker callbacks that are not exposed by upstream 0.20.0.

## Behavior Changes

- Exposes source client id on forwarded publish packets so the runtime can bind publish ACLs to the authenticated MQTT connection.
- Adds subscribe authorization hook for command topic ACL checks.
- Adds publish authorization hook before routing publish packets.
- Propagates TLS peer certificate fingerprint from the accepted TLS connection into connect auth.
- Adds `set_auth_handler_with_peer` while preserving the existing auth handler shape.
- Adds `PeerCertFingerprint` and `PublishAuthHandler` public types.

## Files Touched

- `src/lib.rs`
- `src/server/tls.rs`
- `src/server/broker.rs`
- `src/link/remote.rs`
- `src/link/local.rs`
- `src/router/connection.rs`
- `Cargo.toml`
- `Cargo.toml.orig`

## Refresh Steps

1. Replace this directory with the target upstream `rumqttd` source.
2. Reapply the behavior changes above in the smallest possible patch.
3. Run `cargo test -p excalibur-mqtt-ingest --offline` from `backend/`.
4. Run an mTLS simulator check for accepted, revoked/disabled, and cross-project publish/subscribe denial.
5. Update this manifest with any changed upstream version or file list.
