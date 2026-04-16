# Lessons

## 2026-03-22

- Failure mode: I relied on crate tests and missed a warning that only showed up in a normal `cargo check` build because a test-only helper module was compiled into the main binary target.
- Detection signal: the user ran `cargo check` and surfaced a dead-code warning that my verification had not exercised.
- Prevention rule: for this repo, always run `cargo check` in addition to `cargo test -p shared` and `cargo test -p cli` before declaring the work verified.

## 2026-04-14

- Failure mode: I initially recorded interactive TUI `update` time around the input poll loop, which accidentally included the 16ms idle poll wait and misattributed the cost to state updates.
- Detection signal: the first interactive `--profile` report showed `update` dominating even though the known hot path was render work.
- Prevention rule: when profiling event loops, measure wait time and work time separately; never wrap blocking poll/sleep calls inside an application-phase timer.

- Failure mode: I ran multiple DB-backed CLI verification commands in parallel against the same SQLite index and triggered `database is locked` errors that obscured the actual rendering measurement.
- Detection signal: `profile-scroll` and `dump-screen` failed with `failed to clear contentless FTS index` / `database is locked` during parallel verification.
- Prevention rule: when verifying commands that auto-sync or mutate the shared local index, run them sequentially or point them at isolated state paths; do not parallelize them by default.

- Failure mode: I repeated the same parallel-verification mistake later in the session while checking the new rooted tree browser, which wasted time and produced non-actionable lock failures.
- Detection signal: real `dump-screen` runs for `conversations` and `messages` failed with `database is locked`, while the same commands succeeded immediately when rerun one at a time.
- Prevention rule: once a verification path is known to mutate shared SQLite state, treat parallel execution as invalid by default for the rest of the session; do not retry the same mistake.

- Failure mode: I also parallelized compile/test verification after already confirming this repo's Rust build and SQLite-backed checks should be run sequentially, causing repeated lock contention and wasted cycles.
- Detection signal: multiple `cargo check` / `cargo test` commands started together reported package-cache and build-directory lock waits instead of providing new information.
- Prevention rule: for this repo, run verification serially once the first command needs the build directory or local index; use parallel reads only for inspection, not for cargo or DB-mutating commands.

- Failure mode: I treated Claude sidechain `role:"user"` records as real human user messages, which mislabeled delegated subagent prompts as `You` in the browser.
- Detection signal: a real Claude conversation showed a nested `You` prompt that matched the assistant's `Agent` tool input rather than anything the user actually typed.
- Prevention rule: when importing harness transcripts, classify authorship from transcript provenance as well as message role; sidechain or tool-delegated prompts must not be rendered as human-authored user messages by default.

- Failure mode: I parallelized a manual `sqlite3` inspection against the same local index while another DB-backed verification command was active, and hit `database is locked` again.
- Detection signal: one of two concurrent inspection commands failed immediately with `Error: in prepare, database is locked (5)` while the equivalent serial command worked.
- Prevention rule: in this repo, treat *all* access to the shared SQLite index as serial during verification, including ad hoc `sqlite3` reads; do not assume a manual read is safe to parallelize just because it looks read-only.

- Failure mode: I initially collapsed Claude delegation sidechains by grouping only root rows, which left structural child replies unvisited and leaked subagent transcript messages back into the top-level conversation tree.
- Detection signal: the `burst-perception-integration` tree showed delegated task nodes followed by stray subagent assistant lines at the same visual level.
- Prevention rule: when collapsing a tree into summary rows, mark the full descendant subtree covered by the summary, not just the root nodes that triggered the collapse.

- Failure mode: I made browser visibility of real forked child conversations depend on successful branch-anchor message resolution inside the parent transcript, which caused valid children to disappear entirely when the anchor message was missing or unmatched.
- Detection signal: `cc-sessions` showed `research-building-a-user-model` with child forks, but transcript-browser showed the row as non-expandable or empty on expand.
- Prevention rule: in the conversation browser, real forked child conversations must remain visible from parent→child session lineage alone; anchor-message placement is an optional refinement, not a prerequisite for visibility.

