#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
mode=${1:-all}
if (($# > 1)); then
  echo "Usage: $0 [normal|layout|asan|miri]" >&2
  exit 2
fi

require_nightly() {
  if ! rustc +nightly --version >/dev/null 2>&1; then
    echo "Nightly Rust is required: rustup toolchain install nightly" >&2
    exit 1
  fi
}

normal() {
  cargo test
}

layout() {
  require_nightly
  for seed in 1 2 3 4 5; do
    echo "Randomized layout seed: $seed"
    if ! RUSTFLAGS="-Zrandomize-layout -Zlayout-seed=$seed" cargo +nightly test --all-targets; then
      echo "Randomized layout failed at seed $seed" >&2
      return 1
    fi
  done
}

asan() {
  require_nightly
  if ! rustup component list --toolchain nightly --installed | grep -qx rust-src; then
    echo "ASan requires Nightly rust-src: rustup component add rust-src --toolchain nightly" >&2
    return 1
  fi
  local target
  target=$(rustc +nightly -vV | sed -n 's/^host: //p')
  if [[ $target != *-unknown-linux-gnu ]]; then
    echo "ASan mode requires a Linux GNU target; found $target" >&2
    return 1
  fi
  echo "AddressSanitizer with randomized layout seed 17 ($target)"
  # LeakSanitizer cannot run in ptrace-restricted CI/container environments.
  # AddressSanitizer remains enabled for bounds and use-after-free checks.
  ASAN_OPTIONS='detect_leaks=0' \
    RUSTFLAGS='-Zsanitizer=address -Zrandomize-layout -Zlayout-seed=17' \
    cargo +nightly test -Zbuild-std --target "$target" --all-targets
}

miri() {
  require_nightly
  if ! rustup component list --toolchain nightly --installed | grep -Eq '^miri($|-)'; then
    echo "Miri component missing: rustup component add miri --toolchain nightly" >&2
    return 1
  fi
  for seed in 1 2 3; do
    echo "Miri seed: $seed"
    MIRIFLAGS="-Zmiri-seed=$seed" cargo +nightly miri test
  done
}

case $mode in
  all) normal; layout; asan; miri ;;
  normal) normal ;;
  layout) layout ;;
  asan) asan ;;
  miri) miri ;;
  *) echo "Usage: $0 [normal|layout|asan|miri]" >&2; exit 2 ;;
esac
