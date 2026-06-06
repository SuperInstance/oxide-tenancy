//! # oxide-tenancy
//!
//! Multi-tenant GPU isolation with ternary quality signals.
//!
//! Each tenant receives a fair-share allocation proportional to their weight.
//! The system monitors for interference and can quarantine misbehaving tenants.

use std::collections::HashMap;

/// Ternary isolation quality signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolationQuality {
    /// Tenant is fully isolated — no cross-tenant effects.
    Isolated = 1,
    /// Tenant shares resources cooperatively.
    Shared = 0,
    /// Tenant is causing or experiencing interference.
    Interference = -1,
}

impl IsolationQuality {
    pub fn from_i8(v: i8) -> Option<Self> {
        match v {
            1 => Some(Self::Isolated),
            0 => Some(Self::Shared),
            -1 => Some(Self::Interference),
            _ => None,
        }
    }
}

/// A single tenant in the GPU tenancy system.
#[derive(Debug, Clone)]
pub struct Tenant {
    pub id: String,
    /// Relative weight for fair-share scheduling (must be > 0).
    pub weight: f64,
    /// Hard quota cap in GPU units (e.g. SM%, memory %).
    pub quota: f64,
    /// Current resource usage in GPU units.
    pub usage: f64,
    /// Current isolation quality.
    pub quality: IsolationQuality,
    /// Whether the tenant is quarantined.
    pub quarantined: bool,
}

impl Tenant {
    pub fn new(id: impl Into<String>, weight: f64, quota: f64) -> Self {
        Self {
            id: id.into(),
            weight,
            quota,
            usage: 0.0,
            quality: IsolationQuality::Shared,
            quarantined: false,
        }
    }

    /// Returns the tenant's share of `total` proportional to its weight.
    pub fn proportional_share(&self, total_weight: f64, total_resources: f64) -> f64 {
        if total_weight <= 0.0 {
            return 0.0;
        }
        (self.weight / total_weight) * total_resources
    }

    /// Returns true if usage exceeds quota.
    pub fn is_over_quota(&self) -> bool {
        self.usage > self.quota
    }
}

/// Manages GPU resource allocation across multiple tenants.
#[derive(Debug, Clone)]
pub struct TenancyManager {
    tenants: HashMap<String, Tenant>,
    /// Total GPU resource pool (e.g. 100.0 = 100%).
    total_resources: f64,
}

impl TenancyManager {
    pub fn new(total_resources: f64) -> Self {
        Self {
            tenants: HashMap::new(),
            total_resources,
        }
    }

    /// Register a new tenant.
    pub fn add_tenant(&mut self, tenant: Tenant) {
        self.tenants.insert(tenant.id.clone(), tenant);
    }

    /// Remove a tenant by id.
    pub fn remove_tenant(&mut self, id: &str) -> Option<Tenant> {
        self.tenants.remove(id)
    }

    /// Get a tenant reference.
    pub fn get_tenant(&self, id: &str) -> Option<&Tenant> {
        self.tenants.get(id)
    }

    /// Get a mutable tenant reference.
    pub fn get_tenant_mut(&mut self, id: &str) -> Option<&mut Tenant> {
        self.tenants.get_mut(id)
    }

    /// All registered tenant IDs.
    pub fn tenant_ids(&self) -> Vec<String> {
        self.tenants.keys().cloned().collect()
    }

    /// Number of tenants.
    pub fn tenant_count(&self) -> usize {
        self.tenants.len()
    }

