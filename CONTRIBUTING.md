# Contributing

Thanks for helping improve Loadforge.

## Development

```bash
cargo test
cargo run -- serve --bind 127.0.0.1:7070
```

## Pull requests

1. Fork and branch from `main`
2. Run `cargo test` and `cargo fmt` before opening a PR
3. Describe behavior changes and link issues when applicable

## Load testing ethics

Only run forgework against systems you own or have **written authorization** to test. See [SECURITY.md](SECURITY.md).
