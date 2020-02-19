# Plan

## Questions

Should paths be rate limited with or without the query string?
Should proxied responses be streamed back to the client?
Does this need to be able to scale globally or only within a single zone? I built the rate limit store with modularity in mind & prototyped with redis as an easy first target. I can add support for a db that can scale globally (cassandra/pulsar) or

## Primary functionality

- Redis integration
- Etcd integration
- Streaming responses

## Critical support functionality (not implemented)

- Operational documentation
- CI builds
- CI performance benchmarks
- Great error messages

## Production readiness (not implemented)

- Etcd TLS
- Redis TLS
- Consider implementing a Pulsar counter store
  - Pulsar should scale globally more-or-less out of the box
  - Redis would probably? require clustering and a local cache layer to scale globally
- Record metrics and export to statsd
- Consider Http version support (http2? 3?) & TLS termination support
- Set XFP/XFH headers. (Only XFF is currently implemented.)

## Challenges

- I want to share a bunch of resources across tasks with one copy per thread. This is difficult to reason about.