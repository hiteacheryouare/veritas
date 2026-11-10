# Decisions

Every decision behind Veritas, so no question gets asked twice. D1–D24 are PLAN.md §11. The entries after them were made during the build. Newest entries go at the bottom.

## Foundations

### D1. Fork Actual, clean-room it, or start fresh?
- **Options:** fork `actualbudget/actual` / clean-room reimplementation / fresh repo with Actual as a read-only reference and test oracle
- **Decision:** Fresh repo. Actual is MIT, so we may read and port it with attribution (THIRD_PARTY_NOTICES.md). It's cloned at a pinned tag into git-ignored `reference/actual/`, and `@actual-app/api` at the same version is the oracle in `tools/actual-oracle/`. A fork would mean deleting about 90% of a React app and fighting upstream merges forever.
- **Date:** 2026-11-09

### D2. What is the core written in, and how do the UIs reach it?
- **Options:** Swift-only / TypeScript core like Actual / Rust core with generated bindings
- **Decision:** A Rust core shared by every UI through UniFFI (Swift now, C# for Windows later).
- **Date:** 2026-11-09

### D3. What is the working file format?
- **Options:** a Veritas-native schema with import/export translation / Actual's SQLite schema plus additive `veritas_*` tables
- **Decision:** The Actual-schema SQLite file *is* the native format, plus `veritas_*` tables. Round trip by construction; `veritas-store` is the only code that knows physical Actual names.
- **Date:** 2026-11-09

### D4. How lossless must interop be?
- **Options:** lossless for everything / lossless for Actual data only
- **Decision:** Lossless for Actual data. Veritas-only extras may be dropped by Actual; in practice they ride along in the raw db.
- **Date:** 2026-11-09

### D5. Live sync with actual-server?
- **Options:** be a CRDT peer / file import and export only
- **Decision:** File import and export only. Live sync is a non-goal; `messages_crdt` and `messages_clock` are never touched.
- **Date:** 2026-11-09

### D6. Which budget types does the v1 UI support?
- **Options:** envelope and tracking / envelope only, tracking preserved read-only
- **Decision:** Envelope only. Tracking ("reflect") files are preserved losslessly and open read-only. An assumption, since Ryan uses envelopes.
- **Date:** 2026-11-09

### D7. When is money counted as spent?
- **Options:** at swipe / at card payment
- **Decision:** At swipe. Loan interest is an expense, principal is the debt paydown line, and card payments are never spending.
- **Date:** 2026-11-09

### D8. Credit cards and loans: on or off budget?
- **Options:** migrate to a new model / keep Ryan's current setup
- **Decision:** Cards stay on-budget and loans stay off-budget. No forced migration.
- **Date:** 2026-11-09

### D9. What does closing a goal or deleting a category do to history?
- **Options:** re-file transactions like Actual's delete / budget move plus hide, never re-file
- **Decision:** Closing a goal is a budget move plus hide. Deleting a category with history defaults to Archive. A real delete is only behind an explicit confirmation. The theory that Actual re-files history on delete is checked in P0.4 (`docs/actual-format.md`).
- **Date:** 2026-11-09

### D10. How are recurring bills stored?
- **Options:** a Veritas-only bills table / Actual schedules plus their owned rule
- **Decision:** Actual schedules, with a per-bill auto-post or confirm-first choice (`posts_transaction`).
- **Date:** 2026-11-09

### D11. AI features?
- **Options:** none / cloud / on-device only
- **Decision:** Optional, off by default, on-device only (Apple Foundation Models), read-only.
- **Date:** 2026-11-09

### D12. UI technology and look?
- **Options:** cross-platform web UI / native SwiftUI + AppKit
- **Decision:** "Native but opinionated": SwiftUI with AppKit `NSTableView` where SwiftUI can't hit the perf budget. Minimum macOS 27.
- **Date:** 2026-11-09

### D13. How far does US tax support go?
- **Options:** summary export / form-line mapping / liability estimates
- **Decision:** A summary export (PDF and CSV) only.
- **Date:** 2026-11-09

### D14. Bank sync?
- **Options:** support it / out of scope
- **Decision:** Out of scope; Ryan enters everything by hand. Bank-sync columns are preserved untouched.
- **Date:** 2026-11-09

### D15. Actual's budget templates and automations?
- **Options:** execute them / preserve without executing
- **Decision:** Preserved but not executed in v1. An assumption Ryan didn't state.
- **Date:** 2026-11-09

### D16. Platform order?
- **Options:** any order
- **Decision:** macOS, then iOS, then Windows.
- **Date:** 2026-11-09

### D17. Personal tool or open source?
- **Options:** personal only / open source from day one / personal first, structured for open source
- **Decision:** Built for Ryan first, structured for open source later, with MIT attribution kept from day one.
- **Date:** 2026-11-09

### D18. How is Swift code organized?
- **Options:** everything in the Xcode project / local Swift packages with a thin app shell
- **Decision:** All Swift lives in local packages (`VeritasKit`, `VeritasMacUI`); the `.xcodeproj` is a thin shell so parallel agents don't collide in `project.pbxproj`.
- **Date:** 2026-11-09

### D19. Task runner?
- **Options:** just / make / npm scripts / `cargo xtask`
- **Decision:** `cargo xtask`. No globally installed build tools.
- **Date:** 2026-11-09

### D20. Name and pins?
- **Options:** —
- **Decision:** Veritas. Actual v26.11.0, macOS 27, GitHub repo `veritas` (planned private, made public in D28).
- **Date:** 2026-11-09

### D21. Who does the work?
- **Options:** —
- **Decision:** A fleet of Architect, code writers, code reviewers, coverage cowboys and Fred (PLAN §0.2).
- **Date:** 2026-11-09

### D22. What does a PR need to merge?
- **Options:** —
- **Decision:** CI green, a code reviewer, cowboy tests for every claimed R-ID, and Fred on `finance` PRs.
- **Date:** 2026-11-09

### D23. What does a gate pause?
- **Options:** everything / only dependent work
- **Decision:** Only work that depends on the gate.
- **Date:** 2026-11-09

### D24. Can Ryan's real budget be used for testing?
- **Options:** no / locally only
- **Decision:** Locally only, in git-ignored `fixtures/private/`. It never leaves the machine; only aggregate pass/fail results do.
- **Date:** 2026-11-09

## Build

### D25. How do the Rust toolchain and Xcode get onto the machine?
- **Options:** Ryan installs them / Docker-only Rust / the agent installs them
- **Decision:** The agent installs Rust user-locally into git-ignored `.tooling/` (`RUSTUP_HOME`, `CARGO_HOME`, no shell profile edits), and every trace is deleted when the run ends. Xcode needs Ryan's Apple ID, so Ryan installs it.
- **Date:** 2026-11-09

### D26. Which branch do PRs merge into?
- **Options:** `dev` integration branch / directly into `main` / keep `mega` as trunk
- **Decision:** `dev`. Workstream PRs merge into `dev` once they meet the merge bar. At each gate a `dev → main` PR is opened for Ryan to merge.
- **Date:** 2026-11-09

### D27. How is the merge bar enforced when every agent posts as Ryan?
- **Options:** sign-off status check / Ryan approves every PR / a separate bot account
- **Decision:** A `merge-bar` commit status computed from sign-off comments (`merge-bar: <role> <approve|reject> <sha> [R-IDs]`) by trusted accounts, tied to the head commit. See `.github/scripts/mergeBar.ts`.
- **Date:** 2026-11-09

### D28. Private or public repo?
- **Options:** private (macOS CI minutes billed at 10×) / public
- **Decision:** Public, which makes macOS runners free. `fixtures/private/` stays git-ignored and never leaves the machine.
- **Date:** 2026-11-09

### D29. How are PRs merged?
- **Options:** squash / merge commit / rebase
- **Decision:** Merge commits only, so commit messages survive.
- **Date:** 2026-11-09

### D30. Does the repo get AGENTS.md and a README?
- **Options:** seed them from PLAN Appendix C / leave them to Ryan
- **Decision:** Leave them to Ryan, along with logos and marketing. Agent rules stay in PLAN.md Appendix C until he writes AGENTS.md.
- **Date:** 2026-11-09

### D31. Whose conventions win where PLAN.md and Ryan's other repos differ?
- **Options:** PLAN.md / the other repos
- **Decision:** PLAN.md's explicit choices win (`xtask`, `thiserror`, `docs/adr/`). Everything else follows his Lattice & Co repos: tabs, LF stored and CRLF checked out, hard-tab rustfmt, ESM Node tools run with type stripping, and decision records in this format.
- **Date:** 2026-11-09

### D32. What direction does the design take?
- **Options:** Quiet native / Account book / What's left
- **Decision:** Quiet native's layout and calm, with fill bars that drain to show what's left, semantic color for every money state (always with an icon and a label), negatives written −34.75, and SF Rounded figures, so Veritas reads as its own app that happens to be native.
- **Date:** 2026-11-09

### D33. Gate 0
- **Options:** approve / approve with notes / hold
- **Decision:** Approved. docs/actual-format.md and docs/finance/ are the basis for Phase 1.
- **Date:** 2026-11-09

### D34. How are paychecks recorded?
- **Options:** net deposit / gross with withholding splits
- **Decision:** Gross with splits, so the W-2 income tax bucket is gross pay (docs/finance/classification.md CLS-10).
- **Date:** 2026-11-09

### D35. How do untagged on→off transfers count in Net Profit?
- **Options:** expense with a review count and a per-category wizard / excluded until reviewed
- **Decision:** Expense, plus a visible review count and a one-time wizard that sets a kind per category.
- **Date:** 2026-11-09

### D36. What does Veritas write into Actual for a goal?
- **Options:** a `#goal` note line plus per-month goal rows / goal rows only / nothing extra
- **Decision:** A `#goal <amount>` note line plus `zero_budgets.goal` / `long_goal` rows, so Actual shows progress once its goal-templates flag is on.
- **Date:** 2026-11-09

### D37. What happens to invalid input in an amount field?
- **Options:** keep the old value everywhere / match Actual per field
- **Decision:** Keep the old value everywhere, with a brief inline hint. Stored data still matches Actual.
- **Date:** 2026-11-09

### D38. What does quick entry fill for a known payee?
- **Options:** category and amount from the payee's last transaction / only what Actual's learned rules fill
- **Decision:** Category and amount from that payee's most recent transaction, computed on the fly. Nothing new is stored.
- **Date:** 2026-11-09

### D39. Fred's remaining finance defaults (FIN-2, FIN-3, FIN-5)?
- **Options:** accept / change
- **Decision:** Accepted. An off-budget asset purchase counts as an expense with a capitalized-transfers equity line. Escrow counts as an expense when paid. A no-spend day ignores bills and loan payments.
- **Date:** 2026-11-09

### D40. Accent color?
- **Options:** indigo (rejected, too close to Actual's purple) / graphite / petrol / olive
- **Decision:** Graphite. The interface stays neutral, so money is the only color on screen.
- **Date:** 2026-11-09

### D41. Which extra keyboard shortcuts?
- **Options:** ⌘1–⌘6, ⌘F, ⌘D, ⌥↑/⌥↓, ⌘Return, H in Wrapped
- **Decision:** All except H in Wrapped.
- **Date:** 2026-11-09
