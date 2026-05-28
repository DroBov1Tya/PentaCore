---
category: methodology
title: "Code Review"
tags: [checklist, code-review, source-code, static-analysis, sequential, whitebox]
---

# Code Review

Source code only, no live system.

## Orient

- What kind of app? Language/framework?
- Sketch entry points → business logic → data storage flow
- Find where external data enters (HTTP, files, env, IPC)

```bash
grep -rn "def\|func\|fn " src/ | grep -i "route\|handler\|endpoint\|controller"
```

## Git history (before reading code)

```bash
git log --all --oneline | grep -iE "fix|security|CVE|auth|bypass|remove|revert"
```

Read the diff for every security commit. Find the same pattern elsewhere.
→ `save_hypothesis()` for every potential variant

## Secrets and configuration

```bash
grep -rn "password\|secret\|api_key\|token\|private_key" . | grep -v test
find . -name ".env*" -o -name "config.*" -o -name "settings.*"
```

Is `.gitignore` actually covering secrets? Are there hardcoded credentials?
→ `save_finding()` for any hardcoded secret

## Auth and authorization

```bash
grep -rn "admin\|root\|privilege\|@login_required\|@auth" src/
grep -rn "def delete\|def update\|DELETE\|PUT\|PATCH" src/
```

For each privileged operation: is there a check before it? Can it be reached without the check?
→ `save_hypothesis()` for functions with no visible auth check

## Taint analysis: trace dangerous sinks

```bash
# SQL
grep -rn "execute\|query\|raw\|cursor" src/ | grep -v "prepare\|parameterize"
# Shell
grep -rn "exec\|system\|popen\|subprocess\|os.system" src/
# File
grep -rn "open(\|readFile\|writeFile\|include\|require" src/ | grep -v test
# Eval
grep -rn "eval\|exec(\|compile\|pickle\|unserialize" src/
```

For each: can attacker-controlled data reach it?

## Dependencies

```bash
npm audit        # Node.js
pip-audit        # Python
cargo audit      # Rust
mvn dependency-check:check  # Java
```

→ `save_finding()` for critical/high CVEs

## Language-specific patterns

- **Rust**: `grep -rn "unsafe"` - every unsafe block is mandatory review
- **Go**: `grep -rn "go func\|sync\."` - goroutine races
- **Python**: `grep -rn "pickle\|yaml.load\|eval\|shell=True"`
- **JS/TS**: `grep -rn "innerHTML\|dangerouslySetInnerHTML\|eval\|child_process"`

**Done when:** git log reviewed, all sinks traced, dependencies audited.
