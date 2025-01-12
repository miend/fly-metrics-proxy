# Fly Metrics Proxy

This is a PoC to test things out. It has no good error handling, no tests, and doesn't cover edge cases. Don't use it.

This proxy exposes a `/metrics` endpoint for Fly.io's managed Prometheus service to scrape. When the endpoint is requested, it fetches multiple other endpoints for metrics, aggregates and dedups them, adds a label, and returns them to prometheus.

It's intended to be used in combination with [Fly Autoscaler](https://github.com/superfly/fly-autoscaler) to get scale-to-zero behavior for apps that can't be started by incoming web requests, but can scale based on externally provided metrics like `queue_length`. Fly's prometheus hits this proxy with a request for metrics, the proxy fetches them and attaches an additional label to each so that metrics with the same name can be distinguished after ingestion. The Fly autoscaler can then scale target apps according to whichever label represents their metric.

The proxy expects a JSON-formatted value supplied by environment variable `METRICS_TARGETS`, of the following shape:
```
[
    {
        "endpoint": "http://example.com/metrics_1",
        "queue_name": "my-queue-1"
    },
    {
        "endpoint": "http://example.com/metrics_2",
        "queue_name": "my-queue-2"
    }
]
```
