---
category: technique
title: "API Security - REST and GraphQL Specific Attacks"
tags: [api, rest, graphql, mass-assignment, bola, bfla, excessive-data, rate-limit, api-versioning, method-tampering, content-type, webhook, openapi]
---

# API Security

APIs fail differently from server-rendered apps. There is no UI to constrain you - every field, method, and version is directly reachable. The OWASP API Top 10 is dominated by authorization (BOLA/BFLA) and by the gap between what the API accepts and what the developer intended it to accept.

Pair this with auth-logic-bugs (IDOR/BOLA/JWT details) and business-logic (workflow abuse). This file covers the API-shape-specific issues.

---

## Map the real surface first

The documented API is never the whole API. Find the rest:

```bash
# Spec files - import everything automatically
curl -s https://api.target.com/swagger.json
curl -s https://api.target.com/openapi.json
curl -s https://api.target.com/v2/api-docs
curl -s https://api.target.com/graphql -d '{"query":"{__schema{types{name}}}"}'
# Built-in: parse_api_spec / parse_graphql_spec import all routes into the DB

# Old versions often skip newer auth checks - test every version
/api/v1/users/123    # is v1 still up after v3 launched? often less protected
/api/v2/users/123
/api/internal/...    # internal-prefixed routes sometimes lack external auth

# JS bundle is the client's API map - extract it
curl -s https://target.com/main.js | grep -oE '"/api/[a-zA-Z0-9/_-]+"' | sort -u
```

---

## Mass assignment / autobinding

The highest-value API-shape bug. The API binds the JSON body directly to a model. If it does not whitelist fields, you set fields the UI never exposes.

```http
# Normal registration the UI sends:
POST /api/users  {"username":"bob","email":"bob@x.com","password":"..."}

# What the model also has - try adding these:
POST /api/users  {"username":"bob","email":"bob@x.com","password":"...",
                  "role":"admin","is_admin":true,"email_verified":true,
                  "account_balance":99999,"organization_id":1,"plan":"enterprise"}
```

Where to find candidate fields: the GET response of the same object usually lists every field the model has. Read object first, then echo every field back on update/create.

```bash
# Discover the field set from a read, then replay it into a write
GET /api/users/me          -> note all fields returned
PUT /api/users/me          -> send them all back, including ones the form omitted
```

Framework-specific grep (whitebox): Rails `permit`/`attr_accessible`, Django serializer `fields = '__all__'`, Spring `@ModelAttribute` without `@InitBinder`, Node mongoose `Model(req.body)` directly.

---

## BOLA / BFLA (object and function level authz)

The number one API risk. Covered in depth in auth-logic-bugs - the API angle:

- **BOLA**: every endpoint with an ID in the path or body. Swap the ID for another tenant's. UUIDs are not protection if you can harvest them from list endpoints, error messages, or other objects.
- **BFLA**: admin/privileged functions reachable with a normal token. Map every function from the spec, then call the admin ones with a low-priv token. The function exists in the same API - only a check stands between you and it.

```bash
# Systematic BOLA sweep: replay_as (built-in) replays a saved request as another user
replay_as(request_id: 42, auth_token: "low_priv_user_token")
# then diff_requests to confirm cross-tenant data returned
```

---

## Excessive data exposure

The API returns the full object and trusts the client to display only some fields. The mobile/web client hides them; the raw response does not.

```bash
# The list endpoint returns user objects - check for fields the UI never shows
GET /api/users  ->  does each item include password_hash, ssn, internal_notes,
                    api_key, mfa_secret, is_admin, salary?
```

Always read the raw JSON, never trust what the rendered page shows. The filtering is often client-side only.

---

## HTTP method and content-type tampering

- **Method override.** `PUT`/`DELETE` blocked by a WAF or middleware? Try `PATCH`, or `POST` with `X-HTTP-Method-Override: DELETE` / `_method=DELETE`. (Confirmed real-world: WAF rules target a method, not the endpoint+method pair.)
- **Content-type confusion.** Endpoint validates JSON strictly? Send the same data as `application/xml` (XXE surface) or `application/x-www-form-urlencoded` (different parser, may skip validation or enable CSRF on a JSON API).
- **Verb-based authz gaps.** `GET /api/orders/1` is protected; is `HEAD` or `OPTIONS`? Does `GET` leak via a cache the `POST` does not?

---

## Rate limit and quota bypass

Rate limits gate brute force, enumeration, and business-logic abuse. Common bypasses:

```http
# IP-based limit - rotate the trusted header
X-Forwarded-For: 1.2.3.4
X-Real-IP: 1.2.3.4
X-Originating-IP: 1.2.3.4

# Account-based limit - it counts per user_id but reads it from the request
POST /api/action?user_id=OTHER   # charge the count to someone else

# Case / path variation slips past a path-keyed limiter
/api/Login  vs  /api/login  vs  /api/login/

# GraphQL batching - N operations in 1 request bypasses per-request limits
[{"query":"mutation{login(u:\"a\",p:\"p1\")}"}, {"query":"mutation{login(u:\"a\",p:\"p2\")}"}, ...]
```

A bypassable rate limit is rarely the finding by itself - it is the enabler for credential stuffing, OTP brute force, or coupon abuse (see business-logic).

---

## GraphQL specifics

Covered in web-advanced; the API-testing checklist:

- Introspection on in prod -> full schema. Blocked? Field-suggestion errors leak names anyway.
- Alias-based batching -> bypass rate limits and amplify (run the same mutation 100x in one request).
- Nested query depth -> DoS if no depth/complexity limit.
- Mutations are the dangerous half - enumerate every mutation, check authz on each, look for mass-assignment in input types.
- Global IDs (base64 `Type:id`) -> decode, re-encode with a different type or id, test cross-object access.

---

## Webhooks and callbacks

Any endpoint that accepts a URL and fetches it is SSRF (see ssrf-techniques). API-specific angle:

- Webhook registration with an internal URL -> SSRF to metadata/internal services.
- Webhook signature: is it verified? Can you forge inbound webhook events (fake a "payment succeeded" callback)?
- Replay: are webhook events idempotent, or does replaying a "credit added" event credit twice?

---

## API keys and tokens

- Keys in URLs -> logged in proxies, referrer headers, browser history.
- Key scoping: does a read-only key actually reject writes, or is the scope advisory?
- Missing expiry / no rotation -> a leaked key from a 2-year-old commit may still work. Check git history.
- JWT specifics -> oauth-jwt-attacks.

---

## The API testing mindset

The API is the model and the developer's assumptions about its client, exposed directly. For every endpoint ask:
1. What fields does the underlying object have that I am not being shown? (excessive exposure, mass assignment)
2. Whose object can I reach by changing the ID? (BOLA)
3. What functions exist that my role should not call? (BFLA)
4. What does the server assume the client already validated? (the client is gone - you send raw)
5. What older version or undocumented route skips the checks the main one has?
