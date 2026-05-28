---
category: methodology
title: "Instrumentation Before Research - Baseline and Anomaly Measurement"
tags: [instrumentation, timing-oracle, baseline, side-channel, behavioral-analysis, recon, anomaly-detection]
---

# Instrumentation Before Research

You can't notice an anomaly if you're not measuring. Andres Freund noticed the xz backdoor because he was benchmarking at the time. Without a baseline, there is no anomaly - just "it works."

First thing on any target: set up measurements before you start looking for anything.

---

**Timing.** The most useful thing to measure for web targets.

```bash
for endpoint in /api/user /api/admin /api/search /api/export; do
    printf "%s: " "$endpoint"
    for i in 1 2 3 4 5; do
        curl -s -o /dev/null -w "%{time_total} " "https://target$endpoint"
    done; echo
done
```

Now you have a baseline. If something consistently deviates from it, that's a question.

The specific pattern worth checking on every login endpoint: does the response time differ for valid vs invalid usernames?

```bash
time curl -s -d "user=admin&pass=wrong" https://target/login
time curl -s -d "user=nobody9999&pass=wrong" https://target/login
```

Difference >10ms is enough to leak user existence. It also tells you the password check only runs when the username is valid, which says something about the code structure.

Same idea for token validation - `strcmp` vs constant-time comparison leaks through timing.

---

**Response size.** Often overlooked, often useful.

```bash
curl -s -o /dev/null -w "%{size_download}\n" https://target/api/user/1
curl -s -o /dev/null -w "%{size_download}\n" https://target/api/user/2
```

Different sizes for different user IDs can mean information disclosure. A 401 that's 2KB instead of 100 bytes is worth a look. Different error sizes for "wrong username" vs "wrong password" is user enumeration even without timing.

---

**For binaries and network services:** `strace` and `ltrace` before you open a disassembler.

```bash
strace -e trace=network,file,process ./target 2>&1 | head -100
ltrace ./target 2>&1 | grep -E "malloc|free|strcpy|memcpy|open"
```

What files does it read? What network connections does it make? What environment variables does it touch? This is faster than guessing from static analysis and tells you where to look.

---

The workflow: before you start, spend 15 minutes establishing what normal looks like. Any deviation from normal during testing goes into `save_hypothesis()` with the specific numbers. "This endpoint is 200ms slower with `sort_by=*`" is a hypothesis. "Something feels slow" is not.
