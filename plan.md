# Plan

## Questions

Should paths be rate limited with or without the query string?

## Primary functionality

- Redis integration
- Etcd integration
- Streaming responses

## Critical support functionality (not implemented)

- Operational documentation
- CI builds
- CI performance benchmarks

## Production readiness (not implemented)

- Etcd TLS
- Redis TLS
- Consider implementing a Pulsar counter store
  - Pulsar should scale globally more-or-less out of the box
  - Redis would probably? require clustering and a local cache layer to scale globally
- Record metrics and export to statsd
- Consider Http version support (http2? 3?) & TLS termination support
- Set XFP/XFH headers. (Only XFF is currently implemented.)

