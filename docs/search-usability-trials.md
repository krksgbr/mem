# Search Usability Trials

## Purpose

This document defines a lightweight usability-testing approach for `transcript-browser` search.

The goal is not to benchmark abstract retrieval quality in the large. The goal is to observe how an agent uses the tool to find the right conversation, what effort that takes, and where the workflow breaks down.

This follows the principles in [search-design-principles.md](./search-design-principles.md).

## Trial Structure

Each trial should give an agent:

- one retrieval task
- `transcript-browser` as the only retrieval tool
- a requirement to record its search path and reasoning about query reformulation

The agent should report:

1. the exact search queries it tried
2. why it changed query when it did
3. when it believed it had found the right conversation
4. what evidence made it confident or uncertain
5. what part of the UX/search behavior slowed it down
6. what concrete improvement would have helped most

## What To Measure

For each trial, capture:

- `time_to_first_plausible_hit`
- `time_to_confident_hit`
- `search_count`
- `read_count`
- whether the agent succeeded
- whether the agent was confident
- dominant friction category

Suggested friction categories:

- query formulation
- result ranking
- result labeling/snippets
- similar-session disambiguation
- transcript navigation
- missing literal retrieval surface

## Trial Rules

- The agent must use `transcript-browser` only.
- Do not allow `memex` during the trial.
- Do not ask the agent to optimize the query for the tool artificially.
- The task wording should resemble a real user request, not a benchmark keyword soup.
- If the agent fails, that is still a useful result.

## Instrumented Trial Mode

For experiment builds, compile the CLI with the `trial-tracing` feature.

The tracing-enabled build writes JSONL artifacts under:

- `~/.local/state/transcript-browser/trials/`

The trace currently records:

- each `search` query
- full returned search result lists with rank, title, and snippet
- each `read` call

Current protocol:

- run trials sequentially
- each trial run automatically writes to its own trace file
- commands within one active trial append to the same file
- after enough idle time, the next command starts a new trial file

This is intentionally simple. It is designed for sequential experiment runs without per-trial flags or manual cleanup. If we later need concurrent trials or explicit grouping beyond time-based run rotation, that should be added explicitly rather than inferred.

## Candidate Task Set

These are candidate tasks for the first round. They are intentionally small and aligned with the current product goal: conversation discovery first, deeper synthesis second.

### Task A: Known Conversation Name

Prompt:

> Find the conversation named `gen-ui-spike-2` and verify that it is the right monorail session.

Why this task exists:

- tests direct conversation discovery
- should be easy
- good sanity check for the workflow

Success criteria:

- the agent finds the `gen-ui-spike-2` conversation
- the agent can explain why it is the correct session

### Task B: Feature / Topic Lookup

Prompt:

> Find the conversation where we worked on Lightdash setup in `sibyl-memory-mvp`.

Why this task exists:

- tests broad feature/topic lookup
- likely depends on nouns, workspace scoping, and snippets

Success criteria:

- the agent finds a clearly relevant Lightdash conversation
- the agent can distinguish it from adjacent Lightdash-related sessions

### Task C: Similar-Session Disambiguation

Prompt:

> Find the Claude conversation where we worked on sticky notes in `~/unbody/bookmarking`, and distinguish it from nearby related branch conversations.

Why this task exists:

- tests nearby-session disambiguation
- depends on titles, snippets, and browser structure

Success criteria:

- the agent finds the intended sticky-note conversation
- the agent can distinguish it from related branches like `sticky-note-block-model-research`

### Task D: Recent Feature Lookup

Prompt:

> Find the recent Rhizome conversation about image capture / burst perception work.

Why this task exists:

- tests broad topical retrieval plus recency
- useful real-world agent task

Success criteria:

- the agent finds a recent relevant Rhizome conversation
- the agent can explain why it is likely the right one

### Task E: Stretch Task

Prompt:

> Find the conversation where we discussed why the `tree walk fallback` was added.

Why this task exists:

- tests rationale-oriented lookup
- should be treated as a stretch task, not a core success criterion

Success criteria:

- the agent finds the relevant conversation or clearly explains why the current search surface made the task hard

## Recommended First Trial

Start with **Task B: Feature / Topic Lookup**.

Why:

- it is realistic
- it is not trivial like a direct title lookup
- it is still well within the intended product scope
- it is less degenerate than a rationale/findings query

## Trial Prompt Template

Use a prompt like this for each agent trial:

> Use `transcript-browser` only.
> Do not use `memex` or raw filesystem grep.
> Your goal is to complete this retrieval task: `<task prompt>`.
>
> Record:
> - each search query you try
> - why you reformulate the query
> - when you think you found the right conversation
> - what evidence made you confident or uncertain
> - what UX/search problem slowed you down most
> - one concrete improvement you would make
>
> Stop once you either:
> - have a confident hit, or
> - can explain why the current product blocked you.

## Scaling Plan

Round 1:

- run one trial only
- use it to refine the prompt and measurement format

Round 2:

- run 3 to 5 trials across different task types
- compare friction patterns

Round 3:

- only after the prompt and task set feel stable
- use the results to prioritize concrete search/browser improvements
