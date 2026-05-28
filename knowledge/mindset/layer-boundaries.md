---
category: mindset
title: "At the Seam Between Layers"
tags: [abstraction-layers, dirty-pipe, zero-day-methodology, kernel, layer-boundaries, uninitialized-data]
---

# At the Seam Between Layers

Dirty Pipe started with a corrupted log file. CRC error in a gzip. The contents looked fine, just the checksum was wrong. Max Kellermann traced it down through nginx, through splice(), through pipe buffers, through the page cache, and found that `pipe_buffer.flags` wasn't being initialized - so it inherited whatever garbage was on the stack. One of those garbage values happened to set `PIPE_BUF_FLAG_CAN_MERGE`, which meant writes to the pipe could merge into page cache pages that were supposed to be read-only.

The path from "customer's log file has a bad CRC" to "any unprivileged user can overwrite read-only files" is not obvious. It required following the bug down through multiple layers of abstraction until you found the point where two layers had incompatible assumptions about the state of the data between them.

---

That's the pattern. Bugs love living at boundaries. The developer working on layer A has a mental model of what layer B provides. The developer working on layer B has a mental model of what layer A needs. These models don't always match. The mismatch lives in the gap, and nobody reviews it because neither developer considers it their responsibility.

Classic boundary types where this shows up:
- User space / kernel (syscalls, ioctls)
- Synchronous / asynchronous (callbacks, signals, anything that interrupts normal flow)
- Trusted / untrusted data (the moment where external input first touches internal state)
- Serialization / deserialization (type information gets lost and reconstructed)
- Language runtime / OS (GC assumptions, FFI, anything with ownership questions)

---

When you're following a bug or looking for one, asking "what does this layer assume the layer below it has already done?" is often productive. Kellermann's full trace:

```
Corrupt CRC in gzip
  → nginx splice() data into gzip pipe
    → kernel splice uses a reference to page cache, not a copy
      → new pipe_buffer inherits flags from previous buffer
        → PIPE_BUF_FLAG_CAN_MERGE = 1 (stale)
          → write() can merge into page cache pages
```

At each step: what did the layer above assume the layer below had set up? At the pipe_buffer step, the answer was "flags are initialized to reflect the actual state of this buffer" - and that assumption was wrong.

```bash
# find where subsystems meet
grep -r "splice\|sendfile\|ioctl\|mmap" src/
# FFI / language boundaries
grep -r "unsafe\|extern\|JNI\|ctypes\|ffi" src/
# async transitions
grep -r "signal\|callback\|spawn\|async\|await" src/
```

Also worth specifically looking for: structs that cross layer boundaries without explicit initialization. In C especially, "zero-initialize everything" is correct but not always what happens.
