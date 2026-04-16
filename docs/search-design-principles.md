# Search Design Principles

## Purpose

`transcript-browser` search exists to help humans and agents find the right conversation quickly, then inspect it efficiently.

It is **not** trying to be a general-purpose document retrieval engine for every possible transcript artifact or every bag-of-words query that happened to work in another tool.

This distinction matters. Without it, it is easy to optimize for impressive benchmark queries that do not actually improve the product.

## Primary Product Goal

The search surface should make it easy to answer questions like:

- "What conversation was this about?"
- "Where did we discuss this topic?"
- "What was the session about Lightdash?"
- "Find the Claude conversation where we worked on sticky notes."
- "Find the recent Rhizome conversation about image capture."

Search should primarily help the user:

1. find the right conversation
2. distinguish it from similar conversations
3. open it and navigate it efficiently

The transcript reader and browser then handle the deeper inspection work.

## Core Principle

Search should optimize first for **conversation discovery**, not for **arbitrary transcript forensics**.

That means we should bias toward:

- conversation-level retrieval
- good titles and snippets
- strong project/workspace scoping
- recency-aware ranking
- salient term matching

before we optimize for:

- exact tool-command archaeology
- fuzzy "what did we learn" synthesis
- long bag-of-words queries assembled to coax a lexical engine into a result

## Query Classes

### 1. First-Class Queries

These are core product targets. Search should work well for them.

- conversation titles or known names
- project names
- feature names
- distinctive nouns or phrases
- provider/session identifiers
- broad "session about X" lookups

Examples:

- `gen-ui-spike-2`
- `lightdash`
- `mycelium`
- `sticky-note`
- `burst perception`
- `serviceusage`

If these fail, the product is failing at its main job.

### 2. Stretch Queries

These are worthwhile, but they depend on stronger summaries, ranking, and intent handling.

- "what did we learn about X"
- "what decision did we make about Y"
- "latest feature implemented for Z"
- "why did we add this fallback"

These are legitimate future goals, but they should not distort the core search design before conversation discovery is solid.

### 3. Stress-Test Queries

These are useful for exposing retrieval gaps, but they should not automatically become first-class product requirements.

They often take the form of:

- keyword soups
- reasoning-oriented bag-of-words queries
- exact code/tool-surface archaeology queries copied from prior retrieval workflows

Examples:

- `comparison verdict recommendation decision spike`
- `stuck blocked friction error workaround`
- `thread formation nucleation crystallize nutrient`
- `home wipe intentional clean slate sandbox isolation`

These tell us something about current weaknesses, but they are not the ideal user interaction model for `transcript-browser`.

## Relationship to Memex

`memex` and `transcript-browser` should not be judged by identical standards.

`memex` is closer to a transcript/document retrieval engine. It is good at:

- literal command lookup
- raw artifact retrieval
- throwing many terms at a large lexical index

`transcript-browser` is supposed to be better at:

- finding the right conversation
- giving the user or agent a usable summary of what that conversation is
- opening the right session quickly
- navigating inside that session

That means memex-style queries are useful comparison points, but not all of them should define the product roadmap.

## What We Should Optimize First

Before adding clever query modes or overfitting to stress tests, search should get these right:

1. If the user knows roughly what the conversation was about, they can find it.
2. If the user remembers one memorable term, feature, or project name, they can find it.
3. If there are several similar conversations, the snippets/titles are good enough to distinguish them.
4. Once the conversation is opened, the browser/transcript view makes inspection efficient.

## What We Should Explicitly Avoid

- optimizing the whole search UX around synthetic bag-of-words prompts
- treating every memex query as a desired first-class `transcript-browser` query
- conflating "help me find the conversation" with "answer the whole research question in one search call"

## Practical Evaluation Rule

When evaluating a search improvement, ask:

1. Does this make it easier to find the right conversation?
2. Does it improve distinction between similar conversations?
3. Does it help both humans and agents in the normal browse/open workflow?
4. Or does it only improve exotic retrieval cases that belong to a different tool category?

If the answer is mostly the fourth, it should probably be treated as a secondary enhancement, not a core search priority.

## Current Known Gaps That Still Matter

Even with the principles above, some gaps are clearly real and worth returning to:

- literal tool-command retrieval is weak
- command/artifact-heavy sessions may be underrepresented in search
- conversation summaries/snippets are not yet strong enough for some similar-session disambiguation
- rationale/findings/latest-style queries need better support if they become a recurring real workflow

Those are valid issues. The point of this document is only to keep us from optimizing them out of proportion to the main product goal.
