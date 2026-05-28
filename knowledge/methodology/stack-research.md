---
category: methodology
title: "Stack Fingerprint → CVE → Exploit"
tags: [methodology, cve, stack, fingerprinting, exploit-search, nuclei, searchsploit]
---

# Stack Fingerprint → CVE → Exploit

Once you know what's running - look it up before spending time on manual testing. A known CVE in the exact version takes an hour to exploit; manually finding the same bug takes days.

## Get the versions

```bash
whatweb https://example.com -a 3
curl -si https://example.com | grep -i "server\|x-powered-by\|x-generator"
nuclei -u https://example.com -tags tech -silent
```

From source code: check `package.json`, `requirements.txt`, `Gemfile.lock`, `go.mod`, `pom.xml`, `composer.json` - exact pinned versions, not ranges.

## Search for CVEs

```bash
# searchsploit - local ExploitDB copy
searchsploit django 4.2
searchsploit spring boot 3.1
searchsploit wordpress 6.4

# GitHub for fresh PoCs (searchsploit lags weeks behind)
# search: "django 4.2 CVE" or "framework-name version exploit poc"

# nuclei with version-specific templates
nuclei -u https://example.com -tags cve -severity critical,high

# for known frameworks
nuclei -u https://example.com -tags django
nuclei -u https://example.com -tags rails
nuclei -u https://example.com -tags spring
```

## Framework-specific checks

Django: check `ALLOWED_HOSTS`, `DEBUG=True`, admin at `/admin`, ORM querysets with `.raw()` or `.extra()`, CSRF exempt views.

Rails: mass assignment, `find_by` vs `where` with user input, cookie deserialization on older versions.

Spring Boot: actuator endpoints (`/actuator/env`, `/actuator/heapdump`), SpEL injection in older versions.

WordPress: `wpscan --url https://example.com --enumerate vp,vt,u`.

Node/Express: prototype pollution, SSRF through `axios`/`got` with user-controlled URLs, `eval` in template engines.

## The flow

fingerprint versions → searchsploit + GitHub → nuclei with stack tags → if CVE exists: find PoC and test → if no CVE: go back to manual testing with stack-specific grep patterns

Knowing the stack also changes what you grep for. Django ORM bugs look different from raw SQL bugs. A Rails mass assignment issue requires different grep patterns than a PHP one. Look it up first.
