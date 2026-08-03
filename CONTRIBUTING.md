# Contributing

Keep changes scientifically cautious and reproducible.

1. Create a focused branch.
2. Add tests for every behavior change and regression.
3. Preserve source values and explicit exclusion reasons.
4. Keep methods independent unless a combined score has a documented calibration study.
5. Never describe an anomaly as proof of fraud, misconduct, manipulation, or an incorrect outcome.
6. Run `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-targets`, and `cargo build --release`.
7. Install coverage support with `cargo install cargo-llvm-cov --locked` and `rustup toolchain install nightly --component llvm-tools-preview`, then run `cargo +nightly llvm-cov --all-targets --branch --fail-under-lines 95 --fail-under-regions 95 --fail-under-functions 94`.
8. Update data-format, methodology, MCP, and operator documentation when contracts change.

Contributions are distributed under the GNU General Public License v3.0 only; see `LICENSE`.
