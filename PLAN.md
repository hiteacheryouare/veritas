# Veritas: Implementation Plan

> Drafted 2026-11-09 from Ryan's vision and an interview. Facts marked **VERIFY** have not yet been confirmed against Actual's source code. Phase 0 confirms or corrects them before anyone builds on them.
>
> **Pins:** Actual **v26.11.0** (reference checkout, `@actual-app/api`, schema manifest) · **macOS 27** minimum · private GitHub repo `veritas`.

## 0. How the fleet uses this document

- Every agent reads §0 to §7, §11 and Appendix A first. Then it reads its own workstream brief in §8.
- A workstream owns the directories listed in its brief. Changes outside those directories go through the owner, or through a contract change in `contracts/` (§6.9).
- Requirement IDs (`R-BUD-3`) are stable. Reference them in PRs, tests and commit messages.
- When this plan and Actual's actual behavior disagree, Actual's behavior wins for anything touching the file format or budget math. Fix the plan in the same PR.

## 0.1 Goal brief (paste into `/goal`)

> Build Veritas as specified in `PLAN.md`, phase by phase (§8), until every P0 acceptance criterion in §10 passes and Gate 3 is reached. Follow the roles and merge bar in §0.2. At each 🚦 gate, notify Ryan and pause only the work that depends on that gate. Everything else continues. Never merge without the merge bar. Never install a dependency outside Appendix B. Stop and ask Ryan when a decision isn't covered by this plan and isn't a pure implementation detail.

The run is **done** when Phase 3's shadow run passes and Ryan has been asked for the Gate 3 daily-drive decision. Phases 4–6 (iOS, Windows, open source) are separate future goals.

## 0.2 Fleet roles and merge bar

| Role | Who | Does | Never |
|---|---|---|---|
| **Architect** | A code writer assigned to Phase 0 and to contract changes afterward | Scaffolding, contracts, ADRs, the "Actual bump" playbook; reviews every `contract`-labeled PR | Implements workstream features itself after Phase 0 |
| **Code writers** | One per active workstream | Implement their workstream's brief on branch `ws/<id>` in their own worktree; open PRs; write unit tests alongside code | Merge their own PRs; edit outside their ownership |
| **Code reviewers** | Reviewer pool | Review every PR against this plan, `AGENTS.md`, and the requirement IDs it claims; check Actual citations (`reference/actual/` file:line) for format and math changes | Approve a PR whose CI is red or whose claimed R-IDs lack tests |
| **Coverage cowboys** | Test-writer pool | For each PR: write or verify tests that prove the claimed behavior in realistic use (journeys, differential tests against the oracle, crash and fuzz harnesses). They own WS-QA in Phase 3 | Write tests that only check mocks or framework behavior |
| **Fred** | Finance specialist | Owns the finance specs (below) and gives **required sign-off** on any PR touching money semantics. Advisory on everything else; any agent may consult him | Override Actual parity: where Actual's math is the spec, Fred advises on presentation, not on changing results |

**Merge bar for every PR into `main`:**

1. CI is green, including the oracle differential tests.
2. A code reviewer approved it.
3. A coverage cowboy added or verified tests for every requirement ID the PR claims.
4. **Fred approved it** if it carries the `finance` label.

**`finance` label (Fred sign-off required)** applies to PRs touching:

- the classification layer (R-REP-1)
- Net Profit and cash flow definitions
- the Sankey's node and flow semantics
- statements and the equity reconciliation (R-REP-7)
- tax buckets (R-REP-8)
- debt and loan modeling (R-DEBT-*, R-XFER-4)
- goal release semantics (R-GOAL-4)
- forecast and suggestion methodology (R-FC-*)
- Wrapped metric definitions

**Fred's specs** are written in Phase 0 (P0.8) and land in `docs/finance/`: `classification.md`, `statements.md`, `tax-buckets.md`, `debt.md`, `forecasting.md` and `starter-categories.md`. Every implementation in those areas cites the spec section it implements.

**Labels:** `finance` (Fred), `contract` (Architect), `file-format` (reviewer must verify against the oracle). Every GitHub PR, issue and comment starts with Ryan's AI-agent note (Appendix C).

---

# 1. Vision

Veritas is a native, fast and boringly reliable personal finance app. It does envelope budgeting the way Actual Budget does and acts as a **strict superset of Actual's data**:

- You can start a budget in Veritas, or bring an Actual budget in, use Veritas for weeks, and take it to Actual with no Actual data lost.
- It fixes Ryan's long-standing gripes:
  1. Savings goals are first-class envelopes. Closing one doesn't wreck your reports.
  2. Transfers say plainly where money went from and to.
  3. Reports answer "what did I earn vs. what did I spend" honestly, including credit cards (counted at swipe) and loans (interest = spending, principal = paying back debt).
  4. Recurring bills are a simple list, not a rules engine you have to outsmart.
  5. It feels like a native Mac app: instant, keyboard-friendly, visual. It doesn't feel like editing a spreadsheet.
- It keeps what makes Actual good, including the small things like typing `+12.50` after a budget amount to add to it.
- It adds a Sankey diagram, 3/6/10-month forecasts with what-if scenarios, next-month budget suggestions, a year-end "Wrapped", US tax summary exports, and tongue-in-cheek corporate financial statements for your household.
- Ryan's entire financial life depends on this app, so it must never lose or corrupt data.

Personal daily driver first. Open source later. Platforms: **macOS → iOS → Windows**.

## 2. Fork, clean room, or fresh?

**Fresh repo. Actual's source stays as a read-only reference, and Actual's own API serves as a test oracle.** No fork, no clean room.

- **Clean room isn't needed.** Clean-room development protects you from copyright claims on code you studied. Actual is MIT licensed, so we may read it, port algorithms from it, and even copy code, as long as we keep the MIT notice. Ported logic gets attribution in `THIRD_PARTY_NOTICES.md`.
- **A fork is the wrong shape.** Most of Actual is a React UI, and that gets thrown away. Its engine (`loot-core`) is TypeScript running in a JS runtime, and we're replacing it with Rust for speed and safety. A fork would mean deleting 90% of the code and then fighting upstream merges forever.
- **Docs alone aren't enough.** "Perfect round trip" and "same budget math" are only provable against the real implementation. The docs don't describe the schema, the edge cases of the carryover math, or the rule semantics at the needed precision.
- **What we do instead:**
  - Clone `actualbudget/actual` at a **pinned release tag** into a git-ignored `reference/actual/`. Agents read it.
  - Pin `@actual-app/api` at the same version in an isolated dev-only tool (`tools/actual-oracle/`). Tests open, mutate, compute and export budgets with Actual's real engine and diff the results against Veritas's.

Ryan doesn't need to provide a fork. Phase 0 clones the reference itself at tag **v26.11.0**, the version Ryan runs.

---

# 3. User experience

Ryan opens Veritas on his Mac. The window is open and interactive before he can blink.

- **Budget.** This month's budget is a column of envelope rows grouped by category group. Each row shows *budgeted*, *spent* and *available*, with a fill bar, so he can read it at a glance. A hero number at the top shows **To Budget**.
  - Clicking an amount turns it into a text field holding the current value. Typing `+40` and pressing Return adds 40.
  - Arrow keys move between rows. ⌘← and ⌘→ change months.
  - A "Dense" toggle collapses the view into an Actual-style compact table for when he wants it.
- **Goals.** Goals live in their own section as cards with progress rings: "New laptop: $1,120 of $1,500, on track for March".
  - When he buys the laptop, he categorizes the purchase to the goal. The goal shows *Reached → Spent*.
  - He clicks **Close goal**. Veritas offers to release the leftover $20 to General Savings as a *budget move*. That is not a transaction, so no history is re-filed.
  - The goal disappears from the budget but stays in reports, Wrapped and the goal archive.
- **Accounts.** Every transfer row in a register reads like a sentence:
  - `→ To Savings`, or `← From Checking`, with a direction glyph.
  - **Move money** opens one sheet: *From* [Checking ▾, $2,140] → *To* [Car Loan ▾, −$9,800]. Both balances are visible before and after.
  - When the destination is a loan, Veritas asks how much is principal and how much is interest. It pre-fills from the loan's last payment.
- **Quick entry.** Since Ryan enters everything by hand, ⌘N opens a quick-entry sheet anywhere in the app.
  - Typing a payee fills in the category and amount from that payee's history.
  - Return saves. Tab walks the fields. Every amount field accepts arithmetic.
- **Bills.** A plain list: *Rent: $1,650, 1st of month, Checking → Housing, auto-post.* *Electric: about $90, 15th, confirm first.*
  - Auto-post bills appear on their date.
  - Confirm-first bills show up in an "Upcoming" tray with a **Paid** button that lets him tweak the amount.
  - Rules never appear in this flow.