- Failure mode: I checked for a newly added skill only in the global user skill directory and missed the repo-local skill the user had pointed me to.
- Detection signal: the user explicitly said the skill was "here in this repo" after I incorrectly reported it missing.
- Prevention rule: when a user points to a skill path, resolve it relative to the current repository before assuming it should exist in the global skill directories.

- Failure mode: I changed how conversation titles/previews are derived from transcript content, but left the rebuildable SQLite index at the same schema version, so the browser kept serving stale conversation summaries while `read` recomputed the new title on demand.
- Detection signal: the same conversation rendered as `/login` in `read`, but still appeared as a raw internal conversation ID in the conversations browser.
- Prevention rule: when changing any persisted derivation logic that affects indexed summaries or query-visible fields, bump the SQLite schema/index version so the derived store is rebuilt instead of silently mixing old indexed data with new runtime logic.

- Failure mode: I initially treated `tui-use` as broken when the real blocker was sandbox access to `~/.tui-use`, which the tool needs for its daemon socket and session state.
- Detection signal: `tui-use start` failed with `EPERM: operation not permitted, mkdir '/Users/gaborkerekes/.tui-use'`, and `nono why` reported `path_not_granted` for write access to that path.
- Prevention rule: when evaluating local tooling that maintains daemon or session state under the home directory, check its state path and sandbox access first; do not diagnose higher-level tool behavior until that prerequisite is verified.

- Failure mode: I navigated `transcript-browser` under `tui-use` by raw key counts, which made me inspect the wrong workspace/conversation and reduced the value of the captured snapshots.
- Detection signal: the PTY snapshots were readable and highlighted the selected row correctly, but the later interaction sequence landed in `~/.config/konfigue` instead of the intended bookmarking workspace.
- Prevention rule: use `tui-use` as an exploratory interaction tool, but anchor scripted navigation to visible text or explicit searchable state rather than relative key counts whenever the target matters.

- Failure mode: applying `Event::SetWorkspaces` after background sync unconditionally reset the app to `Screen::Workspaces { selected_workspace: 0 }`, which made the UI "bounce" back to home after a delayed refresh.
- Detection signal: the user consistently observed the app returning to the workspace screen after roughly the background-sync completion window, and the core update handler showed `SetWorkspaces` hard-resetting the screen regardless of current navigation state.
- Prevention rule: background data refresh events must preserve current navigation context by stable workspace/conversation/message identity whenever possible; never treat a snapshot refresh as an implicit navigation reset.

- Failure mode: I let the conversation browser hydrate the currently selected conversation during ordinary list navigation, which turned simple up/down movement into transcript preloading and made large workspaces feel sluggish.
- Detection signal: interactive profiles on the conversations screen showed `hydrate` and `view` dominating each Down key even though the browser was only showing conversation lineage, not transcript content.
- Prevention rule: collapsed browse surfaces must not trigger transcript hydration on selection change; only detail views or explicitly expanded structures should pay the load cost.

- Failure mode: I persisted raw provider `title`/`preview` fields into the SQLite index even when the domain-level title derivation would have sanitized or skipped structural wrapper tags like `<role>`, so the browser showed placeholder markup as conversation labels.
- Detection signal: a real Codex conversation list rendered many rows as `<role>`, while `read` showed the first actual prompt line immediately after that wrapper tag.
- Prevention rule: when storing indexed conversation summaries, persist the same derived title/preview semantics the browser uses at runtime; do not let raw provider metadata bypass normalization rules.

- Failure mode: I implemented Claude thinking extraction against the wrong field (`text`) even though the real transcript shape stores reasoning text under `thinking`, so the rebuilt index still contained zero `thinking` entries.
- Detection signal: a raw Claude JSONL line clearly contained `{"type":"thinking","thinking":"..."}`, but SQLite `conversation_entry` counts for that session still showed only `assistant_message`, `user_message`, and `metadata_change`.
- Prevention rule: when normalizing provider-specific blocks, verify the exact raw field names against real transcript samples before declaring the importer done.

