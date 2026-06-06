# oxide-tenancy

Multi-tenant GPU isolation with ternary quality signals.

## Why This Exists

When multiple tenants share a GPU — different teams, different workloads, different SLAs — you can't just hand out time slices and hope for the best. GPUs have shared memory bandwidth, shared L2 cache, shared SM schedulers. One tenant's memory-hungry kernel can degrade another's latency by 40% without either exceeding its nominal allocation.

The core insight: isolation quality is not binary. A tenant can be **fully isolated** (+1), **cooperatively sharing** (0), or **actively interfering** (-1). This ternary signal drives every decision — allocation, quarantine, rebalancing — without needing exact interference measurements (which are expensive and often unavailable on real hardware).

## Architecture

```
┌─────────────────────────────────────────────────┐
│                TenancyManager                    │
│  total_resources: f64 (e.g. 100.0 = 100%)       │
│                                                  │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐      │
│  │ Tenant A │  │ Tenant B │  │ Tenant C │      │
│  │ weight=3 │  │ weight=1 │  │ weight=2 │      │
│  │ quota=40 │  │ quota=30 │  │ quota=50 │      │
│  │ quality  │  │ quality  │  │ quality  │      │
│  │ =Shared  │  │ =Isolat. │  │ =Interf. │      │
│  └──────────┘  └──────────┘  └──────────┘      │
│                                                  │
│  fair_share_allocate() ──→ HashMap<id, units>    │
│  detect_interference()  ──→ Vec<offender ids>    │
│  quarantine(id)         ──→ remove from pool     │
│  rebalance()            ──→ reallocate + update  │
└─────────────────────────────────────────────────┘
```

**Key types:**

- `IsolationQuality` — ternary enum: `Isolated(+1)`, `Shared(0)`, `Interference(-1)`
- `Tenant` — weight, quota, current usage, quality signal, quarantine flag
- `TenancyManager` — the allocation engine: fair-share with quota caps and surplus redistribution

**Data flow:** Tenants register → manager computes proportional shares → quota caps applied → surplus redistributed → interference detected → offenders quarantined → pool shrinks → rebalance.

## Usage

```rust
use oxide_tenancy::{TenancyManager, Tenant, IsolationQuality};

let mut mgr = TenancyManager::new(100.0); // 100 GPU units total

mgr.add_tenant(Tenant::new("training", 3.0, 60.0));  // 60% max, weight 3
mgr.add_tenant(Tenant::new("inference", 1.0, 30.0)); // 30% max, weight 1
mgr.add_tenant(Tenant::new("analytics", 1.0, 40.0)); // 40% max, weight 1

// Fair-share allocation: training gets ~60, inference ~20, analytics ~20
let alloc = mgr.fair_share_allocate();
assert!((alloc["training"] - 60.0).abs() < 1e-6); // capped at quota
assert!((alloc["inference"] - 20.0).abs() < 1e-6);

// Detect interference and quarantine
let noisy = mgr.detect_interference(); // returns tenants with quality == Interference
for id in &noisy {
    mgr.quarantine(id); // removes from allocation pool
}

// Rebalance after changes
mgr.update_weight("inference", 2.0);
let new_alloc = mgr.rebalance();
```

## API Reference

### `IsolationQuality`

```rust
pub enum IsolationQuality {
    Isolated = 1,    // Full isolation, no cross-tenant effects
    Shared = 0,      // Cooperative sharing
    Interference = -1, // Causing or experiencing interference
}
```

- `from_i8(v: i8) -> Option<Self>` — convert from numeric ternary value

### `Tenant`

```rust
pub struct Tenant {
    pub id: String,
    pub weight: f64,       // Fair-share weight (must be > 0)
    pub quota: f64,        // Hard cap in GPU units
    pub usage: f64,        // Current resource consumption
    pub quality: IsolationQuality,
    pub quarantined: bool,
}
```

- `new(id, weight, quota) -> Self`
- `proportional_share(total_weight, total_resources) -> f64` — weight-proportional allocation
- `is_over_quota() -> bool` — usage exceeds quota

### `TenancyManager`

- `new(total_resources: f64) -> Self`
- `add_tenant(tenant: Tenant)` — register a tenant
- `remove_tenant(id: &str) -> Option<Tenant>`
- `get_tenant(id: &str) -> Option<&Tenant>` / `get_tenant_mut(id: &str) -> Option<&mut Tenant>`
- `tenant_ids() -> Vec<String>` / `tenant_count() -> usize`
- `fair_share_allocate() -> HashMap<String, f64>` — proportional allocation with quota caps and surplus redistribution
- `detect_interference() -> Vec<String>` — tenants with `Interference` quality
- `quarantine(id: &str) -> bool` / `release_quarantine(id: &str) -> bool`
- `rebalance() -> HashMap<String, f64>` — recompute allocations and update usage
- `update_weight(id: &str, new_weight: f64) -> bool`
- `over_quota_tenants() -> Vec<String>`

## The Deeper Idea

This is the **multi-tenancy layer** in the oxide stack's resource management architecture. The ternary isolation signal (`Isolated`/`Shared`/`Interference`) is the same {-1, 0, +1} pattern used throughout the ecosystem — in health monitoring, capacity planning, and load shedding. The shared vocabulary means a tenancy interference event can cascade into a capacity scale-up or a load-shed decision without protocol translation.

The fair-share algorithm does two passes: first allocate `min(proportional_share, quota)`, then redistribute any surplus from over-quota tenants back to those with headroom. This avoids the classic problem where strict proportional allocation wastes resources when a high-weight tenant has a low quota.

## Related Crates

- **oxide-capacity** — cluster-level capacity planning that consumes tenancy allocation data
- **oxide-health-monitor** — GPU health signals that feed into tenant quality classification
- **oxide-lease-grid** — spatial resource allocation (which GPUs, not how much)
- **oxide-loadshed** — when tenancy allocation can't help and jobs must be shed