- **Reports.** **Net Profit** shows Income − Expenses per month, with *Debt paydown* and *Savings* as separate lines underneath, and every number drills down to transactions.
  - Ryan's existing Actual custom reports and dashboard widgets render natively, and edits to them round-trip back to Actual.
  - A Sankey diagram shows income flowing into category groups, categories, savings, debt paydown and unallocated money.
- **Insights.** A forecast chart shows net worth and account balances 3, 6, 10 and 12 months out, with a confidence band.
  - He adds a what-if ("+$300/mo rent from June", "$2,000 vacation in August") and compares it with the baseline. Scenarios never change real data.
  - Next month's suggested budget explains itself: "Groceries $430, 6-month median".
  - In December, **Wrapped** plays a card story of his year.
  - **Statements** produces a Balance Sheet, Income Statement, Statement of Owner's Equity and Cash Flow statement for "Ryan Holdings, LLC (Unaudited. Obviously.)" as a PDF.
  - **Tax summary** exports the year's tax-tagged categories as PDF and CSV for TurboTax or an accountant.
- **Optional on-device AI.** Off by default. When enabled, he can ask "how much did I spend on takeout this summer vs last?" and Wrapped gets narrated. Nothing leaves the Mac.
- **Backups.** Without thinking about it, Ryan gets rolling local snapshots, plus an automatic Actual-format `.zip` written to his OneDrive folder on a schedule. That replaces his manual backup ritual. **Export to Actual** is one menu item.

---

# 4. Requirements

Priority: **P0** = required for the first daily-driver release (v1, end of Phase 3). **P1** = in v1 if on schedule, otherwise the first update. **P2** = later. Ryan asked for every "v1 extra" in v1, so they are P0. The phase plan makes sure the foundations land first.

## 4.1 Interop with Actual (R-IO)