    /// Compute fair-share allocations for all active (non-quarantined) tenants.
    ///
    /// Returns a map of tenant-id -> allocated units. Each tenant receives
    /// `min(proportional_share, quota)` so no tenant exceeds its cap.
    pub fn fair_share_allocate(&self) -> HashMap<String, f64> {
        let total_weight: f64 = self
            .tenants
            .values()
            .filter(|t| !t.quarantined)
            .map(|t| t.weight)
            .sum();

        let mut allocations = HashMap::new();
        let mut allocated_total = 0.0;

        // First pass: allocate min(proportional_share, quota)
        let mut raw: Vec<(String, f64)> = Vec::new();
        for t in self.tenants.values().filter(|t| !t.quarantined) {
            let share = t.proportional_share(total_weight, self.total_resources);
            let alloc = share.min(t.quota);
            raw.push((t.id.clone(), alloc));
            allocated_total += alloc;
        }

        // Distribute surplus (from tenants whose quota < proportional share)
        let surplus = self.total_resources - allocated_total;
        if surplus > 0.0 && !raw.is_empty() {
            // Re-distribute surplus proportionally among tenants still under quota
            let remaining_weight: f64 = self
                .tenants
                .values()
                .filter(|t| !t.quarantined)
                .filter(|t| raw.iter().any(|(id, a)| id == &t.id && *a < t.quota))
                .map(|t| t.weight)
                .sum();

            if remaining_weight > 0.0 {
                for (id, base) in &mut raw {
                    let t = self.tenants.get(id).unwrap();
                    let headroom = t.quota - *base;
                    if headroom > 0.0 {
                        let bonus = ((t.weight / remaining_weight) * surplus).min(headroom);
                        *base += bonus;
                    }
                }
            }
        }

        for (id, alloc) in raw {
            allocations.insert(id, alloc);
        }

        allocations
    }

    /// Detect interference: returns tenant IDs whose quality is `Interference`.
    pub fn detect_interference(&self) -> Vec<String> {
        self.tenants
            .values()
            .filter(|t| t.quality == IsolationQuality::Interference)
            .map(|t| t.id.clone())
            .collect()
    }

    /// Quarantine a tenant: sets quality to Isolated and quarantined = true.
    /// Returns false if tenant not found.
    pub fn quarantine(&mut self, id: &str) -> bool {
        if let Some(t) = self.tenants.get_mut(id) {
            t.quarantined = true;
            t.quality = IsolationQuality::Isolated;
            true
        } else {
            false
        }
    }

    /// Release a tenant from quarantine.
    pub fn release_quarantine(&mut self, id: &str) -> bool {
        if let Some(t) = self.tenants.get_mut(id) {
            t.quarantined = false;
            t.quality = IsolationQuality::Shared;
            true
        } else {
            false
        }
    }

    /// Rebalance: re-run fair-share allocation and update each tenant's usage
    /// to match their new allocation. Returns the new allocations.
    pub fn rebalance(&mut self) -> HashMap<String, f64> {
        let allocations = self.fair_share_allocate();
        for (id, alloc) in &allocations {
            if let Some(t) = self.tenants.get_mut(id) {
                t.usage = *alloc;
            }
        }
        allocations
    }

    /// Update a tenant's weight and rebalance everyone.
    pub fn update_weight(&mut self, id: &str, new_weight: f64) -> bool {
        if let Some(t) = self.tenants.get_mut(id) {
            t.weight = new_weight;
            true
        } else {
            false
        }
    }

