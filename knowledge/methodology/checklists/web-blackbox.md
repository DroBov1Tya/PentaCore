---
category: methodology
title: "Web Testing - Blackbox"
tags: [checklist, web, blackbox, no-source, sequential, pentest, enumerate, crawl, timing-baseline, idor, input-validation, business-logic]
---

# Web Testing - Blackbox

No source code. Enumerate, measure, probe.

## Map the surface

- Crawl all endpoints: browser, JS files, robots.txt, sitemap.xml
- Identify technologies: response headers, cookies, error pages
- Find all input forms and parameters
-> `save_scope()` for every discovered domain/service

## Establish baseline

```bash
for endpoint in /api/user /api/admin /api/search /api/export; do
    printf "%s: " "$endpoint"
    for i in 1 2 3 4 5; do
        curl -s -o /dev/null -w "%{time_total} " "https://target$endpoint"
    done; echo
done
```

Record response times and sizes. Any deviation later is a hypothesis.
-> `save_hypothesis()` for any anomaly

## Authentication

- Timing difference: valid username vs nonexistent? -> user enumeration oracle
- Response size difference: wrong user vs wrong password?
- Decode any JWT. Check: alg:none accepted? Expired token accepted?
- If OAuth: is `state` parameter validated? Is `redirect_uri` strictly checked?
-> `save_hypothesis()` for each suspicious pattern

## Authorization

Build the role/action/resource matrix. For each "forbidden" cell - try it as a lower role.

- IDOR: `/api/resource/123` -> try `/api/resource/124`
- Try admin endpoints directly without admin role
- Check indirect IDOR through related objects
-> `save_hypothesis()` for each test

## Input validation

For every parameter: send too much data, wrong type, null, special characters.

- Search/filter fields: SQL metacharacters, brackets, quotes
- File uploads: extension bypass, content-type bypass
- Redirect parameters: open redirect?
-> `save_hypothesis()` for interesting reactions

## Business logic

- Workflow: can you skip a step? Call the final action directly?
- Race condition: send 50 identical requests simultaneously
- Negative numbers, zero amounts, duplicates where not expected
-> `save_hypothesis()`

## Infrastructure

- Security headers: CSP, HSTS, X-Frame-Options
- Open directories, backup files (.bak, .old, ~)
- Subdomains: DNS enumeration
-> `save_finding()` for confirmed issues

**Done when:** all 7 steps completed, every hypothesis has an outcome.
