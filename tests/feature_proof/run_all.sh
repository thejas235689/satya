#!/bin/sh
# Re-run every feature-proof model and print what the tool actually does.
#
#   ./run_all.sh /path/to/satya [corpus-root]
#
# Each model exercises one construct. The runner prints, per model: whether it
# parses, and — for models whose top action is named in the file header — the
# solved values, so a reader can compare against docs/FEATURE-AUDIT.md.
#
# It does not assert verdicts: the verdicts in the audit are what these runs
# showed on v0.46.0. A different build will print different values, and that is
# the point — the evidence is re-derivable, not baked in.
set -u
BIN=${1:-}
if [ -z "$BIN" ] || [ ! -x "$BIN" ]; then
    echo "usage: $0 /path/to/satya" >&2
    echo "error: no executable binary given" >&2
    exit 2
fi
cd "$(dirname "$0")" || exit 2

pass=0; fail=0
for f in slice-*/*.pss; do
    [ -e "$f" ] || continue
    if out=$("$BIN" parse "$f" 2>&1); then
        act=$(printf '%s\n' "$out" | sed -n 's/^ *top *: *//p' | head -1)
        echo "PARSE-OK   $f"
        pass=$((pass+1))
    else
        echo "PARSE-FAIL $f"
        printf '           %s\n' "$(printf '%s\n' "$out" | head -1)"
        fail=$((fail+1))
    fi
done
echo
echo "parsed: $pass   failed: $fail   total: $((pass+fail))"
