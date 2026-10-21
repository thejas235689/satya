#!/usr/bin/env python3
"""Cross-check the OpenTitan UART PSS model against the IP's register spec.

`hw/ip/uart/data/uart.hjson` in lowRISC/opentitan is the authoritative
definition of the UART's registers and fields. This script extracts that
register map and compares it, name by name and width by width, against the
structs declared in `examples/opentitan/ot_uart.pss`.

A struct is matched to a register by the convention `uart_<register>_s`
(case-insensitive), and a field by its HJSON name lower-cased.

Usage:
    ot_regmap_check.py [uart.hjson] [ot_uart.pss]

With no arguments the HJSON is fetched from OpenTitan's default branch.
"""
import json
import os
import re
import sys
import urllib.request

HJSON_URL = ("https://raw.githubusercontent.com/lowRISC/opentitan/master/"
             "hw/ip/uart/data/uart.hjson")
HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_PSS = os.path.join(HERE, "..", "examples", "opentitan", "ot_uart.pss")


def load_hjson(path):
    if path and os.path.exists(path):
        return open(path, encoding="utf-8").read()
    print("# fetching %s" % HJSON_URL)
    with urllib.request.urlopen(HJSON_URL, timeout=30) as r:
        return r.read().decode("utf-8")


def parse_registers(text):
    """Return {REGISTER: {"sw": ..., "fields": {name_lower: width}}}."""
    lines = text.split("\n")
    starts = [(i, re.match(r'^    \{ name: "([A-Z][A-Z0-9_]*)"', l).group(1))
              for i, l in enumerate(lines)
              if re.match(r'^    \{ name: "([A-Z][A-Z0-9_]*)"', l)]
    regs = {}
    for k, (ln, name) in enumerate(starts):
        end = starts[k + 1][0] if k + 1 < len(starts) else len(lines)
        blk = "\n".join(lines[ln:end])
        if "swaccess" not in blk:
            continue
        sw = re.search(r'swaccess:\s*"(\w+)"', blk)
        fields = {}
        for fm in re.finditer(
            r'\{ bits:\s*"([0-9]+(?::[0-9]+)?)"(?:,\s*name:\s*"([A-Za-z0-9_]+)")?', blk
        ):
            bits, fname = fm.group(1), fm.group(2)
            if fname is None:
                continue
            hi, lo = ((int(x) for x in bits.split(":")) if ":" in bits
                      else (int(bits), int(bits)))
            fields[fname.lower()] = hi - lo + 1
        regs[name] = {"sw": sw.group(1) if sw else "?", "fields": fields}
    return regs


def parse_model(path):
    """Return {struct_name_lower: {field_lower: width}} for uart_*_s structs."""
    text = open(path, encoding="utf-8").read()
    structs = {}
    for m in re.finditer(r"struct\s+(uart_\w+_s)\s*\{", text):
        name = m.group(1)
        depth, i = 1, m.end()
        while i < len(text) and depth:
            if text[i] == "{":
                depth += 1
            elif text[i] == "}":
                depth -= 1
            i += 1
        body = text[m.end():i - 1]
        fields = {}
        for fm in re.finditer(r"\bbit\[(\d+)\]\s+(\w+)\s*;", body):
            fields[fm.group(2).lower()] = int(fm.group(1))
        structs[name.lower()] = fields
    return structs


def main():
    hjson_path = sys.argv[1] if len(sys.argv) > 1 else None
    pss_path = sys.argv[2] if len(sys.argv) > 2 else DEFAULT_PSS

    regs = parse_registers(load_hjson(hjson_path))
    model = parse_model(pss_path)

    print("OpenTitan UART: %d registers, %d modelled structs\n"
          % (len(regs), len(model)))
    print("%-14s %-6s %s" % ("REGISTER", "SW", "RESULT"))
    print("-" * 72)

    ok = bad = 0
    problems = []
    for reg, info in regs.items():
        key = "uart_%s_s" % reg.lower()
        if key not in model:
            if not info["fields"]:
                print("%-14s %-6s (whole-register, no named fields)" % (reg, info["sw"]))
                continue
            print("%-14s %-6s NOT MODELLED (no struct %s)" % (reg, info["sw"], key))
            problems.append("%s: register not modelled" % reg)
            bad += 1
            continue

        spec, got = info["fields"], model[key]
        missing = {k: v for k, v in spec.items() if k not in got}
        extra = {k: v for k, v in got.items() if k not in spec}
        width = {k: (v, got[k]) for k, v in spec.items() if k in got and got[k] != v}

        if not (missing or extra or width):
            print("%-14s %-6s match (%d fields)" % (reg, info["sw"], len(spec)))
            ok += 1
        else:
            print("%-14s %-6s MISMATCH" % (reg, info["sw"]))
            for k, v in missing.items():
                print("                   missing field %s[%d]" % (k, v))
                problems.append("%s.%s missing" % (reg, k))
            for k, v in extra.items():
                print("                   extra field   %s[%d]" % (k, v))
                problems.append("%s.%s extra" % (reg, k))
            for k, (a, b) in width.items():
                print("                   width %s: spec %d, model %d" % (k, a, b))
                problems.append("%s.%s width" % (reg, k))
            bad += 1

    print("-" * 72)
    print("registers matching: %d   mismatching: %d" % (ok, bad))
    if problems:
        print("\nproblems:")
        for p in problems:
            print("  -", p)
        sys.exit(1)


if __name__ == "__main__":
    main()
