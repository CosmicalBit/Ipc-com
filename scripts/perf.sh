#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
mode=${1:-}
if (($#)); then shift; fi

if ! command -v perf >/dev/null 2>&1; then
  echo "perf is missing; install the Linux perf tool for your kernel." >&2
  exit 1
fi

case $mode in
  stat|record)
    cargo build --profile profiling --example profile_ipc
    binary="${CARGO_TARGET_DIR:-target}/profiling/examples/profile_ipc"
    if [[ $mode == stat ]]; then
      perf stat -d -- "$binary" "$@" || {
        echo "perf stat failed; check perf permissions (kernel.perf_event_paranoid)." >&2
        exit 1
      }
    else
      perf record --call-graph dwarf,16384 --output perf.data -- "$binary" "$@" || {
        echo "perf record failed; check perf permissions (kernel.perf_event_paranoid)." >&2
        exit 1
      }
      echo "Open $(pwd)/perf.data in Hotspot, or run ./scripts/perf.sh report"
    fi
    ;;
  report)
    if (($#)); then echo "report takes no workload arguments" >&2; exit 2; fi
    if [[ ! -f perf.data ]]; then echo "perf.data is missing; run perf.sh record first." >&2; exit 1; fi
    perf report --input perf.data
    ;;
  *) echo "Usage: $0 {stat|record|report} [--payload BYTES] [--iterations COUNT]" >&2; exit 2 ;;
esac
