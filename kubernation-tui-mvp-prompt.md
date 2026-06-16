# Project: Civilization-Inspired Kubernetes TUI — MVP

## Context

You are implementing the MVP of a terminal user interface for observing and managing Kubernetes clusters. The conceptual foundation is the user-interface grammar of early Sid Meier's Civilization (Civ 1/2): a spatial, tile-based main view with overlays, a "city screen" for detailed entity management, an attention queue that surfaces what needs the operator's focus, and (in later phases) a planning-turn intervention model.

This is **not** a retro skin on k9s. It's a genuinely different operator UX that treats Kubernetes objects as having spatial structure, that surfaces problems to the operator instead of requiring them to go looking, and that frames intervention as deliberate staged changes rather than imperative edits.

The user is a Rust-first developer who has been designing this concept and is ready to see a working MVP. He uses Claude Code as his principal agentic implementor and works across macOS (M4 Max primary), Linux, and Windows.

## Conceptual Model

The CNCF cloud-native landscape has six layers (Provisioning, Runtime, Orchestration & Management, App Definition & Development, Observability & Analysis, Platforms). Reframe these as **concentric zones of operator agency**:

- **Provisioning** = the continent (cloud account, VPC, IAM). Read-mostly, slow-changing. Out of MVP scope.
- **Runtime** = terrain type per node (container runtime, CNI, CSI, GPU). Tile attribute shown on inspection.
- **Orchestration** = the main game board. Kubernetes objects, the primary view.
- **App Definition** = what your "cities" produce (Deployments, StatefulSets, Jobs, the data services they own).
- **Observability** = how anything is visible at all. Not a separate view; a property of every view.
- **Platforms** = the politics of the world (EKS vs GKE vs ARO vs on-prem). Cluster metadata, contextual.

Future scope (not MVP but design for):

- **Hot + warm cluster pair** as two continents shown side-by-side or unified with sync-state badges.
- **External managed services** (RDS, S3, managed Kafka, third-party APIs) as foreign powers with diplomatic relations and visible trade routes.
- **Chaos engineering events** as barbarian raids — hostile-but-bounded units with TTLs, blast radius, and abort conditions.

## MVP Scope

Build a working terminal application that does the following.

### 1. Main Map View

- Renders nodes as tiles in a grid laid out by zone/region (failure domain). Within a zone, nodes are positioned by stable hash so the layout does not reshuffle on every reconcile.
- Each node tile shows: pod count, CPU pressure indicator, memory pressure indicator, conditions (Ready, MemoryPressure, DiskPressure, PIDPressure), and schedulability.
- Color encodes a single primary signal by default (pressure level); overlays toggleable via hotkey (replica health, ownership by namespace, etc.).
- Cursor navigation via arrow keys and vim-style h/j/k/l. Enter on a node opens its detail view.
- Visible viewport with scroll for clusters larger than the terminal; minimap in a corner if cluster exceeds viewport.

### 2. Workload Detail View ("City Screen")

- Triggered by selecting a Deployment, StatefulSet, or DaemonSet from a workload list, or by drilling in from a node tile.
- Shows: desired/ready/available replicas, current rollout status, pod list with individual status, recent events, owned resources (ConfigMaps, Secrets, PVCs, Services).
- This is the equivalent of Civ's city screen — one workload, full context, no mode switching.

### 3. Attention Queue

