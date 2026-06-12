/// The staged-intervention world.
///
/// In future phases, operator changes are staged here as a diff against
/// [`super::observed::ObservedWorld`] and committed as a deliberate
/// "end of turn" — the planning-turn model from the design doc. The MVP
/// never mutates the cluster, but the type exists so that work has an
/// anchored place in the architecture (and so nothing in the UI layer is
/// ever tempted to treat the observed world as writable).
#[derive(Debug, Default, Clone)]
pub struct PlannedWorld {}
