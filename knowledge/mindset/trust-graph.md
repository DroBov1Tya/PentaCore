---
category: mindset
title: "The Trust Graph"
tags: [trust-model, ssh-agent, baron-samedit, zero-day-methodology, privilege-escalation, implicit-assumptions]
---

# The Trust Graph

Every piece of software makes assumptions about the things it talks to. Most of those assumptions are never written down anywhere. They're just... implicit. Understood. Obviously true. And occasionally completely wrong.

The ssh-agent RCE is a good example of this going badly. When you use agent forwarding, the remote server can ask your local agent to load PKCS#11 providers - shared libraries. Your agent just does it. Why wouldn't it? The server is trusted, right? Except "trusted to run your SSH session" is not the same as "trusted to load arbitrary code into your agent process." The agent conflated those two things, and someone with access to the remote server could use that to execute code on your local machine.

The sudo Baron Samedit bug is different but the same idea. sudo trusted itself. The escape logic and the buffer size calculation were in different code paths, and nobody had explicitly verified that they agreed with each other. The implicit invariant was "after escaping, the buffer is still big enough" - and that turned out not to be true.

---

When you're looking at a piece of code, it helps to think about trust explicitly. Draw it out if you need to. Every component trusts something - trusts that inputs are validated, trusts that callers are authorized, trusts that other components behave correctly. The question is: are those trust relationships actually enforced, or are they just assumed?

The questions worth asking:
- What does this component assume about who calls it?
- What does it assume about what it's talking to?
- What happens if that assumption is wrong?
- Can an attacker be in a position to violate that assumption?

```bash
# places where code explicitly signals trust
grep -r "trusted\|// safe\|// always\|// guaranteed" src/

# cross-component interfaces where assumptions get made
grep -r "socket\|pipe\|IPC\|shared_mem" src/

# privileged operations - what does the code assume is true before reaching them?
grep -r "setuid\|CAP_\|sudo\|priv_" src/
```

One thing worth remembering: trust chains compound. If A trusts B and B trusts C, then compromising C gives you A. The longest chains are usually the least scrutinized, because the developer at A was thinking about A's security, not about what happens three hops away.
