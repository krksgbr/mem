# Research Brief: Transcript Browser / Indexer Design Survey

## Goal

Study the transcript/session browser and indexer repos I cloned under:

`/Users/gaborkerekes/playground/ai-stuff/harness-utils/session-managers`

The purpose is to understand the range of architectural approaches represented in this curated set, extract useful ideas, and narrow down what fits this app's priorities and use case.

This is a design-space exploration, not a final implementation plan.

## Product Context

We are building a terminal-first, local-first transcript browser.

Current priorities:

- terminal-first CLI/TUI app
- local-first data model and UX
- Claude Code and Codex support now
- more providers likely later
- rich browsing matters, not just search/resume
- startup responsiveness matters
- transcript watching is a near-term goal
- avoid premature heavy architecture
- keep a path from spike to real maintainable product

## Scope

Focus only on the repos in:

`/Users/gaborkerekes/playground/ai-stuff/harness-utils/session-managers`

Do not search for other tools or broaden the project list.

You may use external sources only to better understand these curated repos, for example:

- upstream GitHub repositories
- project READMEs or docs
- issue discussions
- screenshots or demos

## Research Questions

Answer these questions based on the curated repos:

1. What architectural families of transcript/session browsers or indexers are represented in this set?
2. Which repos fall into each family?
3. How does each repo ingest data?
4. Does it read raw transcript files at runtime, maintain an index, or both?
5. How does it support multiple providers?
6. How does it model conversation structure:
   - flat session
   - conversation with segments
   - tree/forks/sub-agents
7. How does it support updates:
   - restart only
   - manual sync
   - file watching
   - incremental import
8. What are the UX patterns for browsing:
   - workspace/project list
   - conversation list
   - preview pane
   - transcript view
   - search-first vs browse-first
9. What startup/performance strategies are used?
10. Which patterns seem most compatible with our priorities?
11. Which patterns seem overbuilt for our current stage?
12. What 3-5 architecture directions should we seriously consider next?

## Key Dimensions To Extract

Please organize findings around these dimensions where possible:

- ingestion model
- normalization strategy
- summary-first vs full hydration
- persistent index vs raw-file scanning
- update model
- watch/sync strategy
- multi-provider extensibility
- transcript hierarchy model
- browse UX vs search UX
- failure handling and recovery
- startup latency tradeoffs
- operational complexity

## Sources

Prefer primary sources whenever possible:

- the local repo source itself
- upstream repo docs / code
- issue discussions / design notes

Be explicit when something is inferred rather than directly documented.

## Deliverables

Produce:

1. A final synthesis report at:
   `/Users/gaborkerekes/projects/transcript-browser/docs/research/<date>-transcript-indexer-survey.md`
2. One dated per-repo note for each repo you survey at:
   `/Users/gaborkerekes/projects/transcript-browser/docs/research/transcript-indexers/<date>-<repo-slug>.md`

Use `YYYY-MM-DD` for the date.

The final synthesis should include:

- a taxonomy of approaches
- a comparison matrix
- all relevant curated repos grouped by approach
- a shortlist of the best inspirations for this app
- a shortlist of patterns or repos that are probably not a fit
- a final synthesis focused on our priorities

Each per-repo note should include:

- a short intro with an ASCII architecture diagram showing the high-level shape of the solution
- repo name
- category / family
- purpose
- provider coverage
- ingestion / storage / update strategy
- transcript structure model
- UX shape
- strengths
- weaknesses
- a few relevant code snippets where useful to support big-picture understanding
- evidence: local file paths and/or upstream links as appropriate

## Constraints

- taxonomy first, recommendations second
- do not treat this as a final architecture recommendation yet
- do not optimize only for search; browsing matters a lot
- do not introduce non-curated tools into the survey
- call out uncertainty explicitly

## Suggested Execution Strategy

Use sub-agents to survey the individual repos in parallel.

Recommended approach:

- the main research agent should orchestrate and synthesize
- individual sub-agents should each study one repo or a small well-scoped subset
- the main research agent should not try to read every repo in detail itself
- the main research agent should use the per-repo reports as the primary inputs to the final synthesis

For each per-repo report:

- include an ASCII architecture diagram near the top so the overall structure is easy to glance
- include only selective code snippets that clarify important architectural choices
- do not drown the report in implementation detail; optimize for big-picture understanding

## Quality Bar

The result should:

- reflect the actual variety present in the curated repo set
- distinguish quick wins from mature architectures
- surface the important design tradeoffs clearly
- help us narrow down to a few serious candidates for a later focused design session
