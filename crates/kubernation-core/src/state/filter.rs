//! Namespace filtering — scope the *derived* world to one or more namespaces
//! without touching what the reflectors observe (they always watch all
//! namespaces). The filter is applied purely in `Models::build_filtered`, so
//! cities, the workload list, attention, and island structures all narrow
//! together while the terrain (nodes are cluster-scoped) stays put.

use std::collections::BTreeSet;

/// Which namespaces the operator wants to see.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum NamespaceFilter {
    /// No filter — show every namespace (the default).
    #[default]
    All,
    /// Show only these namespaces.
    Only(BTreeSet<String>),
}

impl NamespaceFilter {
    /// Focus a single namespace.
    pub fn only(namespace: impl Into<String>) -> Self {
        NamespaceFilter::Only(BTreeSet::from([namespace.into()]))
    }

    /// Does a namespaced object in `namespace` pass the filter?
    pub fn matches(&self, namespace: &str) -> bool {
        match self {
            NamespaceFilter::All => true,
            NamespaceFilter::Only(set) => set.contains(namespace),
        }
    }

    /// Like [`matches`], but for objects whose namespace may be absent
    /// (cluster-scoped custom resources). A cluster-scoped object belongs to
    /// no namespace, so it shows only when the filter is inactive.
    pub fn matches_opt(&self, namespace: Option<&str>) -> bool {
        match namespace {
            Some(ns) => self.matches(ns),
            None => !self.is_active(),
        }
    }

    /// Is a namespace restriction in effect (i.e. not `All`)?
    pub fn is_active(&self) -> bool {
        matches!(self, NamespaceFilter::Only(_))
    }

    /// Toggle one namespace in/out of the set (for a multi-select picker).
    /// Emptying the set reverts to `All`.
    pub fn toggle(&mut self, namespace: &str) {
        let mut set = match std::mem::take(self) {
            NamespaceFilter::All => BTreeSet::new(),
            NamespaceFilter::Only(s) => s,
        };
        if !set.remove(namespace) {
            set.insert(namespace.to_string());
        }
        *self = if set.is_empty() {
            NamespaceFilter::All
        } else {
            NamespaceFilter::Only(set)
        };
    }

    /// Is this exact namespace currently selected?
    pub fn contains(&self, namespace: &str) -> bool {
        matches!(self, NamespaceFilter::Only(s) if s.contains(namespace))
    }

    /// A short human label for status bars / chrome.
    pub fn label(&self) -> String {
        match self {
            NamespaceFilter::All => "all namespaces".into(),
            NamespaceFilter::Only(set) => {
                let names: Vec<&str> = set.iter().map(String::as_str).collect();
                if names.len() <= 2 {
                    format!("ns: {}", names.join(", "))
                } else {
                    format!("ns: {} +{}", names[0], names.len() - 1)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_matches_everything_only_matches_members() {
        let all = NamespaceFilter::All;
        assert!(all.matches("anything"));
        assert!(all.matches_opt(None));
        assert!(!all.is_active());

        let only = NamespaceFilter::only("demo");
        assert!(only.matches("demo"));
        assert!(!only.matches("kube-system"));
        // Cluster-scoped objects vanish under an active filter.
        assert!(!only.matches_opt(None));
        assert!(only.is_active());
    }

    #[test]
    fn toggle_builds_and_clears_the_set() {
        let mut f = NamespaceFilter::All;
        f.toggle("a");
        assert_eq!(f, NamespaceFilter::only("a"));
        f.toggle("b");
        assert!(f.contains("a") && f.contains("b"));
        f.toggle("a"); // remove
        assert!(!f.contains("a") && f.contains("b"));
        f.toggle("b"); // empties → back to All
        assert_eq!(f, NamespaceFilter::All);
    }
}
