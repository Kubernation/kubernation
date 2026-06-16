CLUSTER ?= k8sciv
KCTX    := kind-$(CLUSTER)
PERF_CLUSTER ?= k8sciv-perf
PERF_KCTX    := kwok-$(PERF_CLUSTER)
WARM_CLUSTER ?= k8sciv-warm
WARM_KCTX    := kind-$(WARM_CLUSTER)

.PHONY: dev kind-up samples run smoke kind-down lint test \
        perf-up perf perf-test perf-down \
        warm-up warm-drift pair warm-down

## dev: full loop — cluster up, samples applied, TUI running
dev: kind-up samples run

## kind-up: create the 4-node dev cluster (idempotent) and wait for Ready
kind-up:
	@kind get clusters 2>/dev/null | grep -qx '$(CLUSTER)' || \
		kind create cluster --config hack/kind-config.yaml
	kubectl --context $(KCTX) wait --for=condition=Ready nodes --all --timeout=180s

## samples: apply demo workloads (healthy, crashing, stateful, daemon, stuck PVC)
samples:
	kubectl --context $(KCTX) apply -f hack/samples-crd.yaml
	kubectl --context $(KCTX) wait --for=condition=Established crd/gizmos.example.com --timeout=30s
	kubectl --context $(KCTX) apply -f hack/samples.yaml

## run: launch the TUI against the dev cluster
run:
	cargo run --release -- --context $(KCTX) --project gizmos.example.com

## smoke: headless connect + world summary (CI / sanity)
smoke:
	cargo run -- --context $(KCTX) --smoke

## metrics-up: install metrics-server on the dev cluster (kind needs
## --kubelet-insecure-tls); gauges switch from scheduling pressure to live
## usage within ~30s. Absent it, K8sCiv falls back automatically.
metrics-up:
	kubectl --context $(KCTX) apply -f https://github.com/kubernetes-sigs/metrics-server/releases/latest/download/components.yaml
	kubectl --context $(KCTX) patch -n kube-system deployment metrics-server --type=json \
		-p '[{"op":"add","path":"/spec/template/spec/containers/0/args/-","value":"--kubelet-insecure-tls"}]'
	kubectl --context $(KCTX) rollout status -n kube-system deployment/metrics-server --timeout=120s

## kind-down: delete the dev cluster
kind-down:
	kind delete cluster --name $(CLUSTER)

## warm-up: second kind cluster with the same samples (the warm standby)
warm-up:
	@kind get clusters 2>/dev/null | grep -qx '$(WARM_CLUSTER)' || \
		kind create cluster --config hack/kind-config.yaml --name $(WARM_CLUSTER)
	kubectl --context $(WARM_KCTX) wait --for=condition=Ready nodes --all --timeout=180s
	kubectl --context $(WARM_KCTX) apply -f hack/samples-crd.yaml
	kubectl --context $(WARM_KCTX) wait --for=condition=Established crd/gizmos.example.com --timeout=30s
	kubectl --context $(WARM_KCTX) apply -f hack/samples.yaml

## warm-drift: make the warm cluster drift (replica, image, missing workload)
warm-drift:
	kubectl --context $(WARM_KCTX) -n k8sciv-demo scale deploy/web --replicas=1
	kubectl --context $(WARM_KCTX) -n k8sciv-demo delete deploy crashy --ignore-not-found
	kubectl --context $(WARM_KCTX) -n k8sciv-demo set image daemonset/agent sleeper=busybox:1.37

## pair: run the TUI observing hot + warm side by side
pair:
	cargo run --release -- --context $(KCTX) --warm $(WARM_KCTX) --project gizmos.example.com

## warm-down: delete the warm cluster
warm-down:
	kind delete cluster --name $(WARM_CLUSTER)

## perf-up: kwok-simulated 100-node / 1000-pod cluster (needs kwokctl)
perf-up:
	@kwokctl get clusters 2>/dev/null | grep -qx '$(PERF_CLUSTER)' || \
		kwokctl create cluster --name $(PERF_CLUSTER)
	hack/perf-seed.sh $(PERF_KCTX)

## perf: run the TUI against the kwok perf cluster
perf:
	cargo run --release -- --context $(PERF_KCTX)

## perf-test: release-mode rebuild+frame latency budget (<100ms asserted)
perf-test:
	cargo test --release scale_rebuild -- --nocapture

## perf-down: delete the kwok perf cluster
perf-down:
	kwokctl delete cluster --name $(PERF_CLUSTER)

## gui: run the windowed client against the dev cluster
gui:
	cargo run --release -p k8sciv-gui -- --context $(KCTX) --project gizmos.example.com

## gui-pair: windowed client observing hot + warm archipelagos
gui-pair:
	cargo run --release -p k8sciv-gui -- --context $(KCTX) --warm $(WARM_KCTX) --project gizmos.example.com

## lint: formatting + clippy, the same gate as CI
lint:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings

## test: unit + snapshot tests
test:
	cargo test --workspace
