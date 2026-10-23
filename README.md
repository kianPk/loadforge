# Loadforge

**High-concurrency HTTP load forge** written in Rust — Tokio worker pool, percentile latency histograms, Axum control plane, and a live WebSocket dashboard.

> Only test systems you own or have explicit permission to hammer.

[![Demo](https://img.shields.io/badge/Demo-kianpk.github.io%2Floadforge-e4572e?style=for-the-badge)](https://kianpk.github.io/loadforge/)
[![Rust](https://img.shields.io/badge/Rust-Tokio%20%2B%20Axum-1f6f8b?style=for-the-badge&logo=rust)](https://github.com/kianPk/loadforge)

## Why it hits hard

- **Tokio workers** with a concurrency semaphore and optional global RPS cap
- **Latency histogram** → p50 / p90 / p95 / p99 + recent stream for the chart
- **Axum + WebSocket** control plane — start/stop runs, stream snapshots at 4 Hz
- **CLI + dashboard** — headless JSON output or a foundry-grade UI in `web/`
- **GitHub Pages demo** in `docs/` (simulated forge when no backend is present)

## Quick start

```bash
# dashboard (serves ./web on :7070)
cargo run --release -- serve --bind 127.0.0.1:7070

# headless forge
cargo run --release -- run \
  --url https://httpbin.org/get \
  --concurrency 64 \
  --duration 20 \
  --rps 200
```

Open [http://127.0.0.1:7070](http://127.0.0.1:7070) → set a target → **Ignite**.

## Also in this release

- `GET /api/report` — graded summary (A–D) with notes
- `scenarios/*.json` — headless runs via `--scenario`

```bash
cargo run --release -- run --scenario scenarios/httpbin.json
```

## API

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/health` | liveness + busy flag |
| `GET` | `/api/metrics` | latest snapshot |
| `POST` | `/api/run` | start a forge (`RunConfig` JSON) |
| `POST` | `/api/stop` | signal cool-down |
| `WS` | `/ws` | live metric stream |

### `RunConfig`

```json
{
  "target": "https://httpbin.org/get",
  "method": "GET",
  "concurrency": 48,
  "durationSecs": 20,
  "rps": 200,
  "timeoutMs": 5000,
  "headers": {},
  "body": null,
  "warmupSecs": 1
}
```

## Stack

```text
Rust 2021 · Tokio · Axum · reqwest (rustls) · clap · serde
WebSocket metrics · IBM Plex Mono + Bricolage Grotesque UI
```

## Layout

```text
src/          engine, metrics, Axum server, CLI
web/          live dashboard (served by `loadforge serve`)
docs/         GitHub Pages copy (demo mode on github.io)
```

## License

MIT © Kian Zarandi ([@kianPk](https://github.com/kianPk))
