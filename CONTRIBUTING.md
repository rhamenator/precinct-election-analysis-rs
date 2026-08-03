# Contributing

Keep changes scientifically cautious and reproducible.

1. Create a focused branch.
2. Add tests for every behavior change and regression.
3. Preserve source values and explicit exclusion reasons.
4. Keep methods independent unless a combined score has a documented calibration study.
5. Never describe an anomaly as proof of fraud, misconduct, manipulation, or an incorrect outcome.
6. Run `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-targets`, and `cargo build --release`.
7. Update data-format, methodology, MCP, and operator documentation when contracts change.

Do not add a software license without the repository owner’s decision.
