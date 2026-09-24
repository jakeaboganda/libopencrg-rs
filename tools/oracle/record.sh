#!/bin/sh
# Records C-API results for every case in tools/oracle/cases.txt into tests/oracle/NAME.txt.
#
# Usage: tools/oracle/record.sh [ORACLE]
#
# ORACLE defaults to target/oracle/crg-oracle, built with:
#   cmake -S tools/oracle -B target/oracle -DCMAKE_BUILD_TYPE=Release
#   cmake --build target/oracle
#
# Each output line is "QUERY = RESULT". Cases whose fixture is missing are skipped.
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
oracle=${1:-$root/target/oracle/crg-oracle}
out=$root/tests/oracle
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$out"

grep -v '^#' "$root/tools/oracle/cases.txt" | while read -r name fixture options; do
    [ -n "$name" ] || continue
    file=$root/tests/fixtures/$fixture
    if [ ! -f "$file" ]; then
        echo "skip $name: $fixture is missing" >&2
        continue
    fi

    : >"$tmp/settings"
    for option in $options; do
        key=${option%%=*}
        value=${option#*=}
        case $key in
            border_mode_u) echo "opti 1 $value" ;;
            border_mode_v) echo "opti 2 $value" ;;
            border_offset_u) echo "optd 5 $value" ;;
            border_offset_v) echo "optd 6 $value" ;;
            *) echo "unknown option $key in case $name" >&2; exit 1 ;;
        esac >>"$tmp/settings"
    done

    # The seed depends only on the name, so reordering cases changes nothing.
    seed=$(printf '%s' "$name" | cksum | cut -d' ' -f1)
    seed=$((seed % 2147483646 + 1))
    echo range | "$oracle" "$file" 2>/dev/null >"$tmp/range"
    awk -v seed="$seed" -f "$root/tools/oracle/queries.awk" "$tmp/range" >"$tmp/queries"
    { echo range; cat "$tmp/queries"; } >"$tmp/all"

    cat "$tmp/settings" "$tmp/all" | "$oracle" "$file" 2>/dev/null \
        | tail -n +$(($(wc -l <"$tmp/settings") + 1)) >"$tmp/results"
    {
        echo "# Recorded by tools/oracle/record.sh from the ASAM OpenCRG C-API. Do not edit."
        echo "fixture $fixture"
        for option in $options; do echo "option $option"; done
        paste -d'=' "$tmp/all" "$tmp/results" | sed 's/=/ = /'
    } >"$out/$name.txt"
    echo "recorded $name: $(wc -l <"$tmp/all") queries"
done
