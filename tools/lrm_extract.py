#!/usr/bin/env python3
"""Extract every code example from the PSS v3.0 LRM into individual .pss files.

The LRM lists 317 examples. Each begins with a line `Example N—Title` followed
by a code body (often spanning page breaks, which insert running headers and
footers). Fragments use `...` as an elision marker.

Outputs:
  examples/lrm/NNN_slug.pss   the extracted snippet, elisions removed
  examples/lrm/manifest.json  number, title, page, kind, wrap strategy
"""
import json
import os
import re

SRC = "/scratch/work/lrm/lrm.txt"
OUT = "/scratch/work/examples/lrm"

NOISE = (
    "Copyright ©",
    "Portable Test and Stimulus Standard",
    "=== PAGE",
)

DECL_KEYWORDS = (
    "action ", "component ", "struct ", "enum ", "buffer ", "resource ",
    "state ", "package ", "constraint ", "activity {", "extend ", "exec ",
)


def is_noise(line):
    s = line.strip()
    if s.startswith(NOISE):
        return True
    if re.fullmatch(r"\d{1,3}", s):
        return True
    return False


def is_prose(line):
    """A running-text line (as opposed to code) — used to end an example body."""
    s = line.strip()
    if not s:
        return False
    if s.startswith(("//", "/*", "*", "#")):
        return False
    words = s.split()
    if len(words) < 6:
        return False
    # A full sentence, with no code punctuation, is running text.
    return s.endswith(".") and s[0].isupper() and not any(c in s for c in "{};")


CODE_STARTERS = (
    "action", "component", "struct", "enum", "buffer", "resource", "state",
    "package", "constraint", "activity", "abstract", "pure", "extend",
    "import", "rand", "exec", "static", "typedef", "function", "target",
    "int", "bit", "list", "array", "set", "map", "ref", "scenario",
    "monitor", "symbol", "compile",
)


def looks_like_code(line):
    """A line that plausibly starts the code of an example (prologue filter)."""
    st = line.strip()
    if not st or "::=" in st:
        return False
    if st.startswith(("//", "/*", "*", "#")):
        return True
    words = st.split()
    # A long line is running text, even if it begins with a keyword.
    if len(words) >= 10:
        return False
    first = words[0] if words else ""
    if first in CODE_STARTERS:
        return True
    # Short line with code punctuation, e.g. `int data[4];`
    return any(c in st for c in ";{}") and len(words) <= 8


def slug(title):
    s = re.sub(r"[^a-z0-9]+", "_", title.lower()).strip("_")
    return (s or "example")[:48]


def main():
    lines = open(SRC).read().split("\n")
    os.makedirs(OUT, exist_ok=True)

    # Locate real example markers (skip the table of contents, which uses dots).
    starts = []
    for i, l in enumerate(lines):
        m = re.match(r"^\s*Example\s+(\d+)\s*[—–-]\s*(.*)$", l)
        if not m or "...." in l:
            continue
        title = m.group(2).strip()
        if not title:
            continue
        starts.append((i, int(m.group(1)), title))

    page = 0
    page_of_line = []
    for l in lines:
        m = re.match(r"=== PAGE (\d+) ===", l)
        if m:
            page = int(m.group(1))
        page_of_line.append(page)

    manifest = []
    seen = set()
    for idx, (i, num, title) in enumerate(starts):
        if num in seen:
            continue
        seen.add(num)
        end = starts[idx + 1][0] if idx + 1 < len(starts) else len(lines)

        body = []
        started = False
        for l in lines[i + 1 : end]:
            if is_noise(l):
                continue
            s = l.strip()
            if not s:
                if body:
                    body.append("")
                continue
            if started and is_prose(l):
                break
            if started and re.match(r"^[A-Za-z_][A-Za-z0-9_ ]*\s*::=", s):
                break  # trailing BNF after the code
            body.append(l.rstrip())
            started = True
        while body and not body[-1].strip():
            body.pop()

        # Drop a leading BNF/prose prologue so the snippet starts at real code.
        while body and not looks_like_code(body[0]):
            body.pop(0)

        # Drop elision markers.
        code = [l for l in body if l.strip() not in ("...", "…", "..")]
        code = [re.sub(r"\s*\.\.\.\s*$", "", l) for l in code]
        text = "\n".join(code).strip("\n")
        if not text:
            continue

        if re.search(r"^\s*(monitor\s+\w|\w+\s*:\s*(cover|monitor)\b|covergroup\b)", text, re.M):
            kind = "coverage"
        elif any(k in text for k in DECL_KEYWORDS):
            kind = "model"
        else:
            kind = "expression"
        has_decl = kind == "model"
        if not has_decl:
            kind = "expression"      # literal / operator snippet, not a model
        elif re.search(r"^\s*component\s+\w+", text, re.M):
            kind = "component"
        elif re.search(r"^\s*(package)\s+\w+", text, re.M):
            kind = "package"
        elif re.search(r"^\s*(abstract\s+)?action\s+\w+", text, re.M):
            kind = "action"
        else:
            kind = "declaration"

        name = "%03d_%s.pss" % (num, slug(title))
        with open(os.path.join(OUT, name), "w") as fh:
            fh.write("// PSS v3.0 LRM — Example %d: %s (page %d)\n" % (num, title, page_of_line[i]))
            fh.write(text + "\n")

        manifest.append({
            "num": num,
            "title": title,
            "page": page_of_line[i],
            "kind": kind,
            "file": name,
            "lines": len(code),
        })

    manifest.sort(key=lambda m: m["num"])
    with open(os.path.join(OUT, "manifest.json"), "w") as fh:
        json.dump(manifest, fh, indent=1)

    from collections import Counter
    print("example markers found:", len(starts))
    print("code extracted:", len(manifest))
    print("dropped (prose / grammar / elided):", len(starts) - len(manifest))
    print("by kind:", dict(Counter(m["kind"] for m in manifest)))


if __name__ == "__main__":
    main()
