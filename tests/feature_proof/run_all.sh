#!/bin/sh
# Run the complete feature proof: six slices, 146 constructs, 16 CLI/backend items.
#
#   ./run_all.sh /path/to/satya
#
# Each slice script is self-contained (its models are embedded), asserts on
# observed behaviour, and prints one PASS/FAIL line per item. This runner
# aggregates them.
set -u
BIN=${1:-}
if [ -z "$BIN" ] || [ ! -x "$BIN" ]; then
    echo "usage: $0 /path/to/satya" >&2
    echo "error: no executable binary given" >&2
    exit 2
fi
case "$BIN" in /*) ;; *) BIN=$(pwd)/$BIN ;; esac
cd "$(dirname "$0")" || exit 2

pass=0; fail=0; slices_failed=0
for i in 1 2 3 4 5 6; do
    out=$(mktemp)
    ./proof-slice-$i.sh "$BIN" >"$out" 2>&1
    rc=$?
    p=$(grep -cE '^PASS ' "$out")
    f=$(grep -cE '^FAIL ' "$out")
    pass=$((pass+p)); fail=$((fail+f))
    [ "$rc" -ne 0 ] && slices_failed=$((slices_failed+1))
    printf 'slice %s: %2d pass  %2d fail\n' "$i" "$p" "$f"
    grep -E '^FAIL ' "$out" | sed 's/^/    /'
    rm -f "$out"
done

echo
echo "TOTAL: $pass pass, $fail fail, $((pass+fail)) checks"
[ "$slices_failed" -eq 0 ] && exit 0 || exit 1
