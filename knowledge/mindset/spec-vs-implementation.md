---
category: mindset
title: "The Spec is the Contract, the Code is the Lie"
tags: [spec-vs-implementation, heartbleed, rfc-analysis, zero-day-methodology, protocol-bugs, openssl]
---

# The Spec is the Contract, the Code is the Lie

RFC 6520 says you must validate the heartbeat payload length before sending a response. This is not subtle. It's not buried in a footnote. OpenSSL just... didn't do it. For two years. On servers that handled a significant fraction of encrypted traffic on the internet.

Heartbleed was found two ways: Codenomicon was testing OpenSSL *against the RFC* while improving their fuzzing tool. Neel Mehta at Google found it through code review. Both were essentially asking the same question: does the code actually do what the specification says it should?

---

This is underrated as an approach. Developers write code based on their understanding of a spec, and their understanding is often incomplete or slightly wrong. The longer a protocol has been around, the more edge cases exist in the spec that nobody ever implemented correctly. The more complex the spec, the more places where "I'll handle this case properly later" became permanent.

The gap between spec and implementation is the attack surface. It's not about the code being obviously wrong - it's about the code being wrong *relative to what it's supposed to do*.

---

Pick your protocol. Find the RFC. Look for MUST requirements:

```
RFC 6520:  "length MUST be validated"
RFC 7519:  "implementations MUST validate the alg header"  
OAuth 6749: "server MUST validate the redirect_uri"
```

Now find where that requirement is (or isn't) implemented. `grep` is your friend. If you can't find the check, you've probably found something worth looking at.

The RFC approach works especially well on:
- Authentication protocols (lots of MUST requirements, often partially implemented)
- Anything with length-prefixed fields (classic source of over-reads)
- State machines (the spec defines valid transitions; implementations often accept invalid ones)
- Anything involving cryptographic verification (there are a lot of ways to do this wrong)

For web APIs without an RFC: the documentation is the spec. Test what happens with fields not mentioned in the docs. Wrong types. Missing required fields. Extra unexpected fields. The docs describe the happy path; the interesting behavior is usually in the gaps.
