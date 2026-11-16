# Development Guidelines

To run `cargo-crap`:

```sh
# Install required crates
cargo install cargo-llvm-cov cargo-crap

# 1 Generate an LCOV coverage report.
cargo llvm-cov --lcov --output-path lcov.info

# 2 Score every function.
cargo crap --lcov lcov.info
```
