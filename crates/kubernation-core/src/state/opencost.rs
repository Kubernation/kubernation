//! OpenCost `/allocation` parsing — the first 3rd-party "source" adapter.
//!
//! OpenCost (CNCF) runs in-cluster and computes *invoice-grade* per-allocation
//! cost by reading the cloud billing APIs (amortizing spot/reserved/savings-plans)
//! and accounting for network/load-balancer/storage — the costs the
//! requests-derived [`cost`](crate::state::cost) model structurally cannot see.
//! KuberNation reaches it read-only through the kube API-server **service proxy**
//! (`k8s/opencost.rs`) — the same authenticated connection as the reflectors, no
//! port-forward, no new off-laptop egress.
//!
//! This module is the **pure** half: it parses the `/allocation` JSON into a typed
//! [`OpenCostData`]; [`cost::from_opencost`](crate::state::cost::from_opencost)
//! turns that into the same `CostReport` the overlay + advisor already render, so
//! when OpenCost is present it simply *replaces* the estimate, provenance-labelled.
//!
//! **Honest:** the numbers are imported, not KuberNation's own derivation — the UI
//! labels them "from OpenCost". `totalCost` is **cumulative over the query window**,
//! so we convert to an hourly rate (`totalCost / minutes × 60`) to match the rest of
//! the cost model.

use serde::Deserialize;

/// One OpenCost allocation, normalized to an hourly rate.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OcAllocation {
    pub namespace: String,
    /// The owning controller name (`aggregate=controller`); None for un-aggregated.
    pub controller: Option<String>,
    /// "deployment" / "statefulset" / "daemonset" / "replicaset" / … (lowercased).
    pub controller_kind: Option<String>,
    /// The node this allocation ran on (when OpenCost reports it).
    pub node: Option<String>,
    /// Total cost per hour ($) = totalCost ÷ minutes × 60.
    pub per_hour: f64,
    /// Per-hour breakdown (cpu + ram + pv + network + lb sums to ~per_hour).
    pub cpu_per_hour: f64,
    pub ram_per_hour: f64,
    pub pv_per_hour: f64,
    pub network_per_hour: f64,
    pub lb_per_hour: f64,
}

/// The parsed OpenCost allocation set.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OpenCostData {
    pub allocations: Vec<OcAllocation>,
    /// The `__idle__` allocation, per hour (unallocated cluster capacity OpenCost
    /// attributes to nobody) — present only when the query asked for it.
    pub idle_per_hour: f64,
    /// Whole-set total per hour (incl. idle) — a cross-check / headline.
    pub total_per_hour: f64,
}

// --- the wire shape (OpenCost /allocation response) ------------------------

