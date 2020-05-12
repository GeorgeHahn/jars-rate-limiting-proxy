# Jars: Just a rate-limiting service

## Configuration

The base configuration is set by a file named `settings.toml` ([example](settings.example.toml)). This should be present in the working directory. Settings can also be set or overridden with environment variables. Appropriate key names are listed in the example toml file.

The `base_path` and `error_path` parameters can be reconfigured at runtime. This is currently implemented for Etcd and is automatically enabled if an Etcd cluster is configured. When in use, writes to keys with the prefix `etcd_key_prefix` and name `base_path` or `error_path` will reconfigure all listening instances of this service. `etcd_key_prefix` defaults to `jars_config_`. In this configuration, the base path can be changed by setting `jars_config_base_path` to the desired path.

## Design

### Requirements
- Global rate limiting
- Minimal latency impact
- High throughput

### Persistence

Persistence will be used because global rate limiting is desired. This service is written to support multiple database options, with Redis chosen for the initial implementation. The database used will have a significant impact on the runtime behavior of this service - performance and scalability are both largely inherited from the database.

### Rate limiting strategy

There are many well-documented strategies for implementing rate limiting, but the desire for high volume geo-distributed rate limiting guides us towards a single option.

Chosen strategy: Sliding window average rate limit

Increment a timestamped counter after each request. The current rate is obtained by performing a weighted average between the current and previous counters. This maps very nicely to Redis because counters can be incremented atomically & key expiration is straightforward.

Well documented by cloudflare at: https://blog.cloudflare.com/counting-things-a-lot-of-different-things/

Other strategies:
 - Bucket-based algorithms: Significant overhead for a system that needs to support an arbitrary number of buckets.
 - Fixed window counter: Uses a single counter for slightly less persistance overhead than a sliding window, but is prone to request spikes when window expires.
 - Sliding window log: Very accurate, but incurs the most persistance overhead for storage and for counter retrieval.

## Development plan

### Primary functionality

- Redis integration for rate limit counters
- Etcd integration for runtime reconfiguration
- Logging: Annotate --everything-- with `tracing` (not implemented)

### Critical support functionality (not implemented)

- Improve operational documentation
  - Actually test the Dockerfile
- Integration tests
- CI: build + test + performance benchmarks
- Great error messages (+ probably implement a crate error type)

### Production readiness (not implemented)

- Get config KVs from Etcd at startup
- Redis TLS
- Etcd TLS
- Streaming responses
- Record metrics and export to statsd
- Set XFP/XFH headers. (Only XFF is currently implemented.)
- Configurable global rate limit (+ runtime reconfigurable)
- Consider supporting http 2, 3, and tls (upstream and/or downstream)
- Implement a client for a globally scalable counter store or a bespoke counter service if scaling across multiple DCs
