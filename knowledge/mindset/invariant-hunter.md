---
category: mindset
title: "Find What the System Never Checks"
tags: [invariant-hunting, zero-day-methodology, implicit-assumptions, missing-checks, state-machine, privilege-escalation]
---

# Find What the System Never Checks

Every non-trivial program has things it assumes are true but never actually verifies. These are invariants. Some are documented. Most aren't. The ones that aren't documented are the interesting ones.

A few invariants that turned out to be false: "the user_args buffer is large enough after escaping" (sudo). "The client won't ask for more heartbeat data than it sent" (OpenSSL). "SIGALRM handlers only call async-signal-safe functions" (OpenSSH). "Log messages don't get evaluated" (Log4j). Each of these was an assumption that seemed so obviously true that nobody thought to check it.

---

The way to find these is to read code and ask "what is this code *relying on* that it doesn't verify?" It's a different mode than looking for bugs directly. You're looking for assumptions.

Some places to look:

**Assertions that get disabled in production:**
```bash
grep -r "assert(" src/
```
Every assert() is someone saying "I'm assuming this is true." In debug builds it gets checked. In release builds it doesn't. If an assertion can be violated by external input, that's worth investigating.

**Comments that declare safety:**
```bash
grep -r "// safe\|// always\|// never\|// guaranteed\|// trusted" src/
```
When someone writes "// safe because X", ask yourself whether X is actually enforced or just assumed.

**Missing null checks, missing bounds checks:**
The annoying thing about these is they're everywhere and most of them are fine. The interesting ones are where the thing being passed in is partially attacker-controlled. `memcpy(dst, src, user_len)` with no check on user_len is a classic. `obj->field` where obj came from an external source and nobody checked for null.

**State machine assumptions:**
Most interesting protocols have states. A lot of implementations are written assuming that requests arrive in the expected order. They often don't validate that. Can you send a message that's only valid in state B while the server thinks it's in state A? What happens?

---

The exploit pattern once you find a violated invariant is usually: find the privileged function → trace all the paths that reach it → find a path that doesn't go through the required preconditions. The invariant violation is often exactly that missing precondition.
