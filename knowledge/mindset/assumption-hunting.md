---
category: mindset
title: "Assumption Hunting - Finding What the Code Believes Is Always True"
tags: [assumptions, mental-model, vulnerability-hunting, trusted-input, enforcement-vs-expectation, zero-day-methodology]
---

# Assumption Hunting

Every piece of code makes assumptions. Most of them are implicit. Security bugs live where an assumption is EXPECTED to be true but not ENFORCED.

The technique: read code not to understand what it does, but to inventory what it believes.

---

## The three categories of assumptions

**Input assumptions** - what this component believes about the data it receives:
- "This value was validated by the caller"
- "This ID belongs to the authenticated user"
- "This file path is within the allowed directory"
- "This token was issued by our auth server"

**Context assumptions** - what this component believes about the state of the world:
- "The previous step in this workflow was completed"
- "The file at this path is the same one that was uploaded"
- "This user's role hasn't changed since the token was issued"
- "The signature covers the element I'm about to read"

**Composition assumptions** - what this component believes the next one will do:
- "The next service will re-validate the token audience"
- "The cache will serve fresh data to privileged users"
- "The worker will re-check ownership before acting on the job"

---

## How to find assumptions in code

**Look for what's missing, not what's present.**

```bash
# Functions that receive IDs but don't join against ownership
grep -rn "findById\|get_by_id\|SELECT.*WHERE id" src/ | \
  # For each: is there a second condition tying this to the authenticated user?

# Async handlers that read from a queue/message without re-validating
grep -rn "consume\|dequeue\|@RabbitListener\|@app.task" src/ | \
  # For each: does it validate the data, or does it trust the message?

# JWT decode without checking all claims
grep -rn "decode\|verify" src/ | grep -i "jwt\|token" | \
  # For each: are aud, iss, exp, jti all explicitly checked?

# File paths used without normalization check
grep -rn "open(\|File(\|readFile\|read_to_string" src/ | \
  # For each: is there a canonicalize() + prefix check before open()?
```

---

## The enforcement gap

For every assumption, ask two questions:
1. Is this assumption stated somewhere? (comment, variable name, doc)
2. Is it checked in code before being acted on?

A common pattern:

```python
# Comment says "user_id comes from JWT, already validated"
def get_order(order_id, user_id):
    return db.query("SELECT * FROM orders WHERE id = ?", order_id)
    # The comment says user_id is trusted. But it's never used in the query.
    # IDOR.
```

The developer STATED the assumption (in the comment). They just didn't ENFORCE it.

---

## Questions to ask about any component

1. What would have to be true about the input for this code to behave safely?
2. Who is responsible for ensuring that's true?
3. What happens if it's not true?
4. Does THIS code check it, or does it rely on something else to have checked it already?
5. Is that "something else" always executed before this? Can it be skipped?
6. Are there multiple code paths into this component? Do all of them go through the check?

---

## Where copy-paste kills you

When a security check exists in one place, developers often assume it applies everywhere. It doesn't.

The pattern:
- Service A validates JWT audience claim correctly
- Services B, C, D are copy-pasted from A with "minor modifications"
- In one of them, the audience check was "simplified" or removed as "not needed here"

**How to find this:**
```bash
# Find all JWT validation middleware
grep -rn "verify\|validate.*token\|check.*jwt" services/*/src/

# Diff them against each other - look for what's in A but not in B
diff <(grep -A20 "function validateToken" services/auth/src/middleware.js) \
     <(grep -A20 "function validateToken" services/transfer/src/middleware.js)
```

The missing line in the diff is often the vulnerability.

---

## The "already validated" trap

The most common assumption in distributed systems: "this was validated upstream."

Signs of this assumption in code:
- Comments like: // user already authenticated at this point
- Variable names like trusted_user_id, validated_input, safe_path
- Functions that only run in authenticated contexts but accept parameters without re-checking

The question to ask: what happens if an attacker bypasses the upstream component entirely and calls this function directly? What if the upstream sends unexpected data?

In microservices: if service B accepts a request from service A with elevated privileges, does B verify that the request actually came from A? Or does it just trust the parameter values?

---

## Tracking assumptions as hypotheses

When you identify an assumption, save it immediately:

```
save_hypothesis(
  hypothesis: "transfer-service assumes the aud claim was validated by auth-service 
               and does not re-check it independently",
  source: "auth middleware diff - aud check present in auth-service, absent in transfer-service"
)
```

This forces precision. "JWT might be vulnerable" is not a hypothesis. "transfer-service does not validate aud claim and will accept a retail token on internal ops endpoints" is.
