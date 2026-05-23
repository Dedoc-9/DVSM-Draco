# Contributing

## Prerequisites

- Rust 1.70+
- Cargo
- Git

## Process

1. Fork repository
2. Create feature branch
3. Ensure cargo test --release passes
4. Commit with descriptive message
5. Include CLA signature in PR description
6. Submit pull request

## Code Style

- Format: cargo fmt
- Lint: cargo clippy --all-targets --release
- Tests: All public APIs must have tests

## Formal Systems Requirements

All code must:
- Use immutable state bindings
- Maintain dual arithmetic separation (Z primary, S dual)
- Preserve hash continuity
- Document ghost state computation
- Follow protocol ordering

See LEGAL_FRAMEWORK.md for licensing compliance.
