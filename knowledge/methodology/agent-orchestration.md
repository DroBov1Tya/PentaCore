---
category: methodology
title: "Multi-Agent Orchestration for Security Research"
tags: [orchestration, multi-agent, parallelization, sub-agents, workflow]
---

# Multi-Agent Orchestration for Security Research

The basic idea: you are the orchestrator. You don't execute tasks - you decompose problems, assign work, evaluate results, and decide what happens next. Sub-agents handle execution. They have access to the same PentaCore MCP you do.

This works well when tasks are genuinely parallel and independent. It fails when the orchestrator doesn't understand the domain well enough to evaluate what agents return. Don't use this as a way to avoid thinking. Use it as a way to do more things simultaneously.

---

## The orchestrator's job

You do exactly three things:

**1. Decompose.** Break the current objective into tasks that can be executed independently. "Test authentication" is not a task. "Find all endpoints that accept credentials and document their request format, response codes for valid/invalid inputs, and whether timing differs between them" is a task.

**2. Evaluate.** Read what agents return and decide whether it's good enough. An agent that returns "no issues found" might have done thorough work or might have checked two things and stopped. You need enough domain knowledge to tell the difference. If you can't evaluate the result, you can't use it.

**3. Synthesize.** Connect the dots between what different agents found. The auth agent found that user enumeration is possible via timing. The recon agent found a password reset endpoint. Those two things together are more interesting than either alone.

---

## How to write sub-agent prompts that actually work

The most common failure: vague prompts that let the agent decide what "done" looks like.

**Bad:** "Test the authentication system for vulnerabilities."

**Good:**
```
Target: https://example.com
Task: Map the authentication surface.

Specifically:
- Find all endpoints that accept credentials (login, registration, password reset, 2FA, OAuth callbacks)
- For each endpoint: document the request format, which fields are required
- Test whether the login endpoint responds differently (timing or response size) for valid vs invalid usernames
- Check whether password reset tokens are predictable or expire correctly

Use PentaCore:
- read the "Anomaly is the Message" mindset entry before starting
- save_hypothesis() for anything suspicious
- save_dead_end() when you rule something out

Return: a structured list of endpoints found, timing results with numbers, and any hypotheses you saved.
Do NOT: scan for other vulnerability classes, test authorization, or do anything outside auth.
```

The key parts: explicit scope, explicit deliverable, explicit PentaCore instructions, explicit "do NOT." Without the last one, agents tend to drift.

---

## Which tasks are worth parallelizing

Parallelize when tasks are independent - meaning agent A's work doesn't affect what agent B needs to do.

Good parallel tasks in a web pentest:
- Recon (enumerate endpoints, build the surface map)
- Git history analysis (security commits, regression hunting)
- Dependency audit (npm/pip/cargo audit)
- Auth flow mapping
- Static analysis for a specific vulnerability class

Bad parallel tasks:
- Anything where one result should change the other's approach
- Confirmation of hypotheses (one agent finds it, you review, then decide whether to exploit - don't parallelize this)
- Tasks that write to the same finding without coordination

---

## PentaCore in a multi-agent setup

This is where it gets useful. Every agent can read from the same knowledge base and write to the same engagement state. Set this up explicitly in each agent's prompt:

```
At the start:
1. recall_engagement_state() - see what's already been found
2. search_knowledge("[relevant query]", domain="global") - get relevant techniques

During work:
- save_hypothesis() for anything suspicious (include your confidence level)
- save_dead_end() for things you ruled out and why

At the end:
- Summarize what you found and what IDs you saved
```

The orchestrator then reads the engagement state after agents finish, not individual agent outputs. This is important: you're looking at the *database*, not the chat output. The chat output is ephemeral. The database is the truth.

---

## The orchestrator loop

```
1. recall_engagement_state()           → understand current state
2. get_phase_playbook()                → what phase, what should be happening
3. Decompose into 2-4 parallel tasks
4. Launch agents with specific prompts
5. When agents complete: recall_engagement_state() again
6. Evaluate: are the hypotheses they saved reasonable? Did they miss obvious things?
7. Decide: move to next phase, dig deeper on something, or launch follow-up agents
8. transition_phase() when appropriate
```

Don't skip step 6. Agents save whatever they found. Some of it is noise. Some hypotheses need follow-up that requires your judgment, not another agent. The orchestrator has to make the call.

---

## When NOT to use sub-agents

- When you need to iteratively probe something based on live responses. The back-and-forth of "try this → see result → adjust → try again" doesn't parallelize well.
- When confirming and writing up a finding. You should do this yourself. The finding is important enough to not delegate.
- When the task requires synthesizing context from the whole engagement. Nobody has that context except you.
- When the task takes less than a few minutes. Overhead of spawning an agent isn't worth it for small things.

---

## A pattern that works: the sweep + depth model

Start with broad parallel sweeps: multiple agents each covering one area quickly. They save hypotheses, rule things out, document the surface. You read the results, identify the two or three most interesting threads. Then you either dig into those yourself or spawn focused agents with very specific mandates ("confirm whether this hypothesis is exploitable and produce a PoC or rule it out definitively").

The sweeps give you map coverage. The depth agents follow the interesting paths. You're the one deciding which paths are interesting. That judgment is the part that doesn't get delegated.
