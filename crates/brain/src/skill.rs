/// Bundled Codex skill installed to `$CODEX_HOME/skills/alexandria-memory/SKILL.md`.
pub const ALEXANDRIA_MEMORY_SKILL: &str = r#"---
name: alexandria
description: Use Alexandria persistent memory before answering when prior context, decisions, preferences, project facts, or open threads may exist. Recall first, honor five-state results and response_mode, remember durable outcomes after acting, consult style for tone, and never quote relational memory.
---

# Alexandria memory loop

You have Alexandria memory tools via MCP (`alexandria` server). Follow this loop on every task where prior knowledge might matter.

## Before you answer

1. Call **`recall`** with a focused query derived from the user's task.
   - For a broad question, prefer specific, content-bearing terms over filler.
   - To see how memory is organized, call **`catalog`** — it lists the available
     collections and tags (with counts), the structural table of contents.
   - When you want a precise, enumerable slice of memory rather than a fuzzy
     match, scope the `recall` call with `collections` and/or `tags` (structured
     recall). This returns exactly the engrams in those facets, deterministically,
     and is the reliable path whenever fuzzy matching is ambiguous or you need
     completeness over ranking. Use whatever facets the catalog shows.
2. Read the result carefully:
   - **`state`**: `strong_hit`, `weak_hit`, `high_confidence_gap`, `low_confidence_gap`, or `nothing`
   - **`response_mode`**: `flow`, `humility`, or `audit`
3. Act according to state and mode:
   - **`strong_hit` / `weak_hit` + `flow`**: Use retrieved claims confidently; cite engram ids when helpful.
   - **Gap states (`high_confidence_gap`, `low_confidence_gap`) or `humility`**: Hedge explicitly. Say what you think you know but cannot retrieve cleanly. Do not fabricate specifics.
   - **`nothing`**: Do not invent memory. Proceed from the task and general knowledge only.
   - **`audit`**: Prefer provenance. Use **`trace`** on important claims before relying on them.
4. Use **`expand`** only for hits worth the token cost — not every match.
5. Call **`threads`** when the topic may involve unresolved decisions.
6. Call **`style`** for tone/pacing parameters. Apply them silently — **never quote relational memory or paste style evidence**.

## While you work

- Link related facts with **`link`** when relationships matter (`supports`, `depends_on`, `conflicts_confirmed`, etc.).
- Use **`timeline`** for temporal context; **`trace`** for provenance chains.

## After you act

Persist durable outcomes with **`remember`**:
- First line = short claim; rest = supporting detail.
- Set **`tier`**: `episodic` for events, `semantic` for facts, `procedural` for how-tos, `relational` only for interaction preferences (never quotable).
- Add **`sources`** (e.g. `conversation:<thread_id>`) and **`derived_from`** when appropriate.
- Use **`collections`** and **`tags`** to keep memory organized — consistent
  facets are what make later structured (`collections`/`tags`-scoped) recall
  precise, so file durable facts under stable, reusable facets.

## Meta and maintenance

- On user correction in a domain, call **`meta`** with `record_correction: true`.
- When a gap state was warranted, call **`meta`** with `record_gap: true` and the appropriate `gap_kind`.
- Do not call **`consolidate`** during normal turns — the brain loop runs consolidation after you finish.

## Hard rules

- **Never quote relational tier content** in user-visible output.
- **Never pretend recall succeeded** when state is a gap or `nothing`.
- Prefer **`remember`** over hoping the user will restate facts next session.
"#;
