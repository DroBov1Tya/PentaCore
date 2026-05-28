---
category: technique
title: "Recon Tools - Port Scanning, Subdomain Enumeration, URL Mining, Tech Detection, Secrets"
tags: [tools, recon, nmap, rustscan, subfinder, bbot, httpx, amass, gau, katana, trufflehog, wafw00f, whatweb, masscan, dnsx, alterx, shodan]
---

# Recon Tools

## Port scanning

rustscan finds open ports fast, then hands off to nmap for service detection:

```bash
which rustscan || cargo install rustscan

rustscan -a 10.0.0.1 -- -sV -sC
rustscan -a 10.0.0.0/24 -- -sV --script=default
```

masscan for large ranges:

```bash
sudo masscan -p0-65535 10.0.0.0/16 --rate=100000 -oG masscan.txt
```

nmap for thorough scanning:

```bash
nmap -sV -sC -oN scan.txt target.com
nmap -p- -T4 target.com
nmap -sU --top-ports 100 target.com
nmap --script=vuln -p 80,443 target.com
```

## Subdomain enumeration

```bash
which subfinder || go install -v github.com/projectdiscovery/subfinder/v2/cmd/subfinder@latest
subfinder -d example.com -all -o subdomains.txt

which amass || go install -v github.com/owasp-amass/amass/v4/...@master
amass enum -passive -d example.com -o subdomains.txt
amass enum -active -d example.com -brute -o subdomains.txt

which dnsx || go install -v github.com/projectdiscovery/dnsx/cmd/dnsx@latest
cat subdomains.txt | dnsx -resp -o resolved.txt
dnsx -d example.com -w subdomains-wordlist.txt -resp

which alterx || go install github.com/projectdiscovery/alterx/cmd/alterx@latest
subfinder -d example.com -silent | alterx | dnsx -silent -resp

which httpx || go install -v github.com/projectdiscovery/httpx/cmd/httpx@latest
cat subdomains.txt | httpx -title -status-code -tech-detect -o alive.txt
```

## URL mining and crawling

```bash
which gau || go install github.com/lc/gau/v2/cmd/gau@latest
gau example.com --threads 10 --o urls.txt
gau example.com | grep -E "\.js$"
gau example.com | grep -E "\?(.*=)"

which katana || go install github.com/projectdiscovery/katana/cmd/katana@latest
katana -u https://example.com -o crawled.txt
katana -u https://example.com -js-crawl -d 3
katana -u https://example.com -ef woff,css,png,svg
```

## Tech detection

```bash
whatweb https://example.com
whatweb -a 3 https://example.com
whatweb -i subdomains.txt --log-json=tech.json

wafw00f https://example.com      # check for WAF before fuzzing
wafw00f -a https://example.com
```

## Full automated recon

```bash
pip install bbot
bbot -t example.com -p subdomain-enum
bbot -t example.com -p aggressive
bbot -t example.com -m nmap,nuclei,httpx,subfinder -o ~/recon/
```

## Secrets in git

```bash
which trufflehog || pip install trufflehog
trufflehog git https://github.com/org/repo
trufflehog git file://./local-repo --only-verified
trufflehog github --org=orgname --only-verified

which gitleaks || go install github.com/gitleaks/gitleaks/v8@latest
gitleaks detect --source . --report-path findings.json
```

## External exposure

```bash
pip install shodan && shodan init <api-key>
shodan search 'org:"Example Corp"'
shodan search 'ssl.cert.subject.CN:example.com'
shodan host 1.2.3.4

pip install cloud-enum
cloud_enum -k example -m aws,gcp,azure

curl -s "https://crt.sh/?q=%.example.com&output=json" | jq -r '.[].name_value' | sort -u

which tlsx || go install github.com/projectdiscovery/tlsx/cmd/tlsx@latest
tlsx -san -cn -silent -l subdomains.txt
```
