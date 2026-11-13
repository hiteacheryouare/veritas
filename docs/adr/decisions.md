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

### D42. Who signs off a contract PR the Architect wrote?
- **Options:** an independent architect agent / Ryan / the code reviewer alone
- **Decision:** The code reviewer alone. Those PRs carry the `architect-authored` label, and the merge-bar check then doesn't require an Architect sign-off.
- **Date:** 2026-11-09

### D43. Gate 1
- **Options:** approve / approve with changes / not yet
- **Decision:** Approved on the third round (design/mockups/). Round 1 cut AI-isms and the purple accent. Round 2 added the account balance graph and tags.
- **Date:** 2026-11-09

### D44. How do tags look?
- **Options:** colored text / pills like Actual
- **Decision:** Pills like Actual's: colored rounded badges inline after the note text.
- **Date:** 2026-11-09

### D45. What does hiding a tag do?
- **Options:** hide from pickers only / hide everywhere in the UI
- **Decision:** A hidden tag drops out of the tag filter and the # autocomplete. It still shows in notes and still counts in reports and tax summaries.
- **Date:** 2026-11-09

### D46. Engine defaults the Architect settled
- **Options:** —
- **Decision:** A leading `-` appends, like Actual (`100.00` then `-5` gives `95.00`). Empty input and malformed input such as `(5` keep the old value (D37). A budget with no income group opens with income 0 and an audit warning; Actual crashes on such a file.
- **Date:** 2026-11-09

### D47. How does Veritas evaluate Actual's regex `matches` rule conditions?
- **Options:** add the `regex` crate / keep those rules read-only
- **Decision:** Add `regex` (rust-lang) to Appendix B. JS-only constructs it can't express, such as lookaround and backreferences, still flag the transaction "may need Actual".
- **Date:** 2026-11-10

### D48. Does D42 cover a contract PR the Architect files from another workstream's commit?
- **Options:** the Architect signs it / treat it as architect-authored
- **Decision:** Treat it as architect-authored. The Architect opening or cherry-picking a contract PR counts as authoring it, so the auto-mode classifier's self-approval block applies, and the independent reviewer clears it (D42). First used on PR #21.
- **Date:** 2026-11-10

### D49. Who gives the finance sign-off on a spec PR Fred wrote?
- **Options:** an independent Fred instance / the code reviewer alone / Ryan
- **Decision:** An independent Fred instance, a fresh agent that had no part in writing the spec, re-derives the examples and signs off as `fred`. Fred's earlier self-sign-offs on #6, #10 and #18 happened before this rule.
- **Date:** 2026-11-10

### D50. Where do scenario types live, and how are they stored?
- **Options:** keep them in veritas-forecast / move them to veritas-domain
- **Decision:** Move them to `veritas-domain/src/scenario.rs` (Scenario, ScenarioEvent, ScenarioChange, Cadence, IncomeDelta, ScenarioEventId) and add them to BudgetData. `veritas_scenario.created_at` is unix milliseconds. The store validates event rows on load and skips invalid ones, reporting them the way `LoadReport`/`SkippedRow` already do. Per-kind CHECKs wait for schema v2. The transfer counterpart column is deferred. Events are ordered by id.
- **Date:** 2026-11-10

### D51. Which audit findings make a budget read-only?
- **Options:** only damage SQLite finds / any finding
- **Decision:** Only damage SQLite itself finds (`AuditFailed`). Other findings are reported and the budget stays writable, which matches import accepting non-corrupt budgets that have findings.
- **Date:** 2026-11-10

### D52. Does Veritas need Actual to start?
- **Options:** Actual-first (import is the way in) / Veritas stands on its own
- **Decision:** Veritas stands on its own, with excellent Actual interop (R-IO-0). Ryan chose to keep the Actual-format SQLite file as Veritas's native storage, so a budget made in Veritas is a valid Actual budget from the first write and export stays lossless by construction.
- **Date:** 2026-11-10

