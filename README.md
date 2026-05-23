# Draco Physics Engine

DVSM v3.3 physics kernel integrated into Draco_BF6 engine.

## Quick Start

Clone: git clone https://github.com/your-username/draco-bf6-physics.git
Build: cargo build --release
Test: cargo test --release -- --nocapture --test-threads=1

## Architecture

- L1 (Load): Input validation
- L2 (Lie-bracket): Manifold evolution
- L3 (Dissipation): Energy dissipation
- L4 (Backreaction): Gravitational backreaction
- L5 (Spectral): Spectral filtering
- L6 (EMA): Exponential moving average
- L7 (Hash): State integrity binding

## Legal: AGPL-3.0 Open Source

For custom licensing, contact the Author: Daniel J. Dillberg at bigdilly95@gmail.com