#[derive(Deserialize)]
struct AllocResponse {
    /// One set per `step`; with no step, a single set covering the window.
    #[serde(default)]
    data: Vec<std::collections::HashMap<String, RawAlloc>>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawAlloc {
    properties: RawProps,
    minutes: f64,
    #[serde(rename = "cpuCost")]
    cpu_cost: f64,
    #[serde(rename = "ramCost")]
    ram_cost: f64,
    #[serde(rename = "pvCost")]
    pv_cost: f64,
    #[serde(rename = "networkCost")]
    network_cost: f64,
    #[serde(rename = "loadBalancerCost")]
    lb_cost: f64,
    // gpuCost / sharedCost / externalCost are ignored here (serde drops unknown
    // fields); they're already summed into totalCost, which drives per_hour.
    #[serde(rename = "totalCost")]
    total_cost: f64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawProps {
    namespace: String,
    node: Option<String>,
    controller: Option<String>,
    #[serde(rename = "controllerKind")]
    controller_kind: Option<String>,
}

/// The `__idle__` allocation key OpenCost uses when `includeIdle=true`.
pub const IDLE_KEY: &str = "__idle__";

/// Accumulator (sums across however many step-sets the response carries, so the
/// per-hour rate is correct = Σtotal ÷ Σminutes × 60 regardless of step count).
#[derive(Default)]
struct Acc {
    minutes: f64,
    total: f64,
    cpu: f64,
    ram: f64,
    pv: f64,
    net: f64,
    lb: f64,
    namespace: String,
    node: Option<String>,
    controller: Option<String>,
    controller_kind: Option<String>,
}

fn rate(value: f64, minutes: f64) -> f64 {
    if minutes > 0.0 {
        let r = value / minutes * 60.0;
        // `.max(0.0)` alone sanitizes NaN but lets Infinity through (a tiny `minutes`
        // or an overflowing `totalCost` → inf, poisoning the whole report); clamp it.
        if r.is_finite() { r.max(0.0) } else { 0.0 }
    } else {
        0.0
    }
}

/// OpenCost control keys are wrapped in double underscores (`__idle__`,
/// `__unmounted__`, `__unallocated__`, …). Only `__idle__` is meaningful here.
fn is_control_key(k: &str) -> bool {
    k.len() >= 4 && k.starts_with("__") && k.ends_with("__")
}

/// Parse an OpenCost `/allocation` JSON body. Tolerant — unknown fields ignored,
/// missing fields default to 0; never panics. Returns the typed set.
pub fn parse_allocation(body: &str) -> Result<OpenCostData, String> {
    let resp: AllocResponse =
        serde_json::from_str(body).map_err(|e| format!("OpenCost JSON: {e}"))?;

    let mut by_key: std::collections::HashMap<String, Acc> = std::collections::HashMap::new();
    let mut idle = Acc::default();
    for set in &resp.data {
        for (key, a) in set {
            // __idle__ → its own bucket; any OTHER control key (__unmounted__,
            // __unallocated__, …) is skipped so it never becomes a blank-namespace
            // workload row (and doesn't inflate the total) — a minor undercount of
            // unattributed PV cost, accepted for cleanliness.
            if key != IDLE_KEY && is_control_key(key) {
                continue;
            }
            let acc = if key == IDLE_KEY {
                &mut idle
            } else {
                by_key.entry(key.clone()).or_default()
            };
            acc.minutes += a.minutes;
            // totalCost already includes cpu+ram+pv+network+lb+gpu+shared+external,
            // so per_hour is computed from it; the breakdown fields are for display
            // and intentionally don't fold in gpu/shared/external.
            acc.total += a.total_cost;
            acc.cpu += a.cpu_cost;
            acc.ram += a.ram_cost;
            acc.pv += a.pv_cost;
            acc.net += a.network_cost;
            acc.lb += a.lb_cost;
            if acc.namespace.is_empty() {
                acc.namespace = a.properties.namespace.clone();
                acc.node = a.properties.node.clone();
                acc.controller = a.properties.controller.clone();
                acc.controller_kind = a.properties.controller_kind.clone();
            }
        }
    }

    let mut data = OpenCostData {
        idle_per_hour: rate(idle.total, idle.minutes),
        ..Default::default()
    };
    for (_, acc) in by_key {
        let per_hour = rate(acc.total, acc.minutes);
        data.total_per_hour += per_hour;
        data.allocations.push(OcAllocation {
            namespace: acc.namespace,
            controller: acc.controller.filter(|c| !c.is_empty()),
            controller_kind: acc.controller_kind.filter(|c| !c.is_empty()),
            node: acc.node.filter(|n| !n.is_empty()),
            per_hour,
            cpu_per_hour: rate(acc.cpu, acc.minutes),
            ram_per_hour: rate(acc.ram, acc.minutes),
            pv_per_hour: rate(acc.pv, acc.minutes),
            network_per_hour: rate(acc.net, acc.minutes),
            lb_per_hour: rate(acc.lb, acc.minutes),
        });
    }
    data.total_per_hour += data.idle_per_hour;
    // Stable order for display / tests.
    data.allocations
        .sort_by(|a, b| (&a.namespace, &a.controller).cmp(&(&b.namespace, &b.controller)));
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A trimmed real-shape /allocation response: one set, two workloads + idle.
    const SAMPLE: &str = r#"{
      "code": 200, "status": "success",
      "data": [ {
        "web": {
          "name": "web",
          "properties": { "namespace": "demo", "node": "n1", "controller": "web", "controllerKind": "deployment" },
          "minutes": 60.0,
          "cpuCost": 0.10, "ramCost": 0.05, "pvCost": 0.0, "networkCost": 0.02, "loadBalancerCost": 0.03,
          "gpuCost": 0.0, "sharedCost": 0.0, "externalCost": 0.0, "totalCost": 0.20
        },
        "db": {
          "name": "db",
          "properties": { "namespace": "demo", "node": "n2", "controller": "db", "controllerKind": "statefulset" },
          "minutes": 120.0,
          "cpuCost": 0.10, "ramCost": 0.10, "pvCost": 0.40, "networkCost": 0.0, "loadBalancerCost": 0.0,
          "gpuCost": 0.0, "sharedCost": 0.0, "externalCost": 0.0, "totalCost": 0.60
        },
        "__idle__": {
          "name": "__idle__",
          "properties": { "namespace": "" },
          "minutes": 60.0,
          "cpuCost": 0.30, "ramCost": 0.10, "totalCost": 0.40
        }
      } ]
    }"#;

    #[test]
    fn parses_allocations_to_hourly_rates() {
        let d = parse_allocation(SAMPLE).unwrap();
        assert_eq!(d.allocations.len(), 2);
        // web: $0.20 over 60 min = $0.20/hr.
        let web = d
            .allocations
            .iter()
            .find(|a| a.controller.as_deref() == Some("web"))
            .unwrap();
        assert!((web.per_hour - 0.20).abs() < 1e-9, "{}", web.per_hour);
        assert_eq!(web.controller_kind.as_deref(), Some("deployment"));
        assert_eq!(web.node.as_deref(), Some("n1"));
        assert!((web.lb_per_hour - 0.03).abs() < 1e-9);
        // db: $0.60 over 120 min = $0.30/hr; its PV cost is real (the invisible line).
        let db = d
            .allocations
            .iter()
            .find(|a| a.controller.as_deref() == Some("db"))
            .unwrap();
        assert!((db.per_hour - 0.30).abs() < 1e-9, "{}", db.per_hour);
        assert!(
            (db.pv_per_hour - 0.20).abs() < 1e-9,
            "PV cost normalized to /hr"
        );
        // idle: $0.40 over 60 min = $0.40/hr.
        assert!((d.idle_per_hour - 0.40).abs() < 1e-9);
        // total = 0.20 + 0.30 + 0.40 idle = 0.90.
        assert!(
            (d.total_per_hour - 0.90).abs() < 1e-9,
            "{}",
            d.total_per_hour
        );
    }

    #[test]
    fn skips_control_keys_and_clamps_infinity() {
        // __unmounted__ (empty namespace) must NOT become a workload row, and a
        // tiny-minutes overflow must clamp to 0, not Infinity.
        let body = r#"{"data":[{
          "web": {"properties":{"namespace":"demo","controller":"web","controllerKind":"deployment"},"minutes":60,"totalCost":0.5},
          "__unmounted__": {"properties":{"namespace":""},"minutes":60,"totalCost":99.0},
          "__unallocated__": {"properties":{"namespace":""},"minutes":60,"totalCost":3.0},
          "boom": {"properties":{"namespace":"x","controller":"boom"},"minutes":1e-10,"totalCost":1e300}
        }]}"#;
        let d = parse_allocation(body).unwrap();
        // web + boom only; the two __…__ control keys dropped.
        assert_eq!(d.allocations.len(), 2);
        assert!(
            d.allocations
                .iter()
                .all(|a| a.controller.as_deref() != Some("__unmounted__"))
        );
        // 1e300 / 1e-10 * 60 overflows to Infinity → clamped to 0, not poisoning the total.
        let boom = d
            .allocations
            .iter()
            .find(|a| a.controller.as_deref() == Some("boom"))
            .unwrap();
        assert_eq!(boom.per_hour, 0.0, "Infinity clamped to 0");
        assert!(d.total_per_hour.is_finite());
        // total = web 0.5 + boom 0 + 0 idle.
        assert!(
            (d.total_per_hour - 0.5).abs() < 1e-9,
            "{}",
            d.total_per_hour
        );
    }

    #[test]
    fn tolerates_garbage_and_empty() {
        assert!(parse_allocation("not json").is_err());
        assert_eq!(
            parse_allocation(r#"{"data":[]}"#)
                .unwrap()
                .allocations
                .len(),
            0
        );
        // missing fields default to 0, zero minutes → 0 rate (no divide-by-zero).
        let d = parse_allocation(
            r#"{"data":[{"x":{"properties":{"namespace":"a"},"minutes":0,"totalCost":5}}]}"#,
        )
        .unwrap();
        assert_eq!(d.allocations.len(), 1);
        assert_eq!(d.allocations[0].per_hour, 0.0);
    }
}