- Failure mode: I assumed a usability-trial agent's qualitative account of search ranking was accurate, but the built-in trace showed the chosen conversation was ranked 10th, not first, and the trace itself did not include enough result context to audit that discrepancy without replaying the query.
- Detection signal: the trial report said the correct Lightdash setup conversation was the top result, while `trial-trace.jsonl` plus a direct `search lightdash --limit 10` replay showed different top-ranked conversations and the target thread at rank 10.
- Prevention rule: when instrumenting search usability trials, capture enough result context in the trace to audit the agent's claims directly (at least top result titles/snippets or stable rank positions), rather than relying on self-report alone.

- Failure mode: the `read` CLI required users to remember an internal `--conversation` selector shape even though the surrounding workflow starts from search results and human-recognizable titles, which made transcript inspection feel harder than it needed to.
- Detection signal: multiple usability-trial agents reported that `read` syntax was opaque or non-obvious, despite already having enough information from search output to identify the target conversation.
- Prevention rule: follow-up inspection commands should accept the most natural selectors already visible in the product surface, and they should expose built-in help/examples rather than forcing users to reconstruct internal identifier syntax from memory.

- Failure mode: I spent time tuning SQLite FTS ranking and query tiers before verifying that the FTS table was actually returning stored conversation context; the table was still configured as contentless, so the context columns came back blank and the product was often falling through to weaker `LIKE` behavior.
- Detection signal: direct SQLite inspection showed `conversation_fts` rows matching queries but returning empty `conversation_id` / `entry_id` / snippet context, while conversation search missed cases that should have been easy lexical wins (`sticky note`) until the schema was changed to a normal stored FTS table.
- Prevention rule: when iterating on search quality, first verify that the retrieval path is genuinely active end-to-end on real data (stored fields, snippets, ids, ranking source) before tuning weights or query construction.

- Failure mode: I let Codex `<user_shell_command>` wrappers participate in conversation title/preview/opening-prompt derivation as if they were ordinary user-authored prose, which caused shell-debug sessions like `which lightdash` to outrank the actual topical conversation for broad project-scoped queries.
- Detection signal: the scoped query `a couple days ago we discussed lightdash in sibyl-memory-mvp` returned a command-wrapper conversation at rank 1 until the shared domain model was inspected against the real transcript and the wrapper shape was made explicit.
- Prevention rule: treat provider-specific command wrappers as low-signal structural artifacts for summary derivation; only fall back to their command text when a conversation is truly command-only.

- Failure mode: I initially treated artifact-like queries as only punctuation-shaped tokens (dotted identifiers, paths, flags, slash commands), which meant CamelCase technical identifiers such as `VZBridgedNetworkDeviceAttachment` were neither indexed as artifacts nor eligible for artifact typo correction.
- Detection signal: the exact query `VZBridgedNetworkDeviceAttachment` found the right `nix-darvm` conversation, but the light typo variant `VZBridgdNetworkDeviceAttachment` returned no results at all.
- Prevention rule: artifact extraction and artifact-query routing must cover CamelCase/PascalCase technical identifiers in addition to punctuation-shaped tokens; otherwise typo handling silently excludes a common class of technical terms.

- Failure mode: I treated explicit workspace mentions like `in sibyl-memory-mvp` as only a soft ranking hint, and I also let the workspace name leak into artifact-term extraction. That let scoped searches rank on the project name itself instead of the actual topic/artifact.
- Detection signal: scoped queries such as `servicusage.services.use in sibyl-memory-mvp` and `a couple days ago we discussed lightdash in sibyl-memory-mvp` returned in-workspace results, but the top hits were initially dominated by irrelevant conversations whose only strong match was the workspace token.
- Prevention rule: when a search query explicitly names a workspace/project, use it as a hard filter and strip that workspace token family from all lexical/artifact query terms before ranking.
