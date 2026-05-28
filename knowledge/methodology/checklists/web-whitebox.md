---
category: methodology
title: "Web Testing - Source Code + Live App"
tags: [checklist, web, whitebox, source-code, sequential, pentest]
---

# Web Testing - Source Code + Live App

You have both the source code and a live application.

## Git history first

```bash
git log --all --oneline | grep -iE "fix|security|CVE|auth|bypass|vuln|remove|revert"
```

For every interesting commit: read the diff, find the same pattern elsewhere in the codebase.
→ `save_hypothesis()` for each pattern found without a matching fix

## Architecture map

- Where is auth middleware? Which routes are protected, which aren't?
- Where is input validated - before or after business logic?
- What external dependencies exist? Run: `npm audit` / `pip-audit` / `cargo audit`
→ `save_scope()` for each service/component

## Auth flow in code

Find how tokens are created, verified, and where the check can be bypassed.

```bash
grep -rn "auth\|token\|session\|login\|password" src/ | grep -v test
```

Look for: early return that skips the check, OR instead of AND, check after action, JWT alg from header trusted blindly.
→ `save_hypothesis()` for each suspicious execution path

## Authorization matrix from code

```bash
grep -rn "@require_role\|@admin_required\|has_permission\|is_admin" src/
```

Endpoints without these decorators are candidates for missing auth. Check indirect access: A → B → C - is the binding verified at each level?
→ `save_hypothesis()` for unprotected endpoints

## Manual taint analysis

Find dangerous sinks first:

```bash
grep -rn "exec\|eval\|system\|query\|open(" src/ | grep -v "test\|#"
```

For each: trace the data back to its source. Is there validation on the path?
→ `save_hypothesis()` where user-controlled data reaches a dangerous sink

## Invariants that aren't checked

```bash
grep -rn "assert(\|// always\|// never\|// safe\|// trusted\|// guaranteed" src/
```

Shared resources without locks? TOCTOU windows?
→ `save_hypothesis()`

## Confirm in the live system

Every code hypothesis → verify manually against the running app. Measure timing baseline for suspicious endpoints.
→ `save_finding()` for confirmed, `save_dead_end()` for disproved

**Done when:** full git log reviewed, all auth paths covered, every hypothesis has an outcome.