### D53. What does first launch offer?
- **Options:** a guided new budget first / a blank new budget first / two equal choices
- **Decision:** Two equal choices, New budget and Import from Actual, with neither one primary. This replaces PLAN §9 scenario 20.
- **Date:** 2026-11-10

### D54. Which categories does a new budget start with?
- **Options:** Actual's seed set / a Veritas starter set / empty
- **Decision:** A Veritas starter set that Fred designed to fit the classification layer (kinds, savings, debt and tax buckets already set), in docs/finance/starter-categories.md: 35 categories in 9 groups. It reuses Actual's seed rows by id and keeps "Starting Balances" exactly. Ryan approved it as proposed on 2026-11-11.
- **Date:** 2026-11-11

### D55. Mac shell UI defaults
- **Options:** keep the shell's defaults / overlay palette / review screenshots first
- **Decision:** Ryan kept the shell's defaults: Apple title case in menus and sentence case elsewhere, the ⌘K palette as a native sheet, ⌘F as a find bar under the toolbar, "Integrity report" as the user-facing name of the audit, and the placeholder bundle id `app.veritas.Veritas` with ad-hoc signing until he picks a domain and team.
- **Date:** 2026-11-10

### D56. Does new-budget setup create 401(k) and HSA accounts?
- **Options:** offer to create them / only mention them / drop those categories
- **Decision:** Offer to create them. Setup asks whether you have a 401(k) or HSA and creates off-budget accounts for the ones you check, so gross-paycheck contributions classify as savings (CLS-10).
- **Date:** 2026-11-11

### D57. How are paycheck withholding categories budgeted?
- **Options:** normal envelopes / auto-cover from the paycheck
- **Decision:** Normal envelopes, as in Actual. Suggestions (R-FC-3) propose the usual withholding so one click covers it. Veritas never budgets them on its own.
- **Date:** 2026-11-11

### D58. Do asset purchases lower projected net worth? (forecast ruling 10)
- **Options:** count them, with a note / add them back / ask per category
- **Decision:** Add them back. The forecast's monthly series leaves out on- and off-budget capitalizing transfers (CLS-4 rows 8 and 9), the way ST-4 E2 does, and shows "N asset purchase(s) excluded". Spec: forecasting.md FC-1 and FC-3 (AP-1).
- **Date:** 2026-11-11

### D59. New-budget flow design sign-off (§5.3)
- **Options:** approve / approve with changes / rework
- **Decision:** Ryan approved the first-launch screen and the three-step new-budget setup as shown in the 2026-11-11 screenshots. The Accounts step uses an account type picker instead of an off-budget checkbox.
- **Date:** 2026-11-11

### D60. What does the session write when a rule can't run in Veritas?
- **Options:** apply the rest and record the note / write nothing from that run and show the note
- **Decision:** When any rule in a run is undetermined, the session writes nothing from that run and shows the note; this replaces "apply the rest and record the note". The transaction, and any payee a rule would have made, is left for Actual. `RunOutcome.transaction` is `None` for such a run, and a schedule plan moves a bill with such a run into `SchedulePlan.left`, with none of its posts, writes or new payees, and leaves out the `lastScheduleRun` update. (Narrowed by D64: a hand-typed row is saved as typed.)
- **Date:** 2026-11-11

### D61. Sidebar group label and new-budget account types
- **Options:** "Investments" / "Other assets" / Ryan's own wording
- **Decision:** The third sidebar group reads "Investments & Other assets". Setup offers ten account types (Checking, Savings, Cash, Credit card, Line of credit, Investment, Retirement, Property, Loan, Mortgage), with debts entered as a positive "Amount owed". A line of credit is on budget like a card (D8), and the setup HSA account is an Investment.
- **Date:** 2026-11-11

