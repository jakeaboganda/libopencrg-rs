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
#
# A second pass maps every recorded xy result A back to uv: "reset", "uv A", then "uv B", where
# B is the next point's xy nudged off it, then "reset", "uv B". A uv query right after reset
# searches globally; one after another uv query starts from the previous result.
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

    paste -d' ' "$tmp/all" "$tmp/results" | awk '
        BEGIN { n = 0 }
        $1 == "xy" { x[n] = $4; y[n] = $5; n++ }
        END {
            for (i = 0; i < n; i++) {
                j = (i + 1) % n
                bx = sprintf("%.17g", x[j] + 0.013); by = sprintf("%.17g", y[j] - 0.021)
                print "reset"; print "uv " x[i] " " y[i]; print "uv " bx " " by
                print "reset"; print "uv " bx " " by
            }
        }' >"$tmp/uv"
    "$oracle" "$file" <"$tmp/uv" 2>/dev/null >"$tmp/uv_results"
    cat "$tmp/uv" >>"$tmp/all"
    cat "$tmp/uv_results" >>"$tmp/results"
    {
        echo "# Recorded by tools/oracle/record.sh from the ASAM OpenCRG C-API. Do not edit."
        echo "fixture $fixture"
        for option in $options; do echo "option $option"; done
        paste -d'=' "$tmp/all" "$tmp/results" | sed 's/=/ = /'
    } >"$out/$name.txt"
    echo "recorded $name: $(wc -l <"$tmp/all") queries"
done