- A persistent panel (always visible or summonable) listing current operational concerns in priority order: failing pods, stuck rollouts, pending PVCs, nodes under pressure, recent warning events, workloads with replica gaps.
- Hotkey to cycle to the "next concern" (the analog of Civ's "next unit needing orders").
- Each entry is selectable and opens the relevant detail view.

### 4. Live Updates

- Uses `kube-rs` watchers/reflectors so the UI reflects cluster state changes within seconds, with no manual refresh.
- A local cache (the "observed world") is the rendering source of truth; informers update it continuously.

### 5. Cluster Context

- Reads kubeconfig from the standard locations.
- Allows context switching via hotkey or command palette.
- Shows current context name, cluster API endpoint, and platform hint (EKS/GKE/AKS/kind/etc.) prominently.

### Explicitly Out of MVP Scope

- Mutations of any kind. **Observe only.** No `kubectl apply`, no scaling, no rollout commands.
- Hot/warm cluster pairing (but: structure the codebase so a second cluster context could be added cleanly later).
- External managed service integration.
- Chaos engineering integration.
- Planning-turn diff staging UI.
- Service mesh visualization.
- Logs and live tail (events only for MVP).
- Custom resources beyond standard workload kinds.

## Technical Requirements

### Stack

- **Rust** (latest stable).
- **ratatui** (current version) for rendering.
- **ratatui-crossterm** as the backend (per ratatui 0.30+ split; re-export the crossterm version from `ratatui_crossterm::crossterm` to avoid version skew).
- **tokio** as the async runtime.
- **kube-rs** with the `runtime` feature for reflectors and informers.
- **color-eyre** for error reporting.
- **tracing** + `tracing-subscriber` for diagnostics, writing to a log file (not stderr, which would corrupt the TUI).
- **serde** for any config and persisted state.
- **clap** for CLI arguments (kubeconfig path, context override, log level).

### Architecture

- Component-based pattern modeled on the ratatui async-template approach: each major view is a component implementing a common trait (`handle_event`, `update`, `render`).
- Clear separation between the **k8s data layer** (informers, cache, event stream) and the **UI layer** (components, layout, rendering).
- Two state containers, even though only one is used in MVP:
  - `ObservedWorld` — populated by informers, the read-only view of actual cluster state.
  - `PlannedWorld` — stub for now (an empty struct), but the type exists so future planning-turn work has its place.
- Event loop pattern: a single `tokio::select!` over UI input events, informer updates, and tick events.
- Configuration via a `Config` struct loaded from `~/.config/<projectname>/config.toml` if present, with sensible defaults.

### Code Quality

- Modules organized by concern: `k8s/` (data layer), `ui/` (components and layout), `state/` (worlds), `events/` (event types and dispatcher), `config/`, `main.rs`.
- Unit tests for state transitions and any non-trivial logic. UI rendering tests using ratatui's `TestBackend` for snapshot-style assertions on at least the main map and workload detail views.
- `cargo clippy -- -D warnings` clean.
- `cargo fmt` clean.
- A `CLAUDE.md` at the repo root capturing architectural decisions, the conceptual model summary, planned-but-not-implemented features, and conventions for future agent sessions.
- A `README.md` for humans with quick-start, screenshots (ASCII art is fine), and the conceptual pitch.

### Testing & Verification

- Should work against a `kind` cluster out of the box. Include a `Makefile` or `justfile` target that spins up a kind cluster, applies a small set of sample workloads (a healthy Deployment, an intentionally failing Deployment, a StatefulSet with a PVC, a DaemonSet), and launches the TUI against it.
- Should handle reasonable cluster sizes (50–100 nodes, 500–1000 pods) without UI lag.

## Design Notes

- **Aesthetic target: wargame, not retro pixel.** Think Hearts of Iron or the SSI strategic titles. Disciplined use of color, dense but legible layout, Unicode block characters and box-drawing for structure.
- **Keyboard-first.** Vim-style navigation (h/j/k/l) alongside arrows. A discoverable hotkey map shown via `?`. Mouse support is acceptable but not required for MVP.
- **Color discipline.** Color encodes meaning, not decoration. A Running pod is not green — it is the absence of red. Reserve saturated colors for things that need attention. Support both 256-color and truecolor terminals; degrade gracefully on monochrome.
- **Layout algorithm.** Nodes laid out in zone columns, ordered within a zone by stable hash of node name. Do not recompute layout on every update.
- **Symbol grammar.** Establish a consistent visual vocabulary early and document it in `CLAUDE.md`. For example: `▣` healthy node, `▤` cordoned, `▥` under pressure, `▦` unschedulable. Pods within a node tile shown as colored glyphs.

## Acceptance Criteria

When this is done:

1. `cargo run -- --context my-cluster` connects to the cluster and renders the main map within 2 seconds.
2. Creating a Pod via `kubectl` is reflected in the TUI within 5 seconds without manual refresh.
3. A Pod entering `CrashLoopBackOff` appears in the attention queue within 5 seconds.
4. Pressing `?` shows a complete keymap.
5. Pressing the configured "next concern" key cycles through the attention queue and opens each item's detail view.
6. The main map remains responsive (sub-100ms input latency) on a cluster with 50 nodes and 500 pods.
7. Quitting via `q` or Ctrl+C restores the terminal to a clean state (no leftover alternate-screen artifacts).
8. `cargo test` passes. `cargo clippy -- -D warnings` is clean.
9. `CLAUDE.md` and `README.md` exist and accurately describe the project.

## Process Guidance

- **Start with a vertical slice.** Get end-to-end working with one node tile, one workload detail view, and one attention-queue entry before broadening. Do not build all the data plumbing first and the UI later, or vice versa.
- **Use the `kind` integration target as your dev loop.** Do not develop against a remote cluster.
- **Commit in working states** with descriptive messages. The user reviews commits.
- **When making architectural decisions** that aren't fully specified here, document them in `CLAUDE.md` and proceed. Don't ask for clarification on every choice; do ask when the choice would significantly change the shape of future work.
- **Suggest a project name** if you have a good one. Civilization-flavored, aviation-flavored (the user maintains a TowerOps tooling family that includes VOR, Waypoint, Radar, GPS), or k8s-flavored options are all welcome.

When you're done, provide a brief tour: what was built, what was deferred, what surprised you, and what the next phase should focus on.
