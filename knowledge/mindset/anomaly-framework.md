---
category: mindset
title: "Anomaly is the Message"
tags: [anomaly-detection, xz-backdoor, dirty-pipe, zero-day-methodology, behavioral-analysis]
---

# Anomaly is the Message

Most people dismiss weird behavior. That's the wrong instinct.

Andres Freund found the xz backdoor because SSH was 500ms slower than usual. He wasn't hunting for a backdoor. He was doing unrelated performance work and just... noticed. Then he pulled the thread. That thread went: sshd → liblzma → xz tarball → obfuscated test script → C macro injection → nation-state backdoor. The thing that saved the internet was one engineer who refused to shrug and move on.

Max Kellermann's story is even better. A customer filed a ticket: gzip logs have CRC errors. He looked at it, manually patched the CRC, closed the ticket. Forgot about it. Then it happened again. Then again. At some point he got annoyed enough to actually figure out *why* - and ended up finding a kernel privilege escalation that worked on every Linux system since 5.8.

Both of these started with the same moment: something was slightly off, and instead of finding a reason to ignore it, they followed it.

---

The mental shift is simple but hard to actually internalize: an anomaly means your model of the system is wrong. Not "there might be a bug here" - your model is *definitely* wrong somewhere. The code is doing something you didn't predict. That gap between expected and actual is exactly where interesting things live.

Most people's reflex is to explain anomalies away. "Probably network noise." "Maybe a deployment happened." "Could be my test environment." This is how you miss things. Every unexplained deviation is a question. Questions have answers. The answers are usually more interesting than whatever you were doing before.

---

**In practice:** before you start looking for bugs, establish what "normal" looks like.

```bash
# baseline response times
for i in 1 2 3 4 5; do
    curl -s -o /dev/null -w "%{time_total}\n" "https://target/api/endpoint"
done

# does response size change with different inputs?
curl -s -o /dev/null -w "%{size_download}\n" "https://target/api/user/1"
curl -s -o /dev/null -w "%{size_download}\n" "https://target/api/user/2"
```

Once you have a baseline, anything that deviates from it is worth at least five minutes of your time. A 401 that's 2KB instead of 100 bytes. An endpoint that's consistently 300ms slower with one specific parameter. A process that uses 3x more CPU on certain inputs.

These things might be nothing. They're also where findings come from.
