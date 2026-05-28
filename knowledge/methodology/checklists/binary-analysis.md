---
category: methodology
title: "Binary Analysis"
tags: [checklist, binary, reverse-engineering, elf, pe, ghidra, radare2, fuzzing, sequential]
---

# Binary Analysis

You have a binary, no source.

## Reconnaissance (5 min, no execution)

```bash
file ./target                      # type, architecture, stripped?
checksec --file=./target           # ASLR, NX, PIE, stack canary, RELRO
strings ./target | head -200       # URLs, paths, errors, crypto hints
readelf -d ./target | grep NEEDED  # dynamic dependencies
sha256sum ./target                 # hash for VirusTotal / public databases
```

→ `save_scope()` - record architecture and protections

## Static analysis (without running)

```bash
strings ./target | grep -iE "password|secret|key|token|http|admin|debug|test"
strings ./target | grep -E "[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}"
objdump -d ./target | grep -A5 "call.*system\|call.*exec\|call.*popen"
nm ./target 2>/dev/null | grep -i "gets\|strcpy\|sprintf\|system"
```

## Dynamic analysis (run in isolation)

```bash
strace ./target 2>&1 | head -100
ltrace ./target 2>&1 | head -100
strace -e trace=network ./target
```

What does it read from the filesystem? What network connections? What env variables?

## Attack surface

What inputs does it accept? (argv, stdin, network, files, env)

```bash
echo "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA" | ./target
python3 -c "print('A'*10000)" | ./target
```

→ `save_hypothesis()` for every crash

## Reverse dangerous functions (Ghidra / IDA / radare2)

Priority targets:
1. Functions processing network or file input
2. Use of `gets`, `strcpy`, `sprintf`, `memcpy` without bounds check
3. Authentication functions (password comparison, key verification)
4. Crypto implementations (hardcoded keys, weak algorithms)

```bash
# radare2 quick start
r2 -A ./target
afl          # list functions
pdf @main    # disassemble main
```

## Vulnerability class checklist

- **Network daemon**: length fields in protocol → Heartbleed pattern
- **File parser**: malformed input, nested structures, recursion depth
- **Crypto**: hardcoded IV/key, ECB mode, homebrew algorithm
- **setuid binary**: all execution paths to privileged operation

→ `save_hypothesis()` for each suspicious location

## Fuzzing (if time permits)

```bash
afl-fuzz -i corpus/ -o findings/ -- ./target @@
```

**Done when:** checksec reviewed, strings analyzed, dynamic trace captured, critical functions disassembled.
