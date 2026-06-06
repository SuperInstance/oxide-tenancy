# oxide-tenancy

GPU infrastructure crate from the SuperInstance ecosystem.

## Overview

# oxide-tenancy

Multi-tenant GPU isolation with ternary quality signals.

## Architecture

This crate sits within the **five-layer Oxide Stack**:

| Layer | Crate | Role |
|-------|-------|------|
| 1 | open-parallel | Async runtime (tokio fork) |
| 2 | pincher | "Vector DB as runtime, LLM as compiler" |
| 3 | flux-core | Bytecode VM + A2A agent protocol |
| 4 | cuda-oxide | Flux→MIR→Pliron→NVVM→PTX compiler |
| 5 | cudaclaw | Persistent GPU kernels, warp consensus, SmartCRDT |

The key insight: **ternary values {-1, 0, +1} map directly to GPU compute**. They pack 16× denser than FP32, enable XNOR+popcount matmul, and conservation laws become compile-time checks.

## Stats

| Metric | Value |
|--------|-------|
| Tests | 11 |
| Lines of Code | 361 |
| Public API Surface | 21 items |
| License | Apache-2.0 |

## Installation

```toml
[dependencies]
oxide-tenancy = "0.1.0"
```

## Usage

```rust
use oxide_tenancy::*;
// See src/lib.rs tests for complete working examples
```

### Key Types

```
- pub enum IsolationQuality {
    pub fn from_i8(v: i8) -> Option<Self> {
- pub struct Tenant {
    pub fn new(id: impl Into<String>, weight: f64, quota: f64) -> Self {
    pub fn proportional_share(&self, total_weight: f64, total_resources: f64) -> f64 {
    pub fn is_over_quota(&self) -> bool {
- pub struct TenancyManager {
    pub fn new(total_resources: f64) -> Self {
    pub fn add_tenant(&mut self, tenant: Tenant) {
    pub fn remove_tenant(&mut self, id: &str) -> Option<Tenant> {
```

## Design Philosophy

This crate uses **ternary algebra** (Z₃) where every value is {-1, 0, +1}:

- **+1** → positive signal (healthy, allocated, converged, ready)
- **0** → neutral (pending, balanced, monitoring, degraded)
- **-1** → negative signal (failed, free, diverged, overloaded)

This isn't arbitrary — ternary is the natural encoding for:
1. **BitNet b1.58** (Microsoft) — ternary neural networks at 60% less power
2. **GPU warp voting** — hardware ballot instructions return ternary consensus
3. **Conservation laws** — {-1, 0, +1} preserves quantity (what goes in must come out)

## Testing

```bash
git clone https://github.com/SuperInstance/oxide-tenancy.git
cd oxide-tenancy
cargo test
```

## License

Apache-2.0
