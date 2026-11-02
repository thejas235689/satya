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
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "lrm")

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


def strip_code(text):
    """Drop strings and comments so brace and quote counting is not fooled."""
    out, i, n = [], 0, len(text)
    while i < n:
        two = text[i:i+2]
        if two == "//":
            while i < n and text[i] != "\n":
                i += 1
            continue
        if two == "/*":
            i += 2
            while i < n and text[i:i+2] != "*/":
                i += 1
            i += 2
            continue
        ch = text[i]
        if ch == '"':
            i += 1
            closed = False
            while i < n:
                if text[i] == "\\":
                    i += 2
                    continue
                if text[i] == '"':
                    closed = True
                    i += 1
                    break
                # A target-code string may span lines, so do not stop at one.
                i += 1
            if not closed:
                out.append('"')     # an unterminated string survives as a quote
            continue
        if ch == "'" and i + 1 < n and text[i+1] in "bhdBHdD":
            # A based literal (`4'b1010`, `8'hFF`) is not a char literal.
            i += 2
            while i < n and (text[i].isalnum() or text[i] in "_xXzZ?"):
                i += 1
            continue
        if ch == "'":
            i += 1
            closed = False
            while i < n:
                if text[i] == "\\":
                    i += 2
                    continue
                if text[i] == "'":
                    closed = True
                    i += 1
                    break
                if text[i] == "\n":
                    break
                i += 1
            if not closed:
                out.append("'")
            continue
        out.append(ch)
        i += 1
    return "".join(out)


def prose_paragraph(line):
    """A documentation paragraph swept in with the code.

    Catches what the apostrophe test misses: lines of running text with no
    statement punctuation. The earlier version of this was too aggressive only
    because based literals (`4'b1010`) were being read as unterminated character
    literals; that is fixed, so the test is safe again.
    """
    st = strip_code(line).strip()
    if not st or st.startswith("//"):
        return False
    if re.search(r"[;{}()=<>\[\]]", st):
        return False
    words = st.split()
    return len(words) >= 6 and re.match(r"^[A-Za-z]", st) is not None


def classify(text):
    """ok | prose | truncated.

    Two structural signals only: an apostrophe inside a word (documentation
    text, never PSS), and a snippet that does not balance. Prose paragraphs are
    detected by `prose_paragraph` but are *evidence* for the harness, not an
    exclusion here: a heuristic cannot tell a fragment from damage, and both
    looser and tighter versions of this flagged examples that parse fine.
    """
    if re.search(r"\w'\w", strip_code(text)):
        return "prose"

    depth = 0
    for ch in strip_code(text):
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth < 0:
                return "truncated"
    if depth != 0:
        return "truncated"
    if '"' in strip_code(text) or "'" in strip_code(text):
        return "truncated"      # an unterminated string or literal
    return "ok"


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
            "status": classify(text),
        })

    manifest.sort(key=lambda m: m["num"])
    with open(os.path.join(OUT, "manifest.json"), "w") as fh:
        json.dump(manifest, fh, indent=1)

    from collections import Counter
    print("example markers found:", len(starts))
    print("code extracted:", len(manifest))
    print("dropped (prose / grammar / elided):", len(starts) - len(manifest))
    print("by kind:", dict(Counter(m["kind"] for m in manifest)))
    print("by status:", dict(Counter(m["status"] for m in manifest)))
    bad = [m for m in manifest if m["status"] != "ok"]
    if bad:
        print("damaged by extraction (excluded from the parse figure):")
        for m in bad:
            print("  Ex%-4d %-10s %s" % (m["num"], m["status"], m["title"][:46]))


if __name__ == "__main__":
    main()
