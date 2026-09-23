#!/bin/sh
# Downloads the ASAM sample files too large for git into tests/fixtures/large.
set -eu

commit=4b747acf9a25f02f329eb1e640836cbd9a35952f
dest=$(dirname "$0")/../tests/fixtures/large
mkdir -p "$dest"
cd "$dest"

while read -r sum name; do
    if [ ! -f "$name" ] || ! echo "$sum  $name" | sha256sum -c --status; then
        curl -fsSL -o "$name" "https://raw.githubusercontent.com/asam-ev/OpenCRG/$commit/crg-bin/$name"
        echo "$sum  $name" | sha256sum -c
    fi
done <<'SUMS'
5138371c295594297905c028081d34a69e08168bfc1851584848eb5bc807fbc9  country_road.crg
faf8524952b1c519e800555ee19e829b9a4923b26b86589d287ff478d34fbaa0  crg_local_curv_test_fail.crg
ef90015c3492f905f5f7afc178ce207088dd0aa53090d5580c082d07973c1c9c  crg_local_curv_test_ok.crg
6f2d1ded0dc8625ee6fd3a2cd8fa1ebf0c5d901fd91ebf8fda3659078c653c5f  crg_refline_Hoki_HoeKi_Grafing.crg
SUMS
