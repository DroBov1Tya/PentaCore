---
category: methodology
title: "Docker Image Analysis"
tags: [checklist, docker, container, image, trivy, escape, kubernetes, sequential]
---

# Docker Image Analysis

Two angles: static (image contents) and dynamic (runtime behavior).

## Image inspection

```bash
docker inspect <image>      # env vars, exposed ports, user, entrypoint
docker history <image>      # layers and the commands that built them
dive <image>                # interactive layer explorer
```

In `inspect` look for: `USER: root` (bad), secrets in `ENV`, exposed ports.

## Secrets inside the image

```bash
docker run --rm -it <image> sh -c "
  find / -name '*.env' -o -name '*.key' -o -name '*.pem' 2>/dev/null
  env | grep -iE 'password|secret|token|key|api'
  cat /etc/passwd | grep -v nologin
"
```

→ `save_finding()` for any discovered secrets

## Vulnerable packages

```bash
trivy image <image>
# or
grype <image>
```

Look at Critical and High CVEs in both OS packages and app dependencies.
→ `save_finding()` for critical issues

## Security configuration

```bash
docker inspect <container> | grep -E "Privileged|CapAdd|SecurityOpt|Binds|NetworkMode"
```

Red flags:
- `--privileged` → full host access
- `cap_add: SYS_ADMIN` → nearly the same
- Volume mounts: `/etc`, `/var/run/docker.sock` → host escape

→ `save_finding()` for privileged + docker.sock

## Network isolation

```bash
docker network inspect <network>
docker run --rm -it <image> sh -c "
  cat /etc/hosts
  nslookup kubernetes.default 2>/dev/null
  curl -s http://169.254.169.254/ 2>/dev/null
"
```

If cloud metadata is accessible → `save_finding()` immediately (Critical).

## Runtime analysis (if access to running container)

```bash
ss -tlnp          # what's listening inside?
ps aux            # processes
cat /proc/self/status | grep Cap
capsh --decode=<hex>
```

## Escape vectors

- `docker.sock` mounted → escape via Docker API
- `--privileged` → escape via `/dev/mem` or cgroup `release_agent`
- `SYS_ADMIN` + cgroup v1 → classic escape
- Shared PID namespace → see host processes

→ `save_hypothesis()` for each vector found

**Done when:** trivy complete, secrets checked, capabilities checked, network isolation verified.
