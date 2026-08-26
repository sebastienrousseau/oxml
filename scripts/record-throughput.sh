#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

# ---------------------------------------------------------------
# Record a throughput figure, or refuse to.
#
# doc/BENCHMARKS.md: a published figure carries its machine, its
# toolchain, its load average and criterion's confidence interval,
# because the same binary measured 14.7 and 123.1 MB/s on this machine
# on one day. The difference was load.
#
# So this script checks the conditions before it measures, and exits
# without a number if they are not met. A figure you had to override a
# check to obtain is not a measurement.
# ---------------------------------------------------------------

# Load per core, above which a figure is not recorded. The rule is
# "near zero"; a fifth of a core's worth of background work is the
# most that can be called that honestly.
MAX_LOAD_PER_CORE="0.20"

cores=$(sysctl -n hw.ncpu 2>/dev/null || nproc)
# The fifteen-minute average, which is the one that reflects whether
# the machine has actually been quiet rather than momentarily idle.
load15=$(uptime | sed 's/.*averages*: //' | awk '{print $3}' | tr -d ',')

per_core=$(awk -v l="$load15" -v c="$cores" 'BEGIN { printf "%.3f", l / c }')

echo "machine   : $(sysctl -n hw.model 2>/dev/null || uname -m), ${cores} cores"
echo "toolchain : $(rustc --version)"
echo "load (15m): ${load15}  (${per_core} per core)"
echo

if awk -v p="$per_core" -v m="$MAX_LOAD_PER_CORE" \
     'BEGIN { exit !(p > m) }'; then
  cat <<MSG
REFUSING to record a figure.

The fifteen-minute load average is ${per_core} per core, above the
${MAX_LOAD_PER_CORE} this repository treats as "near zero". A number
taken now would be truthful and useless: the spread between a quiet
and a busy machine here has been 8x.

Wait for the machine to settle and run this again. Nothing is written.
MSG
  exit 1
fi

echo "Conditions met. Measuring..."
echo
cargo bench --bench throughput -- --noplot

cat <<MSG

Recorded above. To publish, copy into doc/BENCHMARKS.md *with* the
machine, toolchain, load average and criterion's confidence interval
shown here. A delta inside the interval is not a result.
MSG