    /// Check if any tenant is over quota.
    pub fn over_quota_tenants(&self) -> Vec<String> {
        self.tenants
            .values()
            .filter(|t| t.is_over_quota())
            .map(|t| t.id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tenant_creation() {
        let t = Tenant::new("alice", 1.0, 50.0);
        assert_eq!(t.id, "alice");
        assert_eq!(t.weight, 1.0);
        assert_eq!(t.quota, 50.0);
        assert_eq!(t.usage, 0.0);
        assert_eq!(t.quality, IsolationQuality::Shared);
        assert!(!t.quarantined);
    }

    #[test]
    fn test_fair_share_equal_weights() {
        let mut mgr = TenancyManager::new(100.0);
        mgr.add_tenant(Tenant::new("a", 1.0, 100.0));
        mgr.add_tenant(Tenant::new("b", 1.0, 100.0));
        let alloc = mgr.fair_share_allocate();
        assert!((alloc["a"] - 50.0).abs() < 1e-6);
        assert!((alloc["b"] - 50.0).abs() < 1e-6);
    }

    #[test]
    fn test_fair_share_weighted() {
        let mut mgr = TenancyManager::new(100.0);
        mgr.add_tenant(Tenant::new("a", 3.0, 100.0));
        mgr.add_tenant(Tenant::new("b", 1.0, 100.0));
        let alloc = mgr.fair_share_allocate();
        assert!((alloc["a"] - 75.0).abs() < 1e-6);
        assert!((alloc["b"] - 25.0).abs() < 1e-6);
    }

    #[test]
    fn test_quota_cap() {
        let mut mgr = TenancyManager::new(100.0);
        mgr.add_tenant(Tenant::new("a", 1.0, 30.0)); // quota 30
        mgr.add_tenant(Tenant::new("b", 1.0, 100.0));
        let alloc = mgr.fair_share_allocate();
        assert!((alloc["a"] - 30.0).abs() < 1e-6);
        // b gets the rest: 70
        assert!((alloc["b"] - 70.0).abs() < 1e-6);
    }

    #[test]
    fn test_quarantine_removes_from_allocation() {
        let mut mgr = TenancyManager::new(100.0);
        mgr.add_tenant(Tenant::new("a", 1.0, 100.0));
        mgr.add_tenant(Tenant::new("b", 1.0, 100.0));
        mgr.quarantine("a");
        let alloc = mgr.fair_share_allocate();
        assert!(!alloc.contains_key("a"));
        assert!((alloc["b"] - 100.0).abs() < 1e-6);
    }

    #[test]
    fn test_interference_detection() {
        let mut mgr = TenancyManager::new(100.0);
        let mut t = Tenant::new("noisy", 1.0, 50.0);
        t.quality = IsolationQuality::Interference;
        mgr.add_tenant(t);
        mgr.add_tenant(Tenant::new("quiet", 1.0, 50.0));

        let interfering = mgr.detect_interference();
        assert_eq!(interfering, vec!["noisy"]);
    }

    #[test]
    fn test_rebalance_updates_usage() {
        let mut mgr = TenancyManager::new(100.0);
        mgr.add_tenant(Tenant::new("a", 1.0, 100.0));
        mgr.add_tenant(Tenant::new("b", 3.0, 100.0));
        let alloc = mgr.rebalance();
        assert!((mgr.get_tenant("a").unwrap().usage - alloc["a"]).abs() < 1e-6);
        assert!((mgr.get_tenant("b").unwrap().usage - alloc["b"]).abs() < 1e-6);
        // a gets 25, b gets 75
        assert!((alloc["a"] - 25.0).abs() < 1e-6);
        assert!((alloc["b"] - 75.0).abs() < 1e-6);
    }

    #[test]
    fn test_update_weight_and_rebalance() {
        let mut mgr = TenancyManager::new(100.0);
        mgr.add_tenant(Tenant::new("a", 1.0, 100.0));
        mgr.add_tenant(Tenant::new("b", 1.0, 100.0));
        mgr.update_weight("a", 3.0);
        let alloc = mgr.rebalance();
        assert!((alloc["a"] - 75.0).abs() < 1e-6);
        assert!((alloc["b"] - 25.0).abs() < 1e-6);
    }

    #[test]
    fn test_over_quota_detection() {
        let mut mgr = TenancyManager::new(100.0);
        let mut t = Tenant::new("heavy", 1.0, 30.0);
        t.usage = 45.0;
        mgr.add_tenant(t);
        mgr.add_tenant(Tenant::new("light", 1.0, 50.0));
        let over = mgr.over_quota_tenants();
        assert_eq!(over, vec!["heavy"]);
    }

    #[test]
    fn test_isolation_quality_from_i8() {
        assert_eq!(IsolationQuality::from_i8(1), Some(IsolationQuality::Isolated));
        assert_eq!(IsolationQuality::from_i8(0), Some(IsolationQuality::Shared));
        assert_eq!(IsolationQuality::from_i8(-1), Some(IsolationQuality::Interference));
        assert_eq!(IsolationQuality::from_i8(2), None);
    }

    #[test]
    fn test_release_quarantine() {
        let mut mgr = TenancyManager::new(100.0);
        mgr.add_tenant(Tenant::new("a", 1.0, 100.0));
        mgr.quarantine("a");
        assert!(mgr.get_tenant("a").unwrap().quarantined);
        mgr.release_quarantine("a");
        assert!(!mgr.get_tenant("a").unwrap().quarantined);
        assert_eq!(mgr.get_tenant("a").unwrap().quality, IsolationQuality::Shared);
    }
}
