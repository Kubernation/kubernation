//! A stable identifier for "which cluster is this, really".
//!
//! **Kubernetes has no cluster ID.** There is no field to read. The convention —
//! used by kubeadm, telemetry agents and others — is the UID of the `kube-system`
//! namespace, which is created once at cluster birth and never recreated in
//! normal operation. It is a *fingerprint*, and this module is careful to call
//! it that: describing it as an identifier would promise a guarantee the API
//! does not make.
//!
//! ## Why this is a new read surface, and how it is bounded
//!
//! `Namespace` is deliberately not in the watch set (13 kinds, none of them
//! namespaces — `ObservedWorld::namespaces()` derives the *names* from the
//! metadata of watched objects and never reads a Namespace object, so it cannot
//! yield a UID). This is therefore the only place the project reads one, and it
//! is bounded to match the posture rather than to match convenience:
//!
//! - **One object, by name, once per connection.** Not a list, not a watch.
//!   `logs::first_container` is the shape.
//! - **Failure is not fatal and is reported, not swallowed.** A cluster where
//!   this read is denied is ordinary, and `browse.rs`'s convention is to say
//!   what it could not see rather than to omit it silently. An unreadable
//!   fingerprint means *unverified*, never *mismatched* — conflating the two
//!   would throw away a working map every time an RBAC-restricted user opened
//!   it, and those are the users stability helps most.

use k8s_openapi::api::core::v1::Namespace;
use kube::{Api, Client};

/// The namespace whose UID stands in for cluster identity.
pub const FINGERPRINT_NAMESPACE: &str = "kube-system";

/// What a fingerprint read produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fingerprint {
    /// A UID was read.
    Read(String),
    /// It could not be read, and this is why. Not an error to propagate — the
    /// caller loads the layout unverified — but the reason is carried so it can
    /// be reported rather than guessed at.
    Unavailable(String),
}

impl Fingerprint {
    pub fn value(&self) -> Option<&str> {
        match self {
            Fingerprint::Read(s) => Some(s),
            Fingerprint::Unavailable(_) => None,
        }
    }
}

/// Read the cluster fingerprint. Never fails to the caller.
pub async fn read(client: Client) -> Fingerprint {
    let api: Api<Namespace> = Api::all(client);
    match api.get(FINGERPRINT_NAMESPACE).await {
        Ok(ns) => match ns.metadata.uid {
            Some(uid) if !uid.is_empty() => Fingerprint::Read(uid),
            // Present but with no UID is not a thing a real apiserver does, so
            // if it happens the honest answer is "unknown", not a made-up key.
            _ => Fingerprint::Unavailable(format!("{FINGERPRINT_NAMESPACE} has no uid")),
        },
        Err(e) => Fingerprint::Unavailable(classify(&e)),
    }
}

/// Turn a kube error into something worth showing an operator.
fn classify(e: &kube::Error) -> String {
    if let kube::Error::Api(err) = e {
        return match err.code {
            403 => format!("not permitted to read namespace/{FINGERPRINT_NAMESPACE}"),
            404 => format!("no namespace/{FINGERPRINT_NAMESPACE} on this cluster"),
            code => format!("could not read namespace/{FINGERPRINT_NAMESPACE} ({code})"),
        };
    }
    format!("could not read namespace/{FINGERPRINT_NAMESPACE}: {e}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unavailable_fingerprint_yields_no_value_rather_than_an_empty_one() {
        let f = Fingerprint::Unavailable("denied".into());
        assert_eq!(f.value(), None);
        // The distinction the whole identity check rests on: `None` here reaches
        // `layout_store::from_stored` as "absent", which loads unverified — it
        // must never arrive as `Some("")` and read as a mismatch.
        assert_eq!(Fingerprint::Read("uid-1".into()).value(), Some("uid-1"));
    }
}
