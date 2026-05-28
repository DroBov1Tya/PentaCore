---
category: technique
title: "Auth Logic Bugs - Authentication and Authorization Bypass"
tags: [auth, authentication, authorization, bypass, rbac, privilege-escalation, access-control, jwt, session]
---

# Auth Logic Bugs

Authentication and authorization bugs are the highest-signal target in any web application. They appear where developers assumed something was impossible rather than explicitly checking that it isn't.

---

## The fundamental split

Authentication: proving you are who you say you are.
Authorization: proving you are allowed to do what you're trying to do.

Most auth bugs live at the boundary between these two - a check that proves identity but not permission, or a check that runs in the wrong order, or a check that's present in the frontend but not in the API.

---

## Authentication bypass patterns

**State-machine confusion.** Multi-step login flows (MFA, email verification, password reset) are state machines. Most developers validate the final step but forget to validate that you actually completed all prior steps. Try calling step 3 directly without going through step 1 and 2.

```http
# Normal flow: POST /login -> POST /mfa-verify -> GET /dashboard
# Attack: skip directly to
GET /dashboard   Cookie: session=<token-from-step-1>
```

**Password reset as auth bypass.** Reset flow typically has: request-reset -> email-with-token -> set-new-password. Check: does the reset token get invalidated after use? Can you reuse old tokens? Does requesting a reset for your account invalidate active sessions for that account?

**Race condition on session creation.** Some implementations create the session before validating MFA. Two requests at the right moment can give you an authenticated session without completing MFA.

**Host header injection in password reset.** If the app uses `Host:` header to construct the reset URL, poisoning it sends the reset link to your server.

```http
POST /forgot-password HTTP/1.1
Host: attacker.com
```

---

## Authorization bypass patterns

**IDOR (Insecure Direct Object Reference).** Replace an ID in a request with another user's ID. Most common in REST APIs and mobile backends. Check: numeric IDs (sequential - trivial), UUIDs (enumerate via other endpoints), compound keys.

```bash
# User sees their own object at ID 1337
GET /api/orders/1337

# Try another user's ID
GET /api/orders/1338
```

**Method confusion.** `DELETE /resource/123` is properly protected. `PUT /resource/123` with a `_method=DELETE` override isn't. Some frameworks support HTTP method overriding via `X-HTTP-Method-Override` or `_method` parameter.

**Path traversal in access control.** Authorization check is `if user.owns(requested_path)` but actual file access is at `root + requested_path`. A normalized check against an un-normalized path breaks the equivalence.

**GraphQL batching / introspection escape.** If a query is blocked, try nesting the same operation inside an allowed one, or use aliases to run it multiple times.

```graphql
{
  me {
    ... on AdminUser {
      allUsers { id email }
    }
  }
}
```

**Privilege escalation via role parameter.** API that sets user attributes at registration sometimes allows passing a `role` field that gets stored without validation.

```json
POST /register
{"username": "attacker", "email": "...", "role": "admin"}
```

**BOLA/BFLA.** Broken Object Level Authorization - can you access ANOTHER object? Broken Function Level Authorization - can you call admin functions with a regular user token? These require mapping all functions, not just all objects.

---

## JWT attacks

JWTs are signed tokens. The signature MUST be verified on every request. Classic bugs:

**Algorithm confusion (`alg: none`).** Some libraries accept a JWT signed with no algorithm. Remove the signature, change `alg` to `none`.

```
eyJhbGciOiJub25lIn0.eyJ1c2VyIjoiYWRtaW4ifQ.
```

**RS256 -> HS256 confusion.** If server uses RS256, the public key is often available (`/jwks.json`, `/.well-known/openid-configuration`). Some libraries will accept HS256 with the public key as the HMAC secret.

```python
import jwt
public_key = open("public_key.pem").read()
token = jwt.encode({"user": "admin", "role": "admin"}, public_key, algorithm="HS256")
```

**JKU / X5U header injection.** If the JWT has a `jku` or `x5u` header pointing to a JWKS endpoint, replace it with your own endpoint hosting a key you control.

**Kid header path traversal.** `kid` (key ID) is sometimes used as a filename to load the verification key. Try `kid: ../../../../dev/null` (symmetric) or `kid: ../../../../etc/passwd`.

**Short secret brute force.** HS256 signatures can be brute-forced offline if the secret is weak.

```bash
hashcat -a 0 -m 16500 <token> /usr/share/wordlists/rockyou.txt
```

---

## RBAC bypass checklist

1. Map all roles mentioned anywhere (source code, UI, API responses, error messages)
2. For each privileged action, try it with a lower-privilege role's token
3. Check parameter-based role escalation (`role=admin` in request body or query)
4. Check if admin endpoints have different authentication middleware or just rely on frontend hiding
5. Check horizontal privilege escalation (same role, different tenant/org/user)
6. Look for role checks that happen only at read time but not at write time (cached role)
7. Check if role is embedded in JWT and not re-validated server-side against the database

---

## Finding auth checks in code

```bash
# endpoints with NO auth decorator (missing check pattern)
grep -rn "def \|async fn \|router\." src/ | grep -v "@login_required\|@requires_auth\|auth_guard\|verify_token"

# look for role checks - are they consistent?
grep -rn "role\|permission\|admin\|is_staff" src/ | grep -v "test_\|#"

# JWT decode without verify
grep -rn "decode\|verify" src/ | grep -i "jwt\|token"
```
