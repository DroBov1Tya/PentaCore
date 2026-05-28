---
category: methodology
title: "Tool-First Principle"
tags: [tools, methodology, automation, tool-first]
---

# Tool-First Principle

Before writing a custom curl loop or a Python script - check if a tool already does it. They handle edge cases, retries, encoding, and output parsing. Years of real-world testing went into them.

The pattern:

```bash
# check
command -v ffuf

# install if missing
if ! command -v ffuf &>/dev/null; then
    command -v apt-get && sudo apt-get install -y ffuf \
    || command -v brew && brew install ffuf \
    || go install github.com/ffuf/ffuf/v2@latest
fi

# use it
ffuf -w wordlist.txt -u https://target/FUZZ

# only if all of the above fails: write something custom
```

Write custom code when the tool doesn't support the protocol, when you're building a PoC that needs a specific exploit chain, or when the target has WAF evasion requirements the tool can't handle.

## Quick reference

| Task | Tool |
|------|------|
| Port scan | `rustscan` → `nmap` |
| Mass scan | `masscan` |
| Subdomain enum | `subfinder` + `amass` + `alterx` → `dnsx` → `httpx` |
| URL mining | `gau` |
| Web crawling | `katana` |
| Tech detection | `whatweb` |
| WAF detection | `wafw00f` |
| Full recon | `bbot` |
| Exposed .git | `git-dumper` |
| Exposed registry | `crane` |
| Secrets in git | `trufflehog` / `gitleaks` |
| Directory fuzzing | `feroxbuster` |
| Request fuzzing | `ffuf` |
| Parameter discovery | `arjun` |
| API routes | `kiterunner` |
| Vuln scanning | `nuclei` |
| SQL injection | `sqlmap` |
| XSS | `dalfox` |
| C2 | `sliver` |
| Pivoting | `ligolo-ng` or `chisel` |
| Proxy chains | `proxychains` |
| Network pentest | `netexec` (nxc) |
| AD attacks | `impacket` |
| Exploitation | `msfconsole` |
| Password cracking | `hashcat` |
| Binary exploitation | `GEF` + `pwntools` |
