---
category: technique
title: "Web Fuzzing - feroxbuster, ffuf, arjun, kiterunner"
tags: [tools, fuzzing, ffuf, feroxbuster, arjun, kiterunner, wordlists]
---

# Web Fuzzing

## Directory and file discovery

feroxbuster recurses automatically - finds `/api/`, then fuzzes inside `/api/`, then inside whatever it finds there. Good default for most targets.

```bash
which feroxbuster || cargo install feroxbuster

feroxbuster -u https://example.com -w /usr/share/wordlists/dirb/common.txt
feroxbuster -u https://example.com -w wordlist.txt -x php,html,js,txt,bak,old
feroxbuster -u https://example.com -w wordlist.txt -H "Authorization: Bearer <token>"
feroxbuster -u https://example.com -w wordlist.txt -d 3 -o results.txt
```

ffuf is more flexible - use it when you need to fuzz something other than a path, or need precise filtering:

```bash
which ffuf || go install github.com/ffuf/ffuf/v2@latest

ffuf -w wordlist.txt -u https://example.com/FUZZ
ffuf -w wordlist.txt -u https://example.com/FUZZ -e .php,.html,.js,.bak
ffuf -w wordlist.txt -u https://example.com/login -d "user=FUZZ&pass=test" -H "Content-Type: application/x-www-form-urlencoded"
ffuf -w subdomains.txt -u https://example.com -H "Host: FUZZ.example.com" -fs 0
ffuf -w wordlist.txt -u https://example.com/FUZZ -fs 1234    # filter noise by size
ffuf -w wordlist.txt -u https://example.com/FUZZ -mc 200,301,302,403
```

## Parameter discovery

arjun finds hidden GET/POST parameters - useful before you start testing input handling:

```bash
pip install arjun

arjun -u https://example.com/api/search
arjun -u https://example.com/api/search -m POST
arjun -u https://example.com/api -H "Authorization: Bearer token"
```

Or with ffuf directly:

```bash
ffuf -w params.txt -u "https://example.com/api/search?FUZZ=test" -fs 0
ffuf -w values.txt -u "https://example.com/api/search?q=FUZZ"
```

## API route discovery

kiterunner uses API-specific wordlists and understands REST patterns - much better than generic directory fuzzing for APIs:

```bash
go install github.com/assetnote/kiterunner/cmd/kr@latest

kr scan https://example.com/api -w routes-small.kite
kr scan https://example.com/api -w routes-large.kite -H "Authorization: Bearer token"
```

## Wordlists

```
/usr/share/wordlists/dirb/common.txt           - small, fast first pass
/usr/share/seclists/Discovery/Web-Content/raft-large-words.txt  - thorough
/usr/share/seclists/Discovery/Web-Content/api/objects.txt       - APIs
/usr/share/seclists/Discovery/Web-Content/burp-parameter-names.txt - params
```

```bash
sudo apt-get install seclists
# or: git clone https://github.com/danielmiessler/SecLists /usr/share/seclists
```
