---
category: mindset
title: "Ghost of Bugs Past - Regression Analysis"
tags: [regression-analysis, git-history, regresshion, zero-day-methodology, variant-analysis, openssh]
---

# Ghost of Bugs Past

Here's something nobody talks about enough: old fixed bugs come back.

The regreSSHion vulnerability was a signal handler race condition that got fixed in OpenSSH back in 2006. Fourteen years later, someone refactored the code, and the fix quietly disappeared. Nobody noticed for four years. Qualys found it because they have a practice of taking old security patches and checking whether they're still actually present in current code.

14 million servers. Unauthenticated RCE. A bug that was fixed in 2006.

---

Developers are focused on what they're building right now. They're not thinking about the patch they landed a decade ago. Refactoring moves code around. "Improvements" sometimes remove checks that were there for non-obvious reasons. Port to a new language? The invariants don't come for free.

Git history is basically a record of every time someone's mental model of the code was wrong enough to cause a security issue. Those mental models can be wrong again.

---

The technique is straightforward but tedious. Find the security commits. Read what they fixed. Go look for the same pattern in the current codebase.

```bash
# find security commits
git log --all --oneline | grep -iE "fix|security|vuln|overflow|injection|race|auth"

# look for commits that removed checks - the dangerous kind of "cleanup"
git log -p --all | grep "^-.*check\|^-.*valid\|^-.*assert"

# for a specific old fix: does it still exist?
git show <old-fix-hash> | grep "^+" | grep -v "^+++"
# then grep for that pattern in current source
```

The most interesting thing to look for: commits where the fix was later followed by a refactor. The fix gets tangled up with other changes, the security intent gets lost, and you end up with a regression nobody catches in code review because the reviewer is thinking about the refactor, not the decade-old security property.

Also worth doing: look for the same bug pattern copied to other parts of the codebase. A lot of projects have multiple implementations of similar logic - parsers, handlers, validation functions. Fix lands in one place, the other places never get updated. That's not a regression technically, but it's the same class of finding.
