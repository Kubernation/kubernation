#!/usr/bin/env bash
# Every platform `release.yml` ships must have a target triple in `about.toml`.
#
# WHY THIS EXISTS
#
# The release archives CARRY `THIRD-PARTY-NOTICES.md`, and that file is generated
# from `about.toml`'s pinned target list — cargo-about resolves cfg per triple, so
# a platform absent from that list contributes none of its crates.
#
# `about.toml` pinned macOS + Linux while release.yml had shipped a Windows zip
# since v0.65.0. For six releases the notices distributed with the Windows binary
# omitted six crates only that binary links — including `schannel`, Windows' TLS
# implementation. Nothing was wrong with either file on its own; they simply
# disagreed, and no build step compared them. Recorded in the v0.62.0 About review
# and still true at v1.33.1, which is how long a two-list agreement survives when
# nothing checks it.
#
# A lint rather than a test: `about.toml` and a GitHub workflow are not reachable
# from Rust, and the failure is a legal-accuracy defect in a shipped artifact
# rather than a behaviour a test could observe.
set -euo pipefail
cd "$(dirname "$0")/.."

# The matrix `name:` values in release.yml, mapped to the triples they build.
# `macos-universal` is two triples lipo'd together, so it needs both.
#
# A `case`, not an associative array: macOS ships bash 3.2, where `declare -A`
# does not exist — the same version whose empty-array behaviour under `set -u`
# broke the macOS release script during its dry run.
triples_for() {
  case "$1" in
    macos-universal) echo "aarch64-apple-darwin x86_64-apple-darwin" ;;
    linux-x86_64)    echo "x86_64-unknown-linux-gnu" ;;
    windows-x86_64)  echo "x86_64-pc-windows-msvc" ;;
    *)               return 1 ;;
  esac
}

fail=0
# NOTE the underscore in the class. Without it this matched only
# `macos-universal`: `linux-x86_64` and `windows-x86_64` contain `_`, so the `$`
# anchor failed and they were silently skipped — the guard passed while checking
# one of three platforms. Caught by the mutation that removes the Windows target
# and expects a failure; it did not fail, and the guard was the reason.
shipped=$(grep -oE '^ +name: [a-z0-9_-]+ *(#.*)?$' .github/workflows/release.yml \
  | sed -E 's/^ +name: ([a-z0-9_-]+).*/\1/' | sort -u)

if [ "$(echo "$shipped" | wc -w)" -lt 3 ]; then
  echo "extracted fewer platforms than release.yml ships: $shipped" >&2
  echo "  The matrix parse is wrong — fix it before trusting a pass." >&2
  exit 1
fi

for platform in $shipped; do
  if ! want=$(triples_for "$platform"); then
    echo "release.yml ships '$platform', which this guard has no triples for." >&2
    echo "  Add it to triples_for() here AND to about.toml's targets." >&2
    fail=1
    continue
  fi
  for triple in $want; do
    if ! grep -q "\"$triple\"" about.toml; then
      echo "release.yml ships '$platform' but about.toml has no '$triple' target." >&2
      echo "  The notices in that archive would omit crates only it links." >&2
      fail=1
    fi
  done
done

if [ "$fail" -ne 0 ]; then
  echo >&2
  echo "After editing about.toml, regenerate with the PINNED cargo-about:" >&2
  echo "  cargo install --locked --version 0.9.2 cargo-about --features cli" >&2
  echo "  cargo about generate about.hbs -o crates/kubernation/THIRD-PARTY-NOTICES.md" >&2
  exit 1
fi
echo "about.toml covers every platform release.yml ships"
