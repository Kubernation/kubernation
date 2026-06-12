CLUSTER ?= k8sciv
KCTX    := kind-$(CLUSTER)

.PHONY: dev kind-up samples run smoke kind-down lint test

## dev: full loop — cluster up, samples applied, TUI running
dev: kind-up samples run

## kind-up: create the 4-node dev cluster (idempotent) and wait for Ready
kind-up:
	@kind get clusters 2>/dev/null | grep -qx '$(CLUSTER)' || \
		kind create cluster --config hack/kind-config.yaml
	kubectl --context $(KCTX) wait --for=condition=Ready nodes --all --timeout=180s

## samples: apply demo workloads (healthy, crashing, stateful, daemon, stuck PVC)
samples:
	kubectl --context $(KCTX) apply -f hack/samples.yaml

## run: launch the TUI against the dev cluster
run:
	cargo run --release -- --context $(KCTX)

## smoke: headless connect + world summary (CI / sanity)
smoke:
	cargo run -- --context $(KCTX) --smoke

## kind-down: delete the dev cluster
kind-down:
	kind delete cluster --name $(CLUSTER)

## lint: formatting + clippy, the same gate as CI
lint:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

## test: unit + snapshot tests
test:
	cargo test
