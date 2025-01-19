# Fly Metrics Proxy

**Disclaimer**: This project's dumb and unrefined. But it works for me. I might polish it over time if it's worthwhile.

This proxy exposes a `/metrics` endpoint for Fly.io's managed Prometheus service to scrape. When the endpoint is requested, it fetches multiple other endpoints for metrics, aggregates and dedups them, adds a per-endpoint user-defined set of labels, and returns them to prometheus.

It's intended to be used in combination with [Fly Autoscaler](https://github.com/superfly/fly-autoscaler) to get scale-to-zero behavior for apps that can't be started by incoming web requests, but can scale based on metrics like `queue_length` that are served from an external source (outside Fly). Fly's prometheus hits this proxy with a request for metrics, the proxy fetches them and attaches an additional label to each so that metrics with the same name can be distinguished after ingestion. The Fly autoscaler can then scale target apps according to whichever label represents their metric.

## Usage

The proxy expects a JSON-formatted value supplied by environment variable `METRICS_TARGETS`, of the following shape:
```
[
    {
        "endpoint": "http://example.com/metrics_1",
        "labels": {"app_name": "my-app-1", "environment": "production"}
    },
    {
        "endpoint": "http://example.com/metrics_2",
        "labels": {"app_name": "my-app-2", "environment": "staging", "brisket": "delicious"}
    }
]
```

If the metrics served from each targeted endpoint look like this:
```
# HELP queue_length The number of outstanding jobs in the queue
# TYPE queue_length gauge
queue_length 0
```

The proxy will return:
```
# HELP queue_length The number of outstanding jobs in the queue
# TYPE queue_length gauge
queue_length{app_name="my-app-1",environment="production"} 0
queue_length{app_name="my-app-2",environment="staging",brisket="delicious"} 0
```
