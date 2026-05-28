---
category: methodology
title: "Where to Start"
tags: [start-here, router, checklist, engagement-start]
---

# Where to Start

First thing on any engagement: identify what you have, then fetch the right checklist.

## What you have → search query to run

**Website / web app (no source code)**
→ `search_knowledge("web blackbox pentest checklist enumerate probe")`

**Web app + source code**
→ `search_knowledge("web whitebox source code live app pentest checklist")`

**Source code only (no live system)**
→ `search_knowledge("source code review security audit static analysis checklist")`

**Binary / executable**
→ `search_knowledge("binary analysis reverse engineering checksec ghidra checklist")`

**Docker image / container**
→ `search_knowledge("docker image container security analysis trivy escape checklist")`

**Corporate network / infrastructure**
→ `search_knowledge("infrastructure network corporate pentest active directory lateral movement checklist")`

## Universal first 2 minutes (any scenario)

1. `set_session()` - lock in target and scope
2. `recall_engagement_state()` - check if you've worked on this before
3. Run the search query above - do not skip

## Workflow anchors

- Found something suspicious → `save_hypothesis()`
- Confirmed with PoC → `save_finding()`
- Hit a dead end → `save_dead_end()`
