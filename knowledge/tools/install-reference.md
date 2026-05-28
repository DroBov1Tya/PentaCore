---
category: methodology
title: "Tool Install Reference - Quick Installation Commands by OS"
tags: [tools, install, setup, kali, ubuntu, macos]
---

# Tool Install Reference

When a tool is missing, detect the OS and install before proceeding.

## Detect OS

```bash
uname -s                                          # Linux / Darwin
cat /etc/os-release 2>/dev/null | grep ^ID=       # ubuntu, kali, debian...
sw_vers 2>/dev/null                               # macOS version
```

## Kali Linux (most tools pre-installed)

```bash
sudo apt-get update
sudo apt-get install -y nmap masscan rustscan nuclei sqlmap feroxbuster ffuf dalfox
sudo apt-get install -y subfinder httpx amass dnsx bbot
sudo apt-get install -y netexec impacket-scripts metasploit-framework
sudo apt-get install -y hashcat hydra
```

## Ubuntu / Debian

```bash
sudo apt-get update && sudo apt-get install -y nmap masscan sqlmap metasploit-framework hashcat hydra

# Go-based tools
sudo apt-get install -y golang-go
go install github.com/projectdiscovery/nuclei/v3/cmd/nuclei@latest
go install github.com/projectdiscovery/subfinder/v2/cmd/subfinder@latest
go install github.com/projectdiscovery/httpx/cmd/httpx@latest
go install github.com/projectdiscovery/dnsx/cmd/dnsx@latest
go install github.com/projectdiscovery/alterx/cmd/alterx@latest
go install github.com/projectdiscovery/katana/cmd/katana@latest
go install github.com/projectdiscovery/interactsh/cmd/interactsh-client@latest
go install github.com/ffuf/ffuf/v2@latest
go install github.com/hahwul/dalfox/v2@latest
go install github.com/ropnop/kerbrute@latest
go install github.com/lc/gau/v2/cmd/gau@latest
go install github.com/BishopFox/cloudfox@latest
go install github.com/nicocha30/ligolo-ng/cmd/proxy@latest
go install github.com/nicocha30/ligolo-ng/cmd/agent@latest
go install github.com/jpillora/chisel@latest

# Rust-based
cargo install feroxbuster rustscan

# Python-based
pip install impacket arjun netexec bbot xsstrike certipy-ad prowler scoutsuite mitm6
pip install bloodhound trufflehog shodan
```

## macOS

```bash
brew install nmap masscan sqlmap hashcat hydra go
go install github.com/projectdiscovery/nuclei/v3/cmd/nuclei@latest
go install github.com/projectdiscovery/subfinder/v2/cmd/subfinder@latest
go install github.com/projectdiscovery/httpx/cmd/httpx@latest
go install github.com/projectdiscovery/dnsx/cmd/dnsx@latest
go install github.com/projectdiscovery/katana/cmd/katana@latest
go install github.com/ffuf/ffuf/v2@latest
go install github.com/hahwul/dalfox/v2@latest
go install github.com/lc/gau/v2/cmd/gau@latest
go install github.com/nicocha30/ligolo-ng/cmd/proxy@latest
go install github.com/jpillora/chisel@latest
cargo install feroxbuster rustscan
pip3 install impacket sqlmap arjun netexec bbot certipy-ad trufflehog
```

## Check what's installed

```bash
for tool in nmap masscan rustscan subfinder httpx dnsx nuclei ffuf feroxbuster \
            sqlmap dalfox gau katana netexec kerbrute ligolo-proxy chisel \
            trufflehog gitleaks hashcat hydra; do
    command -v $tool &>/dev/null && echo "✓ $tool" || echo "✗ $tool"
done
```

## PATH - if Go/Cargo tools aren't found

```bash
export PATH=$PATH:$(go env GOPATH)/bin
export PATH=$PATH:$HOME/.cargo/bin
# Add to ~/.bashrc or ~/.zshrc to persist
```

## Wordlists

```bash
# SecLists - most comprehensive, covers everything
sudo apt-get install seclists       # Kali/Ubuntu → /usr/share/seclists/
git clone https://github.com/danielmiessler/SecLists /usr/share/seclists

# Key locations:
# /usr/share/seclists/Discovery/Web-Content/raft-large-words.txt  - directories
# /usr/share/seclists/Discovery/Web-Content/api/objects.txt       - API endpoints
# /usr/share/wordlists/rockyou.txt                                 - passwords
```
