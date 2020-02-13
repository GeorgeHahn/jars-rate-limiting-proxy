# Jars: Just a rate-limiting service

## Requirements
- Global rate limiting
- Minimal latency impact
- High throughput

## Design
### Persistence

Persistence will be used because global rate limiting is desired. Many high performance KV databases would be suitable for persistence. The chosen database will impact geographical scalability & runtime behavior. This service is written to support multiple database options, with Redis chosen for the initial implementation. Support for Apache Pulsar would be an interesting future development, but was deferred due to the lack of a mature Rust client library.

### Rate limiting strategies

There are many well-documented strategies for implementing rate limiting. High volume distributed rate limiting guides us towards a couple of options.

Strategy 1: Sliding window average rate limit

    Calculate a weighted average between the current & previous window counters
        This works really well with redis, because counters can be incremented atomically and expiration is straightforward

    Documented by cloudflare at: https://blog.cloudflare.com/counting-things-a-lot-of-different-things/

Strategy 2: Leaky bucket rate limit
Strategy 3:


Implementation stategy 1: In memory in this app

Use something like a `HashMap<Uri, Counters>` to store rate limit information. This could be made extremely fast, but the service would need a way to distribute this information across a global cluster. This would significantly increase complexity and operational workload. Redis is a known factor with widespread operational experience and adoption.


Implementation stategy 2: Push counter to Redis (cassandra, pulsar? reqs: kv & global clustering, key expiration is nice)

Store rate limit information in database (`URIHASH:MINUTE` with expiration would require no app-controlled maintenance)

`GET URIHASH:MINUTE-1 URIHASH:MINUTE`
if not limited
Make request + stream response
`INCR URIHASH:MINUTE`









Avoid:
    Don't modify the request stream, don't deserialize the entire request stream - this service only needs the URL and that's all it should try to get.

Ops:
    metrics: statsd
    logging: slog or tokio-tracing, TBD



Notes:
    All pre-built rust libraries do more processing than necessary for a light-touch request forwarder. Hyper is used here, but it would be preferable to stream-decode the URL portion of requests and then attach the stream

Challenges:
    Global clustering for any data store
    We can get pretty hand-wavey about this since the rate limit doesn't need to be perfect
        STILL HARD

Optimally:
    Only parse URL out of requests & immediately forward if allowed. Stream response with a minimum of data copies & syscalls. (io_uring should be a win here.)