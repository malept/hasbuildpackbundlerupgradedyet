# Benchmarks

I ran some benchmarks using `wrk` on a quad core laptop with 8GB of RAM.

## Configuration

All daemons ran with 5 threads, 1 worker, and Redis enabled (so as to not hammer GitHub). The `wrk`
client ran with the following configuration:

```
wrk -H 'Accept: application/json' -t 1 -c 4 -d 60 $URL
```

## Raw Results

### Ruby 2.3.1

Rack + Puma

```
Running 1m test @ http://localhost:RUBY
  1 threads and 4 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency   153.12ms   15.67ms 278.61ms   73.77%
    Req/Sec    26.18      8.95    40.00     71.81%
  1567 requests in 1.00m, 133.13KB read
Requests/sec:     26.10
Transfer/sec:      2.22KB
```

## Python 3.4.2

Werkzeug + Gunicorn

```
Running 1m test @ http://localhost:PYTHON
  1 threads and 4 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency   156.96ms   15.44ms 297.32ms   80.81%
    Req/Sec    26.00      9.63    40.00     65.58%
  1527 requests in 1.00m, 259.47KB read
Requests/sec:     25.43
Transfer/sec:      4.32KB
```

## Rust 1.10.0

Hyper 0.9.9 (no mio)

```
Running 1m test @ http://localhost:RUST
  1 threads and 4 connections
  Thread Stats   Avg      Stdev     Max   +/- Stdev
    Latency    19.75ms    4.86ms  67.41ms   88.12%
    Req/Sec   203.90     39.64   282.00     62.17%
  12199 requests in 1.00m, 21.15MB read
Requests/sec:    203.16
Transfer/sec:    360.70KB
```