### D62. Currency codes that aren't in Actual's table (CLS-12.7)
- **Options:** 2 places and an audit finding / refuse read-write
- **Decision:** Use 2 places and list the code in the audit, as Actual does (`?? 2`, currencies.ts:85). Refusing would break a budget Actual opens fine.
- **Date:** 2026-11-12

### D63. Zero-decimal currencies (CLS-12.8)
- **Options:** follow Actual path by path / read everything at 2 places / not in v1
- **Decision:** Follow Actual path by path. The register enters and shows amounts at 2 places, budget cells use the currency's places, and Veritas never rescales a stored integer. Fred had recommended not supporting them in v1; Ryan chose parity.
- **Date:** 2026-11-12

### D64. A hand-entered transaction when a rule can't run
- **Options:** save the typed row, skip rule effects / don't save
- **Decision:** Save the row exactly as typed, apply no rule effects, and mark it "may need Actual". D60's "write nothing" covers what rules would have written, never what the user typed.
- **Date:** 2026-11-12

### D65. Editing a bill
- **Options:** keep Veritas's behavior / match Actual's editor
- **Decision:** The next date resets only when the bill's date or account changes. Actual's editor resets it on every save (its stored conditions use `acct`/`description` while the server checks `account`), which drops a missed occurrence and undoes a skip; Veritas doesn't copy that. The stored rule is identical either way.
- **Date:** 2026-11-12

### D66. Spent and closed goals
- **Options:** Fred's recommendations / keep suggesting refills / always move later budget at close
- **Decision:** A goal that's reached and then spent reads "Reached → Spent", with no refill suggestion or ETA until it's reopened or re-targeted. Closing a goal shows any budget in later months, with an unchecked "Also move those to {destination}"; if left unchecked, those months carry a "closed goal has budget" flag (GOAL-10).
- **Date:** 2026-11-12

### D67. What-if events dated in the past
- **Options:** keep and flag / drop on load / ignore silently
- **Decision:** Keep them, leave them out of the projection, and show them as "in the past" so the user can delete or re-date them.
- **Date:** 2026-11-12

### D68. Where the backup schedule lives
- **Options:** on this Mac / in the budget file
- **Decision:** On this Mac, per budget, next to the security-scoped bookmark for its backup folder. Another device sets its own.
- **Date:** 2026-11-12

### D69. Unbuilt Settings sections
- **Options:** hide until built / keep the placeholders
- **Decision:** Settings shows only sections that work (Backups, Actual, Appearance). AI and Tags appear once they do something.
- **Date:** 2026-11-12

### D70. Shell copy and layout
- **Options:** approve / approve with changes / rework
- **Decision:** Ryan approved the Settings › Backups, Integrity report (findings, clean, refused import) and Open budget copy and layout as shown in the 2026-11-12 screenshots, along with the plain integrity-finding names.
- **Date:** 2026-11-12

### D71. Re-targeting a reached goal
- **Options:** a new goal row / the same goal goes back to active / refuse
- **Decision:** Re-targeting sets the new target (and date) on the same goal and puts it back to `active`. Its first `reached_month` is kept as history, so Wrapped counts the goal once, in the year it was first reached. This amends R-GOAL-3: a re-target is the one way a reached goal returns to active. The store keeps one `veritas_goal` row per category, which is why a second row was not an option.
- **Date:** 2026-11-13

### D73. Every Actual dashboard widget is native
- **Options:** draw common widgets and send the rest to Actual / draw every widget natively
- **Decision:** Veritas draws every Actual 26.11 dashboard widget type natively, and filtered custom reports too. No widget says "Open in Actual". The widget types are summary, calendar, formula, crossover, budget analysis, balance forecast, age of money and Monte Carlo, plus the ones already drawn (net worth, cash flow, spending, custom report, markdown, Sankey). Numbers match Actual's own computation for the same widget config. Layout follows Actual's 12-column grid and sizes, so a user's dashboards carry over 1:1. Cards use Veritas's own type and colors.
- **Date:** 2026-11-13