| ID | Requirement | P |
|---|---|---|
| R-IO-0 | **Veritas stands on its own.** Create a new budget without Actual: a name, currency and number format, accounts with starting balances, and the Veritas starter categories (docs/finance/starter-categories.md). The file is an Actual-format budget from the first write, built from the pinned template (Appendix A), so exporting it imports into Actual cleanly. | P0 |
| R-IO-1 | Import an Actual export `.zip`: `db.sqlite` + `metadata.json`, either at the zip root or both in exactly one shared subfolder (same rule as Actual's `importBuffer`). | P0 |
| R-IO-2 | Export an Actual `.zip` that Actual (Electron, pinned version) imports with no warnings or errors. | P0 |
| R-IO-3 | **Lossless round trip of Actual data.** Actual → Veritas → Actual with no edits produces a canonical dump (§6.4) identical to the original for every Actual table except `kvcache`/`kvcache_key`. Metadata ids may differ. | P0 |
| R-IO-4 | Edits made in Veritas to data Actual understands look correct in Actual: budgets, transactions, transfers, splits, categories, goals as categories, bills as schedules, rules, custom reports and dashboard layout. | P0 |
| R-IO-5 | Data Veritas doesn't model (unknown tables, unknown columns, unknown rule actions, unknown widget types, templates) passes through byte-identical. | P0 |
| R-IO-6 | Veritas-only data lives in `veritas_*` tables inside the same SQLite file. Ryan accepted that Actual may drop it. In practice it survives, because Actual exports the raw db file. When re-importing, Veritas reconciles dangling references (for example, a goal whose category was deleted in Actual) without crashing. | P0 |
| R-IO-7 | Version guard. Veritas compares the file's `__migrations__` ids to its supported manifest. A newer file opens **read-only** with a clear explanation. An older file is told to upgrade in Actual first. | P0 |
| R-IO-8 | Veritas never modifies the source zip or any Actual app data. Importing always copies. | P0 |
| R-IO-9 | "Import from Actual on this Mac" reads the Electron app's budget directory directly, read-only, and refuses while Actual is running. | P2 |

## 4.2 Budget (R-BUD)

| ID | Requirement | P |
|---|---|---|
| R-BUD-1 | Envelope budgeting with math **identical** to Actual's envelope ("zero"/rollover) budget: To Budget, carryover, overspending rules, hold-for-next-month, income categories. Proven by differential tests against the oracle. | P0 |
| R-BUD-2 | Tracking-budget ("reflect") files are preserved losslessly. In v1 they open read-only with a notice. Ryan uses envelope budgeting. | P0 |
| R-BUD-3 | **Every amount field** accepts arithmetic with Actual's semantics (`+ - * / ^ ( )`, separators, currency symbols; see Appendix A). Editing a field shows the current value as text, so appending `+12.34` adds to it. | P0 |
| R-BUD-4 | Move money between envelopes, and "cover overspending" from a chosen envelope. | P0 |
| R-BUD-5 | Category and group create, rename, reorder and hide. **Deleting a category that has history is replaced by Archive**, which hides it. A real delete with re-filing is available only behind an explicit, scary confirmation. | P0 |
| R-BUD-6 | Category notes preserved and editable. Actual's budget templates and automations are preserved untouched. Veritas doesn't execute them in v1. | P0 |
| R-BUD-7 | Dense, Actual-like table mode toggle. | P1 |

## 4.3 Goals (R-GOAL)

| ID | Requirement | P |
|---|---|---|
| R-GOAL-1 | Create a goal with name, target amount, optional target date and optional icon/color. Veritas suggests a monthly contribution: (target − available) ÷ months left. | P0 |
| R-GOAL-2 | A goal is an envelope: a real Actual category in a "Goals" group. Its progress is that envelope's available balance. | P0 |
| R-GOAL-3 | Lifecycle: `active → reached → closed`. Spending on the goal means categorizing the purchase to the goal's category. A re-target returns a reached goal to active (D71). | P0 |
| R-GOAL-4 | **Close goal** releases any leftover (positive or negative) to an envelope Ryan picks, as a budget move in the current month, never as a transaction. It then hides the category. It never deletes, merges or re-categorizes anything. | P0 |
| R-GOAL-5 | Closed goals stay visible in all reports, Sankey, Wrapped and a Goal Archive, with their spend history ("New laptop: $1,480 spent, closed Mar 2027"). | P0 |
| R-GOAL-6 | Goals survive a round trip. In Actual a goal is a normal (possibly hidden) category. Actual can hold the target amount natively (a `#goal <amount>` note line, `goal_def` with `template_settings.source = 'ui'`, or per-month `zero_budgets.goal`/`long_goal = 1` rows, which are the only thing its display reads), but it has no target date, and it shows goal progress only behind the experimental `goalTemplatesEnabled` flag. The date lives in `veritas_*`. Whether Veritas also writes the target natively is an open question for Ryan (see docs/actual-format.md). | P0 |
| R-GOAL-7 | Recurring sinking-fund goals that auto-restart. Ryan didn't select this. | P2 |

## 4.4 Accounts, transactions, transfers, debt (R-ACC, R-XFER, R-DEBT)

| ID | Requirement | P |
|---|---|---|
| R-ACC-1 | Accounts: on/off budget, closed, account groups, reorder, notes. Balances: cleared, uncleared, total. | P0 |
| R-ACC-2 | Register: add, edit, delete, duplicate. Payee, category, notes, tags, cleared. Sorting and filtering. Splits are editable and stay lossless (Ryan rarely uses them, but the loan flow does). | P0 |
| R-ACC-3 | Reconcile: enter the statement balance, check off cleared items, lock as reconciled, optional adjustment transaction. Matches Actual's `cleared`/`reconciled` semantics. | P0 |
| R-ACC-4 | Quick entry (⌘N) with payee memory: last category, last amount and `learn_categories` behavior. | P0 |
| R-ACC-5 | Payees: rename, merge (maintaining `payee_mapping` like Actual), favorite. | P0 |
| R-ACC-6 | Global quick-add hotkey and a menu-bar extra. | P2 |
| R-XFER-1 | One **Move money** flow. *From* and *To* pickers show live balances and the projected balance after the move. Direction is explicit in the UI copy. | P0 |
| R-XFER-2 | In registers, transfer rows show `→ To <Account>` (outflow) or `← From <Account>` (inflow) with a glyph. A bare "Transfer: X" payee label never appears. | P0 |
| R-XFER-3 | Editing either side of a transfer edits the pair atomically (amount, date, accounts). Deleting one side deletes both. Actual itself doesn't sync dates (only amount, account and notes; date sync is an opt-in client preference, `sync-transfer-date`). Veritas does sync them and leaves that preference untouched (see docs/actual-format.md). | P0 |
| R-XFER-4 | An on-budget → off-budget transfer makes Veritas ask: *Loan/debt payment* (→ R-DEBT-2), *Saving/investing*, or *Other* (pick a category). The answer maps to the category on the on-budget side. Actual doesn't require that category (only its close-account dialog does) and shows the row as uncategorized without one; Veritas still asks (see docs/actual-format.md). | P0 |
| R-XFER-5 | A consistent glossary throughout the app (see §5.4). | P0 |
| R-DEBT-1 | Loan account setup (off-budget) with optional metadata in `veritas_*`: rate, payment amount, payment day, lender. | P0 |
| R-DEBT-2 | Loan payment wizard. One payment from checking = a split: principal as a transfer to the loan account (category kind *debt principal*) + interest as an expense. Pre-fills from the last payment. Actual supports transfers inside split children: the principal child gets a standalone counterpart in the loan account, so no fallback is needed. In Actual that on→off child keeps its category and counts as activity in that category's budget (see docs/actual-format.md). | P0 |
| R-DEBT-3 | Credit cards stay **on-budget**, Ryan's current setup. Purchases are spending at swipe. Card payments are on→on transfers and never count as spending. | P0 |

## 4.5 Bills and rules (R-BILL, R-RULE)

| ID | Requirement | P |
|---|---|---|
| R-BILL-1 | Bills screen: name, payee, amount (exact, or "about" for estimates), account, category, repeat (monthly on day N, every N weeks/months, yearly, custom), weekend handling, end condition. | P0 |
| R-BILL-2 | **Per bill**: auto-post or confirm-first. Maps to Actual's `schedules.posts_transaction`. | P0 |
| R-BILL-3 | Upcoming tray and timeline. **Paid** posts the transaction with an editable amount and date. Skip one occurrence. Pause or end. | P0 |
| R-BILL-4 | Bills are stored as Actual schedules plus their owned rule, so they round-trip natively. Veritas validates inputs so it's impossible to build a schedule that Actual would mis-evaluate. Rules are never shown in the bill flow. | P0 |
| R-BILL-5 | Income can be a "bill" too (paychecks), which feeds the forecast. | P0 |
| R-RULE-1 | Rules run with Actual's semantics (stages pre/default/post, then ascending specificity and id within a stage, chained; conditions op; actions) at the same trigger points as Actual. Actual runs rules on manually added transactions, in its client, on new rows only and filling only empty fields; it never runs them when an existing transaction is edited (see docs/actual-format.md). | P0 |
| R-RULE-2 | Friendly rule editor in sentence form: *When payee contains "AMZN" → rename payee to Amazon, set category Shopping.* "Apply to existing" shows a preview first. | P0 |
| R-RULE-3 | Rules with conditions or actions Veritas doesn't support (for example, Handlebars templating) are shown read-only with "Edit in Actual" and preserved byte-identical. | P0 |

## 4.6 Reports (R-REP)

| ID | Requirement | P |
|---|---|---|
| R-REP-1 | **Classification layer.** Every transaction (or split child) resolves to exactly one kind: `income`, `expense`, `internal_transfer`, `debt_principal`, `savings_contribution`, `starting_balance`, `adjustment`. It comes from the category's kind (`veritas_category_meta`), the transfer status and on/off-budget. All reports use this layer, none re-derives it. | P0 |
| R-REP-2 | **Net Profit**: Income − Expenses (interest included) per period. Below it: Debt paydown (principal), Savings contributions, and Net cash flow = Net Profit − paydown − savings. Refunds reduce expenses, they don't add income. Drilldown everywhere. | P0 |
| R-REP-3 | Hidden and archived categories are included in every historical report and labeled as such. | P0 |
| R-REP-4 | Native rendering of Ryan's existing Actual **custom reports** and **dashboard widgets** (known types). Edits are saved back in Actual's JSON format. Unknown widget types show an "Open in Actual" placeholder and are preserved. | P0 |
| R-REP-5 | Standard reports: Net worth, Cash flow, Spending by category, Income vs expense. | P0 |
| R-REP-6 | **Sankey**: income sources → total income → {category groups → categories, Savings, Debt paydown, Unallocated}. Month, quarter, year or custom range. Click a node to filter transactions. | P0 |
| R-REP-7 | **Statements**: Balance Sheet (assets vs liabilities from account balances), Income Statement (= Net Profit in accountant format), Statement of Owner's Equity (opening net worth + net income ± non-cash changes, such as off-budget asset revaluation = closing net worth, always reconciled to the cent), and Cash Flow Statement. PDF export. A configurable entity name and an "Unaudited. Obviously." footer. | P0 |
| R-REP-8 | **US tax summary**: tag categories (and optionally single transactions, via tags) with tax buckets such as charitable, medical, education, HSA, W-2 income, interest income and business. Year-end PDF + CSV with totals and transaction detail. | P0 |

## 4.7 Forecasting and insights (R-FC, R-WRAP, R-AI)

| ID | Requirement | P |
|---|---|---|
| R-FC-1 | Project per-account balances and net worth at 3, 6, 10 and 12 months. Inputs: bills and income schedules (known), plus a per-category baseline for discretionary spending (trailing median; seasonal adjustment once there are ≥13 months of history). Show a confidence band. | P0 |
| R-FC-2 | **What-if scenarios**: named overlays made of one-time events, recurring changes, % change to a category, income changes and goal contributions. Compare up to 3 against the baseline. Stored in `veritas_*`. They **never** modify real data. | P0 |
| R-FC-3 | Next-month budget suggestions per category, each with a one-line reason. One click applies them, and that is undoable. | P0 |
| R-FC-4 | Goal ETA from the current contribution pace. | P0 |
| R-WRAP-1 | Wrapped for any completed year (and the current year to date): total in and out, top categories and payees, biggest month, no-spend streaks, goals completed, net-worth change, and fun comparisons. Card-by-card presentation. | P0 |
| R-WRAP-2 | Export cards as images, with a "hide amounts" redaction mode that shows percentages instead. | P0 |
| R-AI-1 | Optional, **off by default**, on-device only (Apple Foundation Models on macOS). Natural-language Q&A over aggregates, Wrapped narration, plain-English forecast explanations. Read-only: it never mutates data. The app is fully functional without it. Nothing leaves the device. | P0 |

## 4.8 Reliability (R-REL)

| ID | Requirement | P |
|---|---|---|
| R-REL-1 | Every user action is exactly one SQLite transaction. A crash or power loss loses at most the in-flight action, and the file is always valid. | P0 |
| R-REL-2 | `PRAGMA integrity_check` plus an **invariant audit** on open and before export. The audit covers transfer pairs consistent, split children summing to parent, no live rows referencing missing rows, budget months consistent, and mapping tables acyclic. Violations are reported and never auto-"fixed" silently. | P0 |
| R-REL-3 | Automatic local snapshots: on open, before import/export/version changes/bulk edits/rule apply, and hourly while dirty. Retention is configurable (default 48 hourly, 30 daily, 12 monthly). One-click restore. | P0 |
| R-REL-4 | Scheduled Actual-format `.zip` backup to a user-chosen folder (Ryan's OneDrive folder). The write is atomic (temp file then rename). Failures alert the user and never block editing. | P0 |
| R-REL-5 | Unlimited undo/redo per session, covering every command. | P0 |
| R-REL-6 | Single-writer lock per budget, so a second window or instance on the same file gets read-only access. | P0 |
| R-REL-7 | The Rust core never panics across the FFI boundary. All failures are typed errors, and the UI shows actionable messages. | P0 |

## 4.9 Performance (R-PERF)

Measured in CI benchmarks on Apple Silicon against the 100k-transaction synthetic fixture.

| ID | Budget |
|---|---|
| R-PERF-1 | Cold launch → interactive budget screen ≤ 400 ms |
| R-PERF-2 | Budget cell edit → recompute → UI updated ≤ 16 ms (one frame) |
| R-PERF-3 | Register scrolling at display refresh rate (120 Hz ProMotion), no hitches |
| R-PERF-4 | Typical report ≤ 100 ms; year-long Sankey or forecast ≤ 500 ms |
| R-PERF-5 | Import or export of a 100k-transaction Actual zip ≤ 3 s |

---

# 5. Design & behavior

## 5.1 Direction: native but opinionated

- **Native:** real macOS controls, sidebar, toolbar, menus, system keyboard shortcuts, Dark Mode, Dynamic Type where it applies, accessibility labels, and standard window restoration.
- **Opinionated:**
  - Envelopes are rows with fill bars, not grid cells.
  - Goals are cards with rings.
  - Money uses semantic color: available, low, overspent, income. Red/green is never the only signal; icons and labels back it up.
  - Motion is short and functional: around 150 ms, and it respects Reduce Motion.
- **Keyboard-first.** Every primary action has a shortcut. ⌘K is a command palette for "go to account", "new bill", "move money".

## 5.2 Information architecture

Sidebar:

- **Budget**
- **Accounts** (grouped, with balances)
- **Bills**
- **Goals**
- **Reports** (dashboard, Net Profit, Sankey, custom reports)
- **Insights** (Forecast, What-ifs, Wrapped, Statements, Tax)
- Settings: backups, Actual interop, AI, appearance

## 5.3 Design gate

Before Phase 2 UI work, WS-DESIGN delivers mockups of these screens for Ryan to review and edit: Budget, Register + Move money, Bills, Goal card + close flow, Net Profit, Sankey, Forecast/what-if, one Wrapped card, and a Statement. **No Phase 2 UI work starts without Ryan's sign-off.** Ryan has strong opinions here. Ask him, don't guess.

## 5.4 Glossary (use these words, everywhere, exactly)

| Term | Meaning | Never say |
|---|---|---|
| **Move money** (between accounts) | Paired transfer between two accounts | "Transfer: Savings" as a payee |
| **Move budget** (between envelopes) | Reassigning budgeted dollars between categories; not a transaction | "transfer" |
| **To / From** | Direction relative to the account you're viewing | "Payee: Checking" |
| **Envelope** | Category in the budget UI | |
| **Goal** | Envelope with a target, lifecycle and archive | "long-term template" |
| **Bill** | Recurring expected transaction (Actual: schedule) | "schedule", "rule" in the bills flow |
| **Archive** | Hide without touching history | "delete" for categories with history |
| **Debt paydown** | Principal portion of a debt payment | "spending" |

---

# 6. Technical approach

## 6.1 Architecture

```
 macOS app (SwiftUI + AppKit)      iOS app (SwiftUI)        Windows app (WinUI 3 / C#)
 ├─ VeritasMacUI (views)           ├─ VeritasiOSUI          ├─ Veritas.Windows
 └─ VeritasKit (view models, ─────────── shared ─┘          └─ Veritas.Core (C# bindings)
    Swift bindings, formatters)
                │  UniFFI (generated)                              │ UniFFI-C# (generated)
 ───────────────┴──────────────────────────────────────────────────┴──────────
 Rust core (cargo workspace)
   veritas-ffi        the only public surface: commands, queries, event stream
   veritas-session    open/close, command bus, undo/redo, snapshots, locking, events
   veritas-engine     envelope budget math, arithmetic evaluator, goals, move budget
   veritas-rules      rules engine, schedule recurrence, bill posting
   veritas-reports    classification, Net Profit, Sankey, custom reports, statements, tax
   veritas-forecast   projections, scenarios, suggestions, Wrapped aggregates
   veritas-store      Actual-schema adapter + veritas_* extension tables + invariant audit
   veritas-interop    zip import/export, migration manifest, canonical dump
   veritas-domain     Money(i64 minor units), CivilDate, typed ids, kinds, errors
 ─────────────────────────────────────────────────────────────────────────────
 SQLite (bundled via rusqlite: same SQLite version on every platform)
```

Dependency direction is strictly downward. `veritas-domain` depends on nothing internal. UI code never sees SQL or Actual column names.

## 6.2 Storage: the Actual file *is* the native format

This is the key decision. Veritas's working file is an **Actual-schema SQLite database** (pinned version) plus additive `veritas_*` tables, stored per budget in a directory that mirrors Actual's layout:

```
~/Library/Application Support/Veritas/budgets/<id>/
  db.sqlite
  metadata.json
  snapshots/
```

Why:

- **Round trip by construction.** Actual data never gets translated into another model and back. What we don't model, we never touch (R-IO-3, R-IO-5).
- **Export is trivial and safe:** `VACUUM INTO` a temp file, empty `kvcache`/`kvcache_key`, set `journal_mode=DELETE`, zip it with `metadata.json`, rename it atomically.
- **Reversibility.** Ryan's backups stay Actual-compatible forever. If Veritas dies, Actual opens the data.
- **The research confirms it works.** Actual's import writes the db bytes as they are (then clears the kvcache), and its export writes a SQLite backup copy with `kvcache`/`kvcache_key` emptied. Extra tables and columns survive both directions (see docs/actual-format.md).

The cost is that we inherit Actual's quirks: `transactions.description` holds the payee id, mapping tables, tombstones. We also inherit its schema churn of about one migration a month. Both are contained:

- **`veritas-store` is the only module that knows physical Actual names.** It maps them to clean `veritas-domain` types. Everything above it speaks domain.
- **Writes are partial.** `UPDATE` touches only the columns being changed. Never use `INSERT OR REPLACE` or rewrite a whole row, because that would wipe columns we don't know about. Inserts write explicit defaults matching Actual.
- **Deletes follow Actual.** They set `tombstone = 1`. Rows are never hard-deleted.
- **Side tables are maintained like Actual maintains them:** `payee_mapping`, `category_mapping`, transfer payees (one per account), `starting_balance_flag`, `sort_order` conventions.
- **`v_*` views are left alone.** Veritas queries base tables. Actual rebuilds the views itself.
- **`veritas_*` schema** is versioned in its own `veritas_meta` table. Veritas never writes into `__migrations__`, so it can't trip Actual's migration validity check.
- **CRDT.** Actual writes `messages_crdt` only when the device has a sync server configured, and Veritas never writes it. Veritas leaves `messages_clock` and `messages_crdt` untouched, and export sets `resetClock: true` like Actual does. Being a live peer of an actual-server is a **non-goal** (§11).

## 6.3 Migration manifest and Actual version bumps

- `veritas-interop/manifest/actual-<version>.json` lists the exact `__migrations__` ids Veritas's writer supports, plus the Actual release tag that the reference checkout and the oracle are pinned to.
- On open:
  - ids are exactly equal → read/write.
  - The file is missing ids → "Open this budget in Actual ≥ vX first, then export again."
  - The file has unknown ids → read-only plus an explanation (R-IO-7).
- An **"Actual bump" playbook** (`docs/actual-bump.md`, written in Phase 0) covers moving to a new Actual version: update the pinned tag and the oracle version, diff the new migrations, update the `veritas-store` mappings, regenerate fixtures, and run the full differential suite. Any agent can execute it later.

## 6.4 Proving parity: oracle + canonical dump

`tools/actual-oracle/` is a dev-only Node tool with its own `package.json`, using `@actual-app/api` pinned to the exact manifest version. It is never shipped. It exposes CLI commands:

- `dump <zip>` → canonical JSON: every table, rows sorted by id, columns sorted, `kvcache*` excluded, metadata ids normalized. It also includes computed values per month via the API (to-budget, per-category budgeted/spent/balance/carryover) and schedule next dates.
- `apply <zip> <ops.json>` → applies a scripted operation list with Actual's engine and exports.
- `generate <spec>` → builds the synthetic fixtures.

Rust has a matching `canonical_dump` in `veritas-interop`. That gives three test families:

1. **Round trip:** `dump(export(import(zip))) == dump(zip)` for every fixture.
2. **Differential behavior:** a random op sequence, generated with proptest, is applied by Veritas *and* by the oracle. Then the budget values, balances and schedule dates must be equal. This is how R-BUD-1 and R-RULE-1 get proven, not just argued.
3. **Actual acceptance:** the oracle imports every Veritas export with `importBudget`, and `checkDatabaseValidity` passes. That includes budgets created in Veritas (R-IO-0), whose balances and budgets are compared with what the oracle reads back.

Fixtures:

- `fixtures/synthetic/`, committed: `small`, `all-features` (every table populated, splits, transfers on↔off, hidden categories, custom reports, dashboard, rules with unsupported actions), and `perf-100k`.
- `fixtures/private/`, **git-ignored**: Ryan's real export. Ryan approved using it locally; it is never committed, uploaded, pasted into PRs or issues, or quoted in logs. Only aggregate pass/fail results leave the machine.

## 6.5 Runtime model: fast by design

- **Load everything.** On open, load the entire budget into an in-memory domain model. Personal data is small: 100k transactions is tens of MB. SQLite is the durable store, and memory is the query engine.
- **Write path.** A command is validated, applied to memory and to SQLite in one transaction, recorded as an inverse for undo, and emits change events. If SQLite fails, memory rolls back.
- **Incremental budget sheet.** An edit in month M invalidates months M through the last month. Results are memoized per (month, category).
- **Coarse FFI calls.** The UI asks for whole screen models ("budget month 2027-03") rather than doing chatty per-cell calls.
- **Event-driven UI.** The core pushes a "changed ids" stream, and view models refetch only what changed.
- **Off the main thread.** Reports and forecasts run on a background queue in the core and are cancellable when the user navigates away.

## 6.6 Reliability engineering

- SQLite: WAL mode, `synchronous=FULL`, `foreign_keys` off (Actual's schema doesn't declare them; the invariant audit replaces them), and a busy timeout. Use bundled SQLite.
- The audit (R-REL-2) lives in `veritas-store`. It runs on open, before export, and in every test.
- Snapshots use `VACUUM INTO`. Retention pruning never deletes the newest snapshot of each tier.
- Lint gate: `clippy::unwrap_used`, `expect_used` and `panic` are denied in non-test core code. The FFI layer wraps everything in `catch_unwind` → `CoreError::Internal`.
- **Crash tests:** a harness kills the process (SIGKILL) at random points during scripted edits, reopens, and asserts integrity, the audit and "last committed action present".
- **Fuzzing:** a `cargo fuzz` target on the zip importer and the arithmetic parser (hostile input never panics).
- Import works on a copy, audits it, and refuses to adopt the budget if the audit finds corruption. The user gets a report instead.
- Creating a budget (R-IO-0) builds it in a temporary folder and renames it into place, so a crash never leaves a half-made budget. The crash harness covers create too.

## 6.7 UI technology

- **macOS:**
  - SwiftUI with the Observation framework for screens.
  - **AppKit `NSTableView`** (wrapped) for the register and the dense budget view, where SwiftUI `List` can't hit R-PERF-3.
  - Swift Charts for standard charts. A custom `Canvas` for the Sankey and the statement layouts.
  - `ImageRenderer`/PDFKit for PDF and image export.
  - Foundation Models framework for R-AI.
  - Minimum **macOS 27** (Ryan wants the bleeding edge). Build with the matching Xcode; feel free to use any macOS 27-only SwiftUI/AppKit API.
- **Project structure for fleet-friendliness.** All Swift code lives in **local Swift packages**: `VeritasKit` (shared with iOS) and `VeritasMacUI`. The checked-in `.xcodeproj` is a thin app shell that almost never changes. That means parallel agents don't collide in `project.pbxproj`, and no extra project-generator tools are needed.
- **Windows (Phase 5):** WinUI 3 + C# on .NET with self-contained deployment, so there's no runtime install for the user. Bindings via UniFFI's C# generator (**VERIFY** maturity at that time; fallback `csbindgen`). C# is generally discouraged in Ryan's stack, but it is the native Windows UI path. The core stays Rust, so C# is UI-only.
- **iOS (Phase 4):** reuses `VeritasKit` and the Rust core unchanged.

## 6.8 Build and tooling (project-scoped, nothing global beyond Xcode + rustup + Node)

- Cargo workspace with `rust-toolchain.toml` pinning the toolchain.
- **`cargo xtask`** for every task runner need: `xtask bindings`, `xtask oracle <cmd>`, `xtask fixtures`, `xtask template`, `xtask bench`, `xtask ci`. No `just`, `make` or Homebrew tools to install.
- `.nvmrc` + `tools/actual-oracle/package-lock.json` for the oracle. Run it with `npm ci` inside that folder only.
- GitHub Actions (macOS runner): fmt, clippy, Rust tests, oracle differential tests, `swift test` for the packages, `xcodebuild` for the app, benchmarks with a regression threshold (fail at >15% over the budget in R-PERF).

## 6.9 Contracts (frozen at phase boundaries)

`contracts/` holds:

- the FFI surface definition (UniFFI proc-macro types re-exported in `veritas-ffi`, documented in `contracts/ffi.md`)
- the canonical dump schema
- the `veritas_*` schema v1
- the error taxonomy

Changing a contract takes a PR labeled `contract`, reviewed by the Architect workstream, that updates every consumer.

---

# 7. Repository layout (affected areas)

```
veritas/
  AGENTS.md                      seed in Appendix C
  THIRD_PARTY_NOTICES.md         MIT notice for ported Actual logic
  rust-toolchain.toml
  Cargo.toml                     workspace
  crates/
    veritas-domain/  veritas-store/  veritas-interop/  veritas-engine/
    veritas-rules/   veritas-reports/  veritas-forecast/  veritas-session/  veritas-ffi/
  xtask/
  contracts/
  apps/
    macos/  Veritas.xcodeproj (thin shell)  Packages/VeritasKit  Packages/VeritasMacUI
    ios/    (Phase 4)
    windows/ (Phase 5)
  tools/actual-oracle/           dev-only Node oracle (pinned @actual-app/api)
  fixtures/synthetic/            committed
  fixtures/private/              git-ignored
  reference/actual/              git-ignored, pinned clone
  docs/
    actual-format.md             verified spec of the Actual file format
    actual-bump.md
    adr/
  design/                        mockups from WS-DESIGN
```

---

# 8. Implementation steps: phases & workstreams

Each workstream runs on its own branch (`ws/<id>`), in its own git worktree, and lands via PR to `main`. Workstreams inside a phase run in parallel. **Gates** are where the fleet stops for Ryan.

## Phase 0: Foundations & truth-finding (serial, 1 Architect agent)

*Why first:* every other workstream depends on knowing exactly what Actual does, and on having the contracts and the oracle.

| Step | What | Where | Done when |
|---|---|---|---|
| P0.1 | Create the **private** GitHub repo `veritas`. Scaffold, toolchain pins, `xtask`, CI skeleton, seed `AGENTS.md`, `.gitignore`. Labels `finance`, `contract`, `file-format`; branch protection on `main` that enforces the merge bar (§0.2) | root | CI is green on an empty workspace; a test PR can't merge without review |
| P0.2 | Clone Actual at tag **v26.11.0** into `reference/actual/` (read-only) | `reference/` | `xtask reference` reproduces it |
| P0.3 | Build the oracle with `@actual-app/api@26.11.0` exactly (approved, Appendix B): `dump`, `apply`, `generate`; confirm the API works offline and headless | `tools/actual-oracle/` | `dump` on a fresh budget works with no server |
| P0.4 | **VERIFY spikes** → write `docs/actual-format.md`, each claim citing reference file + line (see the checklist below) | `docs/` | Every VERIFY in this plan resolved or corrected |
| P0.5 | Contracts v0: domain types, FFI surface skeleton, canonical dump schema, `veritas_*` v1 schema, error taxonomy | `contracts/`, `veritas-domain`, `veritas-ffi` | Compiles; Swift bindings generate |
| P0.6 | Fixtures: `small`, `all-features`, `perf-100k` via the oracle | `fixtures/synthetic/` | Committed; dumps reproducible |
| P0.7 | ADRs for every §11 decision; `docs/actual-bump.md` | `docs/adr/` | Written |
| P0.8 | **Fred** writes the finance specs in `docs/finance/` (§0.2): classification rules per transaction kind, Net Profit/cash flow definitions, statement layouts and the equity reconciliation, tax buckets, debt/loan modeling (split-child transfers, verified in P0.4), forecasting methodology and error metrics, and Wrapped metric definitions | `docs/finance/` | Each spec has worked examples with exact numbers that cowboys can turn into tests |

**P0.4 verification checklist:**

- amount encoding (integer minor units?)
- date encoding (`YYYYMMDD` int?) and month encoding
- `goal_def` / long-term goal format and how Actual displays it
- whether rules run on manually added transactions
- whether a split child can be a transfer
- what Actual does on category delete (re-files transactions? Likely the root cause of Ryan's goal workaround mess)
- where the "click the amount, append `+x`" behavior lives in the UI layer (the evaluator itself rejects a leading `+`)
- schedule recurrence JSON (frequency, interval, patterns, skip-weekend, end modes) and how `schedules_next_date` is maintained
- custom report and dashboard widget JSON schemas plus the list of widget types
- envelope carryover, overspending and hold semantics
- `zero_budget_months` existence
- transfer category rules on↔off budget, and how Actual creates transfer payees
- `metadata.json` fields Actual requires on import

**🚦 Gate 0 (Ryan):** review the `docs/actual-format.md` highlights (especially any VERIFY that came out differently than this plan assumed) and Fred's finance specs.
- **Waits on the gate:** WS-STORE, WS-ENGINE, WS-RULES and WS-REPORTS beyond scaffolding.
- **Keeps going:** WS-DESIGN, WS-SESSION scaffolding, oracle and fixture hardening.

## Phase 1: Core engine (parallel) + design (parallel)

### WS-STORE: Actual schema adapter + interop
- **Owns:** `veritas-store`, `veritas-interop`.
- **Depends on:** P0.
- **Steps:**
  1. Read and write mappings for every Actual table Veritas models.
  2. Passthrough guarantees.
  3. Tombstones and mapping maintenance.
  4. Transfer payee lifecycle.
  5. `veritas_*` tables + `veritas_meta`.
  6. Invariant audit.
  7. Zip import (copy → audit → adopt) and export (`VACUUM INTO` → zip → atomic rename).
  8. Manifest guard.
  9. Rust `canonical_dump`.
  10. New budget (R-IO-0), including `cargo xtask template`, which WS-STORE owns: copy the pinned template db (Appendix A), write `metadata.json`, apply the starter categories (docs/finance/starter-categories.md), and add accounts with starting balances. It's built in a temp folder and renamed into place.
- **Done when:** round-trip tests pass on every fixture (R-IO-1 to R-IO-8), a new budget's export imports into Actual through the oracle with no errors and the same balances and budgets, the oracle imports every export cleanly, and the importer fuzz target has run 1 h with no crash.

### WS-ENGINE: budget math
- **Owns:** `veritas-engine`.
- **Depends on:** P0, WS-STORE domain loaders (it can start against fixture-loaded domain structs).
- **Steps:**
  1. Port the arithmetic evaluator with attribution, using Actual's semantics, and import Actual's test vectors.
  2. Envelope budget sheet: to-budget, carryover, overspend, hold, income.
  3. Incremental recompute.
  4. Move budget / cover overspending.
  5. Goal math (progress, suggestion, ETA, close with release).
  6. Category kinds.
- **Done when:** the differential proptest against the oracle (≥10k random op sequences) passes, and R-PERF-2 is met in a benchmark.

### WS-RULES: rules, schedules, bills
- **Owns:** `veritas-rules`.
- **Steps:**
  1. Rule condition and action evaluation (supported set; unknowns flagged as preserved).
  2. Stage ordering.
  3. Trigger points per P0.4.
  4. Schedule recurrence and next-date maintenance identical to Actual.
  5. Bill model ↔ schedule+rule mapping.
  6. Posting (auto / confirm), skip.
- **Done when:** differential tests on schedule next dates and rule outcomes pass, and the bill → schedule → bill round trip is identical.

### WS-SESSION: session, FFI, reliability
- **Owns:** `veritas-session`, `veritas-ffi`, crash harness.
- **Steps:**
  1. Create (R-IO-0), open and close, single-writer lock.
  2. Command bus with one transaction per command.
  3. Undo/redo inverses.
  4. Change-event stream.
  5. Snapshots + retention + restore.
  6. Scheduled Actual-zip backup to a folder.
  7. `catch_unwind` boundary.
  8. UniFFI exports + `xtask bindings` → `VeritasKit`.
  9. Crash-kill harness.
- **Done when:** the crash harness runs 1,000 kills with zero integrity, audit or lost-commit failures, and R-PERF-1 (core portion) and R-PERF-5 are met.

### WS-REPORTS: report data
- **Owns:** `veritas-reports`.
- **Steps:**
  1. Classification layer (R-REP-1).
  2. Net Profit.
  3. Net worth, cash flow, spending.
  4. Custom report evaluator for Actual's report JSON (groupBy, balance types, date ranges, conditions).
  5. Dashboard layout model.
  6. Sankey graph data.
  7. Statements with the equity reconciliation always balancing.
  8. Tax buckets + summary.
- **Done when:** values for custom reports in `all-features` match what Actual computes (oracle or hand-verified goldens), statements balance on all fixtures, and R-PERF-4 is met.

### WS-FORECAST: projections & insights data
- **Owns:** `veritas-forecast`.
- **Steps:**
  1. Baselines (median, seasonality ≥13 months).
  2. Projection with confidence band.
  3. Scenario overlay model.
  4. Next-month suggestions with reasons.
  5. Goal ETA.
  6. Wrapped aggregates.
- **Done when:** a backtest on synthetic history reports error metrics in `docs/forecast-eval.md`, and scenarios provably never write non-`veritas_scenario*` tables (test).

### WS-DESIGN: design language & mockups
- **Owns:** `design/`.
- **Depends on:** nothing (it starts with P0).
- **Steps:**
  1. Design tokens (type, color incl. semantic money colors, spacing, motion).
  2. Mockups of the §5.3 screens in light and dark.
  3. Interaction notes for keyboard flows and arithmetic fields.
- **Ask Ryan** about direction early, with 2–3 contrasting options, before polishing.

**🚦 Gate 1 (Ryan):** approve the mockups. In parallel, the Architect freezes contracts v1.
- **Waits on the gate:** all Phase 2 UI work.
- **Keeps going:** every Phase 1 core workstream.

## Phase 2: macOS app (parallel, after Gate 1)

Shared rule: views go in `VeritasMacUI`, view models in `VeritasKit`, and no business logic in Swift. If a number on screen needs computing, it comes from the core.

| Workstream | Builds | Key requirements |
|---|---|---|
| **WS-MAC-SHELL** | App shell, sidebar, toolbar, menus, ⌘K palette, new budget/open/import/export, settings (backups, interop, AI, appearance), snapshot restore UI, read-only mode banners, error presentation | R-IO-*, R-REL-3/4/6, §5.2 |
| **WS-MAC-BUDGET** | Budget screen, envelope rows, arithmetic field component (**reused by every amount field app-wide**: build it first and publish it to the others), move budget, cover overspending, dense mode, goal cards + create/close/archive flows | R-BUD-*, R-GOAL-* |
| **WS-MAC-ACCOUNTS** | Account list, `NSTableView` register, quick entry ⌘N with payee memory, Move money sheet, transfer rendering, on→off "what is this?" prompt, loan setup + payment wizard, reconcile flow, payee manager, account close (Actual's flow: an optional closing transfer moves the balance to the chosen account; see docs/actual-format.md) | R-ACC-*, R-XFER-*, R-DEBT-* |
| **WS-MAC-BILLS** | Bills list/editor, upcoming tray, Paid/skip, sentence-style rule editor, read-only unsupported rules | R-BILL-*, R-RULE-* |
| **WS-MAC-REPORTS** | Dashboard (Actual widgets, native), Net Profit, standard reports, custom report viewer/editor, Sankey canvas, drilldowns | R-REP-1 to 6 |
| **WS-MAC-INSIGHTS** | Forecast + what-if compare, budget suggestions, Wrapped story + image export, Statements + PDF, Tax summary export, on-device AI panel | R-FC-*, R-WRAP-*, R-AI-1, R-REP-7/8 |

**Done when:** each workstream's acceptance criteria in §10 pass in UI tests (XCUITest for the critical journeys), and R-PERF-1 and R-PERF-3 are met end to end.

## Phase 3: Hardening & cutover (v1)

- **WS-QA** (owned by the coverage cowboys; Fred reviews the finance journeys):
  - End-to-end journeys: a new budget from scratch, a month of manual entry, closing a goal, paying a loan, a bill cycle, a round trip.
  - Fuzz the remaining parsers.
  - Run the perf suite on `perf-100k`.
  - Accessibility pass.
- **Shadow run (Ryan + agent):**
  1. Import Ryan's real export (private fixture).
  2. Compare the canonical dumps and every month's budget numbers against the oracle.
  3. Use Veritas for 2 weeks alongside Actual.
  4. Export, import into the Actual Electron app (Ryan does this manually), and spot-check.
- **Cutover checklist** in `docs/cutover.md`.

**🚦 Gate 3 (Ryan):** daily-drive decision. The `/goal` run ends here (§0.1).

## Phase 4: iOS · Phase 5: Windows · Phase 6: Open-source prep (outline only; plan them in detail when Phase 3 ends)

- **iOS:** an iPhone UI on `VeritasKit`. Quick entry is the hero. File handoff via the Files app or iCloud Drive (sync is out of scope until planned).
- **Windows:** WinUI 3 app, C# bindings, MSIX packaging.
- **OSS:** license choice (MIT is the natural fit with Actual), notices audit, README, signing + notarization, update channel.

---

# 9. Edge cases (prioritized)

**Data-destructive / correctness**
1. Unknown columns on a row Veritas edits → partial `UPDATE` only. There's a test with an injected extra column.
2. A file from a newer or older Actual → read-only / upgrade-first (R-IO-7).
3. Live rows referencing tombstoned rows (a transaction in a deleted category, a merged payee) → resolve through the mapping tables like Actual does, and render "(deleted) X".
4. A transfer whose pair is missing or tombstoned (possible in the wild) → the audit flags it, the UI shows it as a one-sided transfer, no crash, no auto-fix.
5. Changing a transfer's destination from an on-budget to an off-budget account (Actual keeps the category but doesn't require it; Veritas asks, per R-XFER-4) and back (Actual clears the category automatically).
6. A split parent whose children don't sum to it (corrupt input) → audit flag. Editing re-balances only with confirmation.
7. A goal category deleted or merged in Actual between round trips → the goal becomes "orphaned" with a cleanup prompt.
8. Closing a goal with a *negative* leftover (overspent) → the release pulls from the chosen envelope, and the copy explains it.
9. Disk full or I/O error mid-command → rollback, memory restored, alert, nothing partial.
10. The OneDrive folder is missing, offline or mid-sync → atomic temp+rename, retry later, alert after N failures, local snapshots unaffected.
11. A second window or instance on the same budget → read-only (R-REL-6).

**Math & dates**
12. Arithmetic: divide by zero, `1,234.56+10`, `(5`, `^` right-associativity, negative results, pasted currency symbols. Behavior matches Actual exactly. Actual's invalid-input fallback depends on the field: transaction amounts keep the old value, budget cells save 0. Which rule Veritas follows is an open question for Ryan (see docs/actual-format.md).
13. Bills on the 29th, 30th or 31st, Feb 29, and skip-weekend crossing a month boundary.
14. "Today" = the local civil date, recomputed on wake or midnight. Time zones never shift transaction dates.
15. Refunds in expense categories reduce expenses. A refund larger than the month's spending gives negative expense, not income.
16. Starting-balance transactions never count as income in any report.
17. Hidden categories with activity appear in historical reports (R-REP-3).
18. Closed accounts with a non-zero balance still count in the balance sheet and net worth.

**Scale & UX**
19. 100k+ transactions, 15+ years of months, 500 categories: still within the perf budgets.
20. First launch with no budget: two equal choices, **New budget** and **Import from Actual**. New budget offers to create off-budget 401(k) and HSA accounts (D56). The new-budget screens get Ryan's design sign-off (§5.3) before they merge.

---

# 10. Acceptance criteria

**Interop & safety**
- [ ] First launch shows New budget and Import from Actual as equal choices, and a UI test walks both to an open budget.
- [ ] A budget created in Veritas (accounts with starting balances, starter categories, a month of budgeting and transactions) exports to a zip that Actual imports with no errors, and Actual shows the same balances and budgets.
- [ ] Ryan exports from Actual, imports into Veritas, and immediately exports. The oracle reports a canonical dump identical to the original.
- [ ] After a scripted session in Veritas (budget edits, transfers, a split loan payment, a new bill, a closed goal, an edited custom report), the export imports into Actual with no errors, and Actual shows the same balances, budgets, schedules and report config.
- [ ] A file with an injected unknown table and an unknown column survives import → edit → export unchanged in those parts.
- [ ] A file from a newer Actual opens read-only with an explanation, and Veritas can't modify it.
- [ ] 1,000 random SIGKILLs during edits produce zero integrity or audit failures, and the last committed action is always present.
- [ ] Every risky operation creates a snapshot first, and restoring a snapshot brings back the exact prior state.
- [ ] The scheduled backup produces a valid Actual zip in the chosen folder. Removing the folder causes an alert and no data loss.

**Budget & goals**
- [ ] For every fixture and 10k random op sequences, Veritas's to-budget and per-category budgeted/spent/available match the oracle's for every month.
- [ ] Clicking a budgeted amount of `100.00`, typing `+12.34` and pressing Return results in `112.34`. The same works in transaction amount fields.
- [ ] Creating a $1,500 goal due in 6 months suggests $250/mo (from $0). Categorizing the purchase moves it to *reached/spent*. Closing it releases the leftover via a budget move. No transaction is created or re-categorized (verified by a diff of `transactions`), the category is hidden, and the goal still appears in last year's Net Profit and Wrapped.
- [ ] Deleting a category with history defaults to Archive and doesn't change any transaction.

**Transfers, debt, reports**
- [ ] Every transfer row in every register shows `→ To X` or `← From X`. No register shows a raw transfer-payee label.
- [ ] A $400 loan payment entered via the wizard as $320 principal + $80 interest shows in Net Profit as an $80 expense and a $320 debt paydown. In Actual it appears as a valid categorized split.
- [ ] A $50 grocery purchase on the on-budget card counts as $50 groceries spending in the month of the swipe. The card payment never appears as spending.
- [ ] The Sankey for a month sums exactly to that month's income. Every outflow node drills to the matching transactions.
- [ ] Ryan's existing Actual custom reports and dashboard render in Veritas with the same numbers Actual shows.
- [ ] The Balance Sheet balances, and opening equity + net income ± adjustments = closing equity to the cent, on every fixture.
- [ ] The tax CSV totals per bucket equal the sum of the tagged transactions for the year.

**Bills, forecast, insights**
- [ ] An auto-post bill posts on its date. A confirm-first bill appears in Upcoming, and **Paid** posts it with an edited amount. Both round-trip to Actual as schedules with the correct `posts_transaction`.
- [ ] Ryan never sees the word "rule" while creating or editing a bill.
- [ ] Forecasts at 3, 6, 10 and 12 months render with confidence bands. Adding a scenario changes only `veritas_scenario*` tables (verified by dump diff).
- [ ] Wrapped renders for any full year in the data and exports a redacted image with no dollar amounts.
- [ ] With AI disabled, nothing AI-related loads. With it enabled, the app makes no network calls at all (verified with a network-blocking test).

**Performance:** all R-PERF budgets pass in CI on `perf-100k`.

---

# 11. Assumptions & decisions

| # | Decision | Source |
|---|---|---|
| D1 | Fresh repo; Actual as a pinned read-only reference + oracle. No fork, no clean room. | Recommendation (§2) |
| D2 | Rust core shared by all UIs via UniFFI. | Ryan |
| D3 | Working file = Actual-schema SQLite + `veritas_*` tables. | Recommendation (§6.2) |
| D4 | Lossless for Actual data. Veritas-only extras may be lost if Actual drops them; in practice they ride along. | Ryan |
| D5 | Interop = file import/export. Live sync with actual-server is a **non-goal**; Ryan's server plans are ~10 years out. CRDT tables are untouched. | Ryan |
| D6 | Envelope budgeting only in the v1 UI. Tracking budgets are preserved and read-only. | Assumption (Ryan uses envelopes) |
| D7 | Spending is counted at swipe. Loan interest = expense, principal = debt paydown line. Card payments are never spending. | Ryan |
| D8 | Credit cards stay on-budget and loans off-budget, matching Ryan's current setup. No forced migration. | Ryan |
| D9 | Goal close = budget move + hide. It never re-files transactions. Category delete defaults to Archive. | Ryan + root-cause hypothesis, confirmed: deleting a category with a transfer target re-maps all its history and budgets to the target (see docs/actual-format.md) |
| D10 | Bills = Actual schedules, with a per-bill auto/confirm choice. | Ryan |
| D11 | AI is optional, off by default, on-device only, read-only. | Ryan |
| D12 | "Native but opinionated" UI. SwiftUI + AppKit tables. Minimum macOS 27. | Ryan |
| D13 | US tax = summary export, not form-line mapping or liability estimates. | Ryan |
| D14 | Bank sync is out of scope (Ryan enters everything manually). Bank-sync columns are preserved untouched. | Ryan |
| D15 | Actual templates and automations are preserved but not executed in v1. | Assumption |
| D16 | Platform order: macOS → iOS → Windows. | Ryan |
| D17 | Built for Ryan first and structured for open source later. MIT attribution kept from day one. | Ryan |
| D18 | All Swift code lives in local packages; the thin `.xcodeproj` avoids fleet merge conflicts. | Engineering |
| D19 | `cargo xtask` as the task runner; no globally installed build tools. | Ryan's isolation values |
| D20 | Name: **Veritas**. Pins: Actual v26.11.0, macOS 27, private GitHub repo. | Ryan |
| D21 | Fleet = Architect, code writers, code reviewers, coverage cowboys, Fred (§0.2). | Ryan |
| D22 | Merge bar = CI green + reviewer + cowboy tests, and Fred too for `finance` PRs. | Ryan |
| D23 | Gates pause only the work that depends on them. | Ryan |
| D24 | Real budget data is allowed locally in `fixtures/private/` and never leaves the machine. | Ryan |

# 12. Open questions

None blocking. Ryan resolved everything on 2026-11-09:
- name: Veritas
- Actual version: v26.11.0
- real-data fixtures: allowed locally
- minimum OS: macOS 27
- dependencies: approved
- repo: private GitHub
- gates pause only dependent work
- merge bar: reviewer + cowboy + CI
- Fred: required sign-off on finance

New questions found during the build go to Ryan as they come up (§0.1). Agents don't guess.

---

# Appendix A: Actual internals cheat sheet

Verified against tag v26.11.0 (sha 1766241e3f4b) in P0.4. This is a summary. [docs/actual-format.md](docs/actual-format.md) has the details, the file:line citations and the Architect's decisions.

- **New budgets** copy `default-db.sqlite`, which already holds seed rows with fixed ids (3 category groups, 7 categories including "Starting Balances", 7 identity `category_mapping` rows), and run every migration on first load, which adds 12 dashboard widgets on one `Main` page. → Veritas creates budgets from a pinned template db that the oracle generates (`cargo xtask template`), committed in `crates/veritas-store/template/` because the oracle is dev-only. A test fails when the template's `__migrations__` don't match the manifest. The starter categories reuse the 3 seed groups and 7 seed categories by id (renamed and moved with plain updates, nothing deleted; "Starting Balances" is kept exactly) and add the rest as new rows (docs/finance/starter-categories.md).
- **Migrations:** exactly **60** (57 up to `ADDITIVE_ONLY_CUTOFF = 1780606215001`, 3 after). `checkDatabaseValidity` fails a file with `out-of-sync-migrations` when it has applied ids but no migrations exist, has an unknown id ≤ the cutoff, has any unknown id while missing a known one, has a post-cutoff id while missing a pre-cutoff one, or its pre-cutoff ids aren't an exact ordered prefix. Otherwise missing ids are applied on load. → **Veritas never writes `__migrations__`.**
- **Export** (`exportBuffer`): a zip of exactly `db.sqlite` + `metadata.json` at the root. The db is a SQLite **backup copy** with `kvcache`/`kvcache_key` emptied, not the raw file. `metadata.json` is copied from disk with `resetClock: true`. No export at all when `budgetName` is missing.
- **metadata.json** (`MetadataPrefs`): `budgetName, id, lastUploaded, cloudFileId, groupId, encryptKeyId, lastSyncedTimestamp, resetClock, lastScheduleRun, userId`. Import requires none of them, only valid JSON.
- **Import** (`importBuffer`): both files at the root or in exactly one shared subfolder, zip safety checks on every entry. New local id; `cloudFileId` is **removed** (not nulled); `groupId` and `encryptKeyId` become null; `lastUploaded` = today; everything else carries over. The db bytes are written verbatim, then `kvcache`/`kvcache_key` are emptied. Extra tables and columns survive.
- **Load** runs migrations, rebuilds `v_*` views only when `__meta__.view-hash` differs, applies `resetClock` (new CRDT node id), rewrites `metadata.json` (`userId`, `resetClock: false`, `lastScheduleRun`), and auto-posts due schedules once a day, **even for a local-only budget**.
- **Encodings:** amounts INTEGER cents (×100 for every currency), outflow negative; dates INTEGER `YYYYMMDD`; budget months INTEGER `YYYYMM`; `zero_budget_months.id` the string `'YYYY-MM'`; booleans 0/1.
- **Key tables** (full DDL in docs/actual-format.md):
  - `transactions`: `acct`, `category`, `amount`, `description` (= payee id), `notes`, `date`, `financial_id`, `imported_description`, `transferred_id`, `isParent`, `isChild`, `parent_id`, `error` (split-imbalance JSON), `cleared` (default **1**; desktop new rows write 0), `reconciled`, `schedule`, `starting_balance_flag`, `sort_order` (REAL), `tombstone`, `pending`, `raw_synced_data`, legacy `type`/`location`. `amount` and `sort_order` (`Date.now()` ms) have no SQL default; Actual's model layer fills them. Parents always read `category = NULL`.
  - `accounts`: `offbudget`, `closed`, `sort_order`, `type`, `last_reconciled`, `account_group_id`, bank-sync columns.
  - `categories`: `is_income`, `hidden`, `cat_group`, `goal_def`, `cleanup_def`, `template_settings` (default `{"source": "notes"}`).
  - `payees`: `transfer_acct` (one transfer payee per account, `name ''`, display name from the account), `favorite`, `learn_categories`, legacy `category`.
  - `payee_mapping` and `category_mapping`: no tombstone; every read resolves ids through them.
  - `rules`: `stage` (`'pre'`, NULL, `'post'`), `conditions_op`, `conditions`/`actions` JSON with **internal field names** (`acct`, `description`, `imported_description`).
  - `schedules`: `rule` (nullable in SQL, required by the model), `active` (unused), `completed`, `posts_transaction`, `name`, `custom_upcoming_length`, `sort_order`. No `next_date` column: it comes from `schedules_next_date` (local/base pair). `schedules_json_paths` is derived; Actual rebuilds it on load.
  - `zero_budgets` / `reflect_budgets`: `month`, `category`, `amount`, `carryover`, `goal`, `long_goal`. Two id formats, so match rows by (month, category). No tombstone.
  - `zero_budget_months`: `buffered` ("hold for next month").
  - Also: `notes`, `preferences` (`budgetType` missing or anything but `'tracking'` = envelope), `custom_reports`, `dashboard` (14 widget types), `dashboard_pages`, `tags`, `payee_locations`, `account_groups`, `banks`, `transaction_filters`, `cleanup_groups`, `messages_crdt`, `messages_clock`, `messages_pending`, `kvcache`, `kvcache_key`, `__meta__`, `__migrations__`; unused legacy `created_budgets` and `pending_transactions`.
- **Soft delete:** `tombstone = 1`. Only the transaction views filter on it; `v_categories` and `v_payees` don't.
- **CRDT:** `messages_crdt` is written whenever a sync server URL is configured on the device, even for a budget never uploaded. `messages_clock` is written on first load and on `resetClock`, even local-only.
- **Rules** run in Actual's client on new manual rows only, filling empty fields. Order: stage pre → default → post, then ascending specificity, then id.
- **Arithmetic** (`loot-core/src/shared/arithmetic.ts` `evalArithmetic`):
  - `+ - * /` and right-associative `^`, plus parentheses. Unary minus on numbers only. All whitespace is stripped first.
  - Number tokens may include `, . ' ’`, NBSP, U+202F, space and currency symbols. Trailing junk is ignored. Invalid input → default value; `1/0` → `Infinity`.
  - A *leading* `+` doesn't parse. The "append to the current value" behavior comes from the field selecting all its text on focus, plus a rule that turns a lone typed operator into `<old text><op>`.
  - Invalid input in a field: transaction amounts keep the old value, budget cells save 0.
- **Rounding:** ports use JS `Math.round` (halves toward +∞), never Rust's `f64::round`.
- **`@actual-app/api`** (`packages/api`, v26.11.0, Node ≥ 22): `init`, `importBudget(path|buffer, {type:'actual'})`, `loadBudget`, `exportBudget()` → zip bytes, `aqlQuery`, CRUD for everything, budget month values (its `forNextMonth` is the manual hold only). It appears usable offline and headless (**VERIFY** in P0.3).

# Appendix B: Dependencies (approved by Ryan, 2026-11-09)

| Dependency | Why | Backing |
|---|---|---|
| `rusqlite` (bundled SQLite) | Storage; same SQLite on every platform | Mature, widely used |
| `uniffi` | Rust → Swift (and later C#) bindings | Mozilla |
| `zip` | Actual export/import | Mature |
| `serde`, `serde_json` | JSON columns, metadata, dumps | De facto standard |
| `uuid` | Actual uses UUIDv4 ids | Mature |
| `chrono` | Civil dates | Mature |
| `thiserror` | Typed errors | dtolnay |
| `regex` | Actual rule `matches` conditions (approved 2026-11-10, D47) | rust-lang |
| `proptest`, `insta`, `criterion`, `cargo-fuzz` (dev only) | Differential/property tests, snapshots, benchmarks, fuzzing | Mature |
| `@actual-app/api` (dev only, pinned, `tools/actual-oracle`) | Test oracle | Actual project |
| Apple frameworks: SwiftUI, AppKit, Swift Charts, PDFKit, Foundation Models | UI, charts, PDF, on-device AI | System, no install |

No third-party Swift packages are planned. Anything not on this list needs Ryan's approval first.

# Appendix C: Seed `AGENTS.md` for the new repo

```markdown
# Veritas — agent rules

- Read PLAN.md §0–§7 and your workstream brief before touching code.
- Work only in your workstream's directories, on branch ws/<id>, in your own worktree. Delete the worktree and branch after merge.
- Actual's behavior is the spec for anything touching the file format or budget math. Check reference/actual/ and the oracle; never guess. Cite file:line in PRs.
- veritas-store is the only code that knows Actual column names. Never INSERT OR REPLACE an Actual row. Never hard-delete an Actual row. Never write __migrations__.
- No unwrap/expect/panic in non-test core code. All errors are typed.
- No business logic in Swift. Numbers on screen come from the core.
- Every behavior change ships with a test, and parity changes ship with a differential test against the oracle.
- No new dependencies without Ryan's approval. No dev servers unless asked.
- Comments only for genuinely non-obvious code. No prose walls.
- CSS (design mockups): no BEM.
- Ported Actual logic gets an entry in THIRD_PARTY_NOTICES.md.
- Merge bar: CI green + code reviewer approval + coverage-cowboy tests for every claimed R-ID. PRs labeled `finance` also need Fred. `contract` PRs need the Architect.
- Unsure about money semantics (what counts as spending, income, debt, equity, tax)? Ask Fred before writing code, and cite docs/finance/ in the PR.
- fixtures/private/ is Ryan's real money. Never commit it, upload it, or quote it in PRs, issues or logs.
- At a 🚦 gate: notify Ryan, pause only work that depends on the gate, continue the rest.
- Anything on GitHub is prefixed with:
  > [!NOTE]
  > This content was created by an AI agent acting on behalf of Ryan.
```


# Notes from Ryan:

- Dont design any logos or anything, dont write the readme, or AGENTS.md to the repo. I'll handle the marketing and everything later. USe placeholders for now if you absolutely have to.
- Look at other repos on this machine for my preferences concerning UI, code style, project struture, etc. I have a lot of opinions, and I want to keep the codebase consistent with my other projects.