---
category: methodology
title: "Patch Diffing - Read Security Patches as Vulnerability Maps"
tags: [patch-diffing, variant-analysis, n-day, semgrep, zero-day-methodology, git, security-commits]
---

# Patch Diffing

A security patch shows you exactly what was broken. The developer already did the hard work of finding the bug - you're just reading their notes.

There are three reasons to do this: understanding a bug before its advisory drops, finding variant bugs in the same codebase, and finding the same pattern in other projects. All three are productive.

---

Finding the interesting commits:

```bash
git log --all --oneline | grep -iE "CVE|security|fix.*overflow|fix.*inject|fix.*bypass"

# what was removed (the vulnerable code)
git show <hash> | grep "^-" | grep -v "^---"

# what was added (the fix)
git show <hash> | grep "^+" | grep -v "^+++"
```

---

What to look for when you read a patch:

If they added a length check before a `memcpy`, ask where else in the codebase `memcpy` is called without one. If they added a NULL check, ask where else object fields are accessed without checking. If they added an auth check before a privileged operation, ask which other privileged operations are missing the same check.

```diff
-  memcpy(dst, src, len);
+  if (len > MAX_LEN) return ERROR;
+  memcpy(dst, src, len);
```

The question is always the same: where else does this pattern exist?

---

Automating the search:

```bash
# after you identify the vulnerable pattern
grep -rn "memcpy" src/ --include="*.c" | grep -v "check\|valid\|bound"

# or with semgrep for more precision
semgrep --pattern 'memcpy($DST, $SRC, $LEN)' --lang c src/
```

Look at ±20 lines of context around each match. Most will be fine. The ones without any validation nearby are worth a hypothesis.

---

The cross-project angle works when the bug is in a pattern that shows up everywhere. Heartbleed was a length validation issue in a TLS library - the same class of bug existed in other TLS implementations. Log4Shell was "user input gets evaluated" in a logging library - similar patterns exist in other Java logging frameworks. Once you understand the pattern abstractly, you're not limited to the one project where you found it.
