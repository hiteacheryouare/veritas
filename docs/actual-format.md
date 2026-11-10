# Actual file format and behavior (v26.11.0)

This is the verified spec behind PLAN step P0.4. It was read from the pinned reference checkout `reference/actual/` (tag v26.11.0, sha 1766241e3f4b). Every claim cites `reference/actual/<path>:<line>`. Where this file and PLAN.md disagree, this file wins (PLAN §0) and PLAN.md is corrected.

**Citation conventions**

- `lc/` = `reference/actual/packages/loot-core/src/`
- `mig/` = `reference/actual/packages/loot-core/migrations/`
- `dc/` = `reference/actual/packages/desktop-client/src/`
- Any other path is written out from `reference/actual/`.
- A bare `:N` cites the file cited last in the same bullet, row or paragraph.
- Inside the verbatim tables and code blocks, a bare migration id or migration filename is that file in `mig/`, `init.sql` is `lc/server/sql/init.sql`, and short filenames resolve through the key below.
- **[unverified]** marks a claim that was inferred from code but not run, or that depends on the external `@rschedule/core@1.5.0` library (pinned at `reference/actual/yarn.lock:7692-7701`, not vendored in the reference). The oracle (P0.3) must confirm each one.
- **[test]** marks a claim that an upstream unit test pins.
- Amounts are integer minor units (cents) unless stated. Month strings are `YYYY-MM`, DB months are the int `YYYYMM`, and dates are the int `YYYYMMDD`.

**Short filename key**

| Short name | Path |
|---|---|
| `actions.ts`, `base.ts`, `envelope.ts`, `goal-template.ts`, `goal-template.pegjs`, `category-template-context.ts`, `template-notes.ts`, `statements.ts` | `lc/server/budget/<name>` |
| `sheet.ts` | `lc/server/sheet.ts` |
| `spreadsheet.ts`, `globals.ts` | `lc/server/spreadsheet/<name>` |
| `arithmetic.ts`, `util.ts`, `months.ts`, `tags.ts` | `lc/shared/<name>` |
| `shared/rules.ts`, `shared/schedules.ts` | `lc/shared/rules.ts`, `lc/shared/schedules.ts` |
| `schedules.test.ts` | `lc/shared/schedules.test.ts` |
| `types/models/rule.ts` | `lc/types/models/rule.ts` |
| `dashboard.ts` (in the widget section) | `lc/types/models/dashboard.ts` (the default dashboard is `lc/shared/dashboard.ts`) |
| `condition.ts`, `action.ts`, `rule.ts`, `rule-utils.ts`, `index.test.ts` | `lc/server/rules/<name>` |
| `transaction-rules.ts`, `transfer.ts` | `lc/server/transactions/<name>` |
| `schema/index.ts`, and bare `index.ts` in the budget section | `lc/server/aql/schema/index.ts` |
| `compiler.ts` | `lc/server/aql/compiler.ts` |
| `db/index.ts` | `lc/server/db/index.ts` |
| `sync/index.ts` | `lc/server/sync/index.ts` |
| `sync.ts` | `lc/server/accounts/sync.ts` |
| `api.ts`, `cloud-storage.ts` | `lc/server/<name>` |
| `budgetfiles/app.ts`, `budget/app.ts`, `schedules/app.ts`, `reports/app.ts`, `dashboard/app.ts` | `lc/server/<dir>/app.ts` |
| `occurrences.test.ts` | `lc/server/schedules/occurrences.test.ts` |
| `utils.ts` | `dc/components/transactions/table/utils.ts` |
| `table.tsx` | `dc/components/table.tsx` |
| `useFormat.ts` | `dc/hooks/useFormat.ts` |
| `ReportOptions.ts`, `getLiveRange.ts`, `reportRanges.ts`, `disabledList.ts`, `Overview.tsx`, `ChooseGraph.tsx` | `dc/components/reports/<name>` |
| `RuleEditor.tsx` | `dc/components/rules/RuleEditor.tsx` |
| `ScheduleEditModal.tsx` | `dc/components/schedules/ScheduleEditModal.tsx` |
| `TransactionsTable.tsx`, `TransactionList.tsx`, `applyRulesToTransaction.ts` | `dc/components/transactions/<name>` |
| `TransactionEdit.tsx` | `dc/components/mobile/transactions/TransactionEdit.tsx` |

---

## 1. Highlights for Ryan (Gate 0)

1. **Your goal-workaround theory was right.** Deleting a category and choosing "transfer to" another category doesn't rewrite any transaction, but from then on Actual reads every past transaction and every past month's budget of the deleted category as the target's. History gets silently re-filed. Hiding a category (Archive) is the only non-destructive option.
2. **Loan payments work as one split.** A split child can be a transfer, so "principal → loan account, interest → expense" is a single native split and the two-transaction fallback is gone. One catch: in Actual the principal child keeps its category and counts as spending in that category's budget.
3. **Actual doesn't require a category on on-budget → off-budget transfers.** Without one, the row just shows up as "uncategorized". Only Actual's close-account dialog insists. Veritas will still ask "what is this?".
4. **Actual doesn't keep the two sides of a transfer in sync for date, cleared or reconciled.** It syncs amount, account and notes, and the source side's notes overwrite the other side's. Veritas will sync the date too. Deleting one side deletes the other, except when the other side is a split child: then Actual unlinks it instead.
5. **Rules do run on manual entry, but only in Actual's desktop UI, only on new transactions, and only to fill empty fields.** Editing an existing transaction never runs rules. Within a stage, rules run from least to most specific, and each one sees the changes of the ones before it. Veritas has to port all of this into the core.
6. **Goals barely exist natively in Actual.** The target amount can be written as a `#goal` line (or the matching per-month budget columns), but Actual only shows goal progress behind an experimental feature flag, and it has no target date at all. Whether Veritas writes the target anyway is your call (open question 1).
7. **Bad input in an amount field behaves differently per field.** Transaction amounts keep the old value. Budget cells save 0. Open question 2.
8. **The `+12.50` trick works through a select-all plus a one-character rule.** Clicking a budget cell selects its text. Typing a lone `+` turns the text back into `100.00+`, and you keep typing. In a budget cell a bare `+12.34` means 12.34.
9. **Actual has no "last amount" or "last category" memory.** It learns categories by writing a rule once 3 of a payee's last 5 transactions share a category. Remembering the last amount is a Veritas addition (open question 3).
10. **New budgets aren't empty and need exactly 60 migrations.** Actual seeds 7 categories (including "Starting Balances"), 12 dashboard widgets and a "Main" dashboard page, and its migration check is strict. Veritas will create new budgets from a pinned template that Actual itself produced.
11. **Exports aren't the raw file, and Actual rewrites files on open.** The zipped db is a SQLite backup copy with the caches cleared, and opening a budget rewrites `metadata.json`. Even a local-only Actual auto-posts due bills once a day when you open it.
12. **Budget math details that bite.** Hidden categories count fully. Only the first income group feeds To Budget. The months Actual computes depend on today's date. The "roll over overspending" flag is stored per month and silently lapses about 12 months out. Moving budget money writes a "Reassigned…" line into the month's notes.
13. **Bills on the 29th, 30th or 31st skip short months** in Actual's scheduling library [unverified], and a bill only counts as paid when the payment is dated on or after its due date (or up to 2 days before it, for a confirm-first bill with an approximate date). Missed auto-post bills are caught up one by one.
14. **Actual already ships 14 dashboard widget types,** including its own Sankey, balance forecast, age of money, Monte Carlo and formula cards. We need to decide which ones Veritas draws natively.
15. **Rounding differs between JavaScript and Rust on negative halves** (JS rounds −2.5 to −2, Rust to −3). Every ported formula uses Actual's rounding.

---

## 2. VERIFY resolutions

One row per PLAN.md **VERIFY** marker and per P0.4 checklist item (PLAN §8). Where a marker and a checklist item ask the same question, they share a row.

| Plan assumption | What Actual actually does | Citation | Impact on Veritas |
|---|---|---|---|
| **Checklist: amount encoding** (integer minor units?) | Yes. `INTEGER`, the amount ×100 (`amountToInteger = Math.round(x * 100)`). Outflow negative, inflow positive. Arithmetic is bounded to ±(2^51 − 1) by `safeNumber`. Storage is ×100 for every currency, even 0-decimal ones like JPY, because the only helper that honours `decimalPlaces` has no callers [unverified]. | `lc/shared/util.ts:413-417`, `:541-555`, `:426-441`, `:485-487`; `lc/shared/currencies.ts:50`; `dc/components/transactions/table/utils.ts:37-38` | `Money(i64)` in cents, signed like Actual. The oracle checks a JPY budget. |
| **Checklist: date and month encoding** | Dates are `INTEGER YYYYMMDD` (`toDateRepr`), also in `schedules_next_date`. `zero_budgets.month`/`reflect_budgets.month` are `INTEGER YYYYMM`. `zero_budget_months.id` is the **string** `'YYYY-MM'`. Sheet names are `budgetYYYYMM`. | `lc/server/models.ts:42-63`; `lc/server/aql/schema/index.ts:118`; `lc/server/budget/actions.ts:58-65`, `:179-191`; `lc/shared/months.ts:434-436` | Three month encodings. `veritas-store` owns the conversions. |
| **R-GOAL-6 `goal_def` (VERIFY) + checklist: long-term goal format and display** | `goal_def` holds parsed templates, but the progress UI never reads it. It reads the per-month `zero_budgets.goal` and `long_goal`, which only a template apply writes. A `#goal <amount>` note line, or `goal_def` with `template_settings.source = 'ui'`, expresses a target amount. Without one of those, Actual's next template apply sets `goal_def = NULL`. There is no target date. The display only renders behind `flags.goalTemplatesEnabled`, default off. | `lc/server/budget/goal-template.ts:52`, `:198-209`, `:340-347`; `lc/server/budget/statements.ts:6-26`; `lc/types/models/templates.ts:101-105`; `dc/components/budget/GoalStatus.tsx:36-50`; `dc/hooks/useFeatureFlag.ts:5-27` | The target date lives in `veritas_*`. Writing the target natively is open question 1 (§8). |
| **R-RULE-1 (VERIFY) + checklist: do rules run on manually added transactions?** | Yes, but in the client. Desktop runs `rules-run` on a new, unsaved row on every field edit, and merges the result only into empty fields the user hasn't cleared (notes may be wrapped by prepend/append). Edits of existing rows never run rules. The server add path never runs rules on the rows it gets, but it does run them on transfer counterparts it creates (keeping only `notes`, `cleared`, `schedule`). | `dc/components/transactions/TransactionsTable.tsx:3688-3733`, `:3729-3731`; `dc/components/transactions/TransactionList.tsx:265-294`; `dc/components/transactions/applyRulesToTransaction.ts:44-82`; `lc/server/transactions/index.ts:40-217`; `lc/server/transactions/transfer.ts:71` | Port `runRules` **and** the client merge into the core. Run on new manual rows, imports, schedule posts, transfer counterparts and reconcile adjustments, never on edits (§6.2). |
| **R-DEBT-2 (VERIFY) + checklist: can a split child be a transfer?** | **Yes.** `addTransfer` refuses only parents. A child with a transfer payee gets a standalone counterpart in the other account, with `transferred_id` set on both. If that counterpart is deleted, the child is unlinked, not deleted. An on→off child keeps its category and counts as activity in that category's budget. | `lc/server/transactions/transfer.ts:48-54`, `:103-111`; `lc/server/transactions/transfer.test.ts:172-219`; `lc/server/budget/base.ts:133-149` | The loan wizard writes one split. The two-linked-transactions fallback is dropped. Fred's debt spec must cover the principal child's budget activity. |
| **D9 (VERIFY) + checklist: category delete** | Hypothesis confirmed. No transaction row changes. With a transfer target, `category_mapping` is repointed (chains included), the target's budget becomes the sum of both for every created month, and the category is tombstoned. All history then reads as the target. Without a target, its transactions read as uncategorized. Group delete has no income/expense guard. | `lc/server/budget/app.ts:378-422`, `:471-492`; `lc/server/budget/base.ts:305-326`; `lc/server/db/index.ts:436-448`, `:538-562`; `lc/server/aql/schema/index.ts:394-397`, `:421-430` | Archive (`hidden = 1`) is the default. A real delete must reproduce the mapping and budget merge exactly. |
| **Appendix A "append to the current value" (VERIFY) + checklist: where the `+x` behavior lives** | The field selects all its text on focus. `InputValue`'s single-operator rule turns a lone `+ - * / ^` into `<original text><op>`. Budget cells also fall back to `currencyToAmount`, so a bare `+12.34` there means 12.34. Transaction fields have the rule but no fallback. | `dc/hooks/useProperFocus.tsx:25-40`; `dc/components/table.tsx:371-383`; `dc/hooks/useFormat.ts:249-280`; `dc/components/transactions/table/utils.ts:83-86` | The arithmetic field component reproduces select-all plus the single-operator rule. `100.00` then `+12.34` gives 112.34. |
| **Checklist: schedule recurrence JSON and `schedules_next_date` maintenance** | The date condition's value is a `RecurConfig` (or a plain date for one-time). `next_date` isn't a column. `v_schedules` picks `local_next_date` when `local_next_date_ts = base_next_date_ts`, else `base_next_date`. `setNextDate` writes base on reset and local otherwise. Month-end and Feb 29 behavior comes from rschedule [unverified]. | `lc/types/models/schedule.ts:5-20`; `lc/shared/schedules.ts:201-354`; `lc/server/schedules/app.ts:216-283`; `lc/server/aql/schema/index.ts:341-370` | WS-RULES ports `getNextDate`/`getNextDateAfter`/`setNextDate` and proves them against the oracle (§6.3). |
| **Checklist: custom report and widget JSON, list of widget types** | 14 widget types with typed `meta`. Report `conditions` use **public** field names (rules use internal ones). `color_scheme` and `metadata.tagScope` must pass through. `balance_type`'s DB default `'Expense'` is legacy. | `lc/server/dashboard/app.ts:31-50`; `lc/types/models/dashboard.ts:41-497`; `lc/server/reports/app.ts:47-102` | WS-REPORTS models all 14 or falls back to "Open in Actual" per type (§6.4, §6.5). |
| **Checklist: envelope carryover, overspending, hold** | A positive balance always rolls over. A negative one rolls over only if that month's `carryover` flag is set, otherwise it is charged to next month's To Budget. Hold (`buffered`) moves money from this month's To Budget into next month. A non-zero manual hold replaces the automatic income hold rather than adding to it. Negative To Budget carries forward. | `lc/server/budget/envelope.ts:48-72`, `:98-226` | Exact formulas in §5.4–§5.7. |
| **Checklist: does `zero_budget_months` exist?** | Yes: `zero_budget_months(id TEXT PK 'YYYY-MM', buffered INTEGER DEFAULT 0)`, in every file. It holds "hold for next month". No tombstone. | `mig/1632571489012_remove_cache.js:7-9`; `lc/server/budget/actions.ts:179-191` | Modeled by `veritas-store`, written by hold and reset-hold. |
| **Checklist: transfer category rules on↔off, how Actual creates transfer payees** | The server clears the category only when both accounts have the same `offbudget`. It never requires one on→off. The off-budget side never gets one. Account create inserts a payee `{name: '', transfer_acct: <account>}` plus a self-mapping. The display name comes from `accounts.name`, and a rename writes nothing to `payees`. | `lc/server/transactions/transfer.ts:24-45`; `lc/server/transactions/index.ts:115-121`; `lc/server/accounts/app.ts:557-577`; `lc/server/db/index.ts:576-586`; `lc/server/aql/schema/index.ts:314-327` | `veritas-store` writes the transfer payee and mapping on account create and never renames it. |
| **Checklist: `metadata.json` fields Actual requires on import** | None. The file must parse as JSON. Both files must sit at the zip root or in exactly one shared directory, and every entry must pass the zip safety checks. | `lc/server/cloud-storage.ts:212-254`; `lc/server/util/zip.ts:43-97` | Export writes `id`, `budgetName` and `resetClock: true` at least. `budgetName` must be present or Actual can't re-export (§3.9). |
| **§6.7 Windows: UniFFI's C# generator maturity (VERIFY)** | Not a question about Actual. | — | Stays **VERIFY** in PLAN.md until Phase 5. |
| **Appendix A: `@actual-app/api` offline and headless (VERIFY in P0.3)** | Not resolved by reading. The api build uses `default-db.sqlite` and `migrations/` like electron. | `lc/platform/server/fs/index.api.ts:18-26` | Stays **VERIFY** for P0.3. |

### Other plan statements that Actual contradicts

| Plan assumption | What Actual actually does | Citation | Impact on Veritas |
|---|---|---|---|
| **R-XFER-3**: editing either side edits the pair (amount, date, accounts); deleting one side deletes both | The server syncs `amount`, `account` (by repointing payees), `notes` (source overwrites the other side) and `schedule`. Not `date`, `cleared` or `reconciled`. Date sync is a desktop-only, opt-in synced pref `sync-transfer-date`, default off, so mismatched dates are normal in Actual files. Changing either side's payee to an ordinary payee deletes the other side. A counterpart that is a split child is unlinked (payee and `transferred_id` nulled), not deleted. | `lc/server/transactions/transfer.ts:95-138`; `lc/server/transactions/__snapshots__/transfer.test.ts.snap` ("inserted/updated/deleted 3"); `dc/components/transactions/TransactionsTable.tsx:1131-1133`, `:1286-1327`; `lc/types/prefs.ts:47` | Veritas syncs dates (decision 5). Whether "delete both" also unlinks split-child counterparts is open (§8, design calls). |
| **R-XFER-4, §9 edge 5**: Actual requires a category on on→off transfers | Not at the data layer. The server only clears the category for on↔on and off↔off. An uncategorized on-budget side shows as "uncategorized". Only the Close Account modal requires a category. Moving a transfer back to on→on clears the category on both sides automatically. | `lc/server/transactions/transfer.ts:24-45`, `:120-138`; `dc/queries/index.ts:45-55`, `:125-135`; `dc/components/modals/CloseAccountModal.tsx:33-40`, `:86-94` | Veritas still asks. Reading files from Actual, an on→off transfer without a category is valid. |
| **§9 edge 12**: invalid input → keep the old value | Only in transaction amount fields. Budget cells save 0 on digit-less input, and `FinancialInput` (hold and transfer menus) yields 0. `(5` gives 5 in budget cells but keeps the old value in transaction fields. `1/0` gives `Infinity`, which makes `setBudget` throw. | `dc/components/transactions/table/utils.ts:83-86`; `dc/components/budget/envelope/EnvelopeBudgetComponents.tsx:428-431`; `dc/components/util/FinancialInput.tsx:104` | Open question 2 (§8). |
| **R-ACC-4**: payee memory of last category and last amount | Neither exists. `learn_categories` writes or rewrites a `payee is X → set category` rule when ≥ 3 of the payee's 5 latest rows share a category. The category then arrives through rules on the next manual add. Mobile only defaults date and account from the last transaction. | `lc/server/transactions/transaction-rules.ts:932-1047`; `dc/components/mobile/transactions/TransactionEdit.tsx:1705-1712` | Category learning is implemented by writing `rules`, like Actual. "Last amount" is open question 3. |
| **R-RULE-1**: stages decide the order | Within a stage, rules run in ascending specificity score, then id. Every match chains into the next. An indexer prunes some candidates. "Apply to existing" previews use a separate SQL evaluator whose semantics differ. | `lc/server/rules/rule-utils.ts:18-110`; `lc/server/transactions/transaction-rules.ts:401-449`, `:493-753` | Port `rankRules` and the indexer quirk. Differential tests cover both evaluators (§6.1). |
| **R-RULE-3**: unsupported means Handlebars | Also HyperFormula `options.formula`, `set-split-amount` with `method: 'formula'`, and `BALANCE_OF(...)` and `{{balance}}`. `matches` (the regex is lowercased, which changes its meaning) and `set payee_name` (it creates payees as a side effect) were in this list too; both are now ported with oracle proof (D47, `crates/veritas-rules/tests/oracle_rules.rs`). | `lc/server/rules/action.ts:102-199`, `:308-401`; `lc/server/rules/condition.ts:418-427` | Templates and formulas stay read-only. A regex Veritas can't read the JS way, and a payee name two payees share, are reported as undetermined. |
| **R-BILL-1**: "monthly on day N" | On the 29th, 30th or 31st Actual skips months that lack the day [unverified]. `day: -1` means the last day of the month. `skipWeekend` throws without `weekendSolveMode`. | `lc/shared/schedules.ts:201-253`, `:340-354`; `lc/server/schedules/occurrences.test.ts:77-87` | R-BILL-4: map "end of month" to `patterns: [{type: 'day', value: -1}]`, warn on 29–31, always write `weekendSolveMode`. |
| **R-BILL-3**: Paid posts with an editable date | A schedule counts as paid only when a linked transaction is dated ≥ `next_date` (≥ `next_date − 2d` for non-auto-post `isapprox` schedules), with no upper bound. An earlier date doesn't advance it. | `lc/shared/schedules.ts:90-102`, `:165-193`; `lc/shared/schedules.test.ts:278-462` | Constrain the date or advance `schedules_next_date` with the local write (§8, design calls). |
| **R-BILL-2**: auto-post timing | Actual posts once a day on open (and on sync), gated by `metadata.json` `lastScheduleRun`, and catches up every missed occurrence. A local-only budget runs it too. | `lc/server/schedules/app.ts:680-825`, `:860-881`; `lc/server/sync/index.ts:802-897` | Keep `lastScheduleRun` consistent in exports. A rerun in Actual is harmless only if Veritas posted with the same `schedule` link and a date ≥ `next_date`. |
| **R-REP-4, R-IO-5**: a few widget types | 14 types, including `sankey-card`, `balance-forecast-card`, `age-of-money-card`, `monte-carlo-card`, `crossover-card`, `budget-analysis-card` and `formula-card`. | `lc/server/dashboard/app.ts:31-50` | Choose native vs "Open in Actual" per type (§8, design calls). R-REP-6 and R-FC-1 overlap Actual's own cards. |
| **R-BUD-1**: envelope math | Hidden categories and groups count fully. Only the first income group feeds `total-income`; a file with no income group crashes Actual's sheet. The created-month range depends on today and on the earliest transaction, tombstoned and off-budget ones included. | `lc/server/budget/base.test.ts:231`, `:283`; `lc/server/budget/envelope.ts:99`, `:116-120`; `lc/server/budget/base.ts:422-447` | The engine never filters hidden categories (only the UI does). Differential tests pin today (decision 9). |
| **R-BUD-4, R-GOAL-4**: a move is just a budget change | `transfer-category`, `cover-overspending` and `cover-overbudgeted` also append a "Reassigned…" line to `notes['budget-YYYY-MM']`. `transfer-available` doesn't. | `lc/server/budget/actions.ts:518-677`, `:723-783` | Veritas writes the same note (decision 6). |
| **§6.4 oracle dump**: per-month budget values via the API | `api/budget-month` returns `forNextMonth: value('buffered')`, the manual hold only. The UI shows `buffered-selected`. `buffered-auto` isn't exposed, and every income group's `received` is `total-income`. | `lc/server/api.ts:474`, `:502-505` | The dump computes both hold values or reads the cells through internal handlers. |
| **§9 edge 16**: starting balances are never income | True for reports. In budget math a categorized starting-balance transaction is income: it feeds `total-income` and To Budget. | `lc/server/budget/base.ts:133-149`; `lc/server/accounts/app.ts:557-594` | Fred's specs state that the budget and the reports differ here. |
| **§6.2**: export reads the raw file; local-only Actual never writes `messages_crdt` | Export zips a SQLite backup of an in-memory copy with `kvcache`/`kvcache_key` emptied. CRDT messages are logged whenever a sync server URL is configured on the device, even for a budget never uploaded. | `lc/server/cloud-storage.ts:157-177`; `lc/server/budgetfiles/app.ts:708-712`; `lc/server/sync/index.ts:73-77`, `:520-528` | Round-trip tests compare logical dumps, never bytes. `messages_crdt` passes through untouched. |
| **§6.6**: WAL mode | Actual opens the db with no WAL and never sets it. | `lc/server/db/index.ts:72-81` | The exported db uses `journal_mode=DELETE` (decision 3). |
| **Account close**: not in the plan | A non-zero balance must be moved by a closing transfer to a chosen account (category required by the UI only for on→off). `closed = 1` is written first. An account with zero transactions is tombstoned instead. Force close tombstones every transaction and orphans counterparts. | `lc/server/accounts/app.ts:596-710`; `dc/components/modals/CloseAccountModal.tsx:33-40` | WS-MAC-ACCOUNTS gets an account-close flow (§4.9). |
| **Appendix A, whole** | Corrected facts: exactly 60 migrations, not about 61; the migration guard has five rules, not one; the export db is a backup copy, not the raw file; import removes `cloudFileId` rather than nulling it and empties `kvcache*` before load; views are rebuilt only when `__meta__.view-hash` differs; CRDT logging depends on a configured server URL; `v_categories` and `v_payees` don't filter tombstones; fresh budgets carry seed rows; `schedules.rule` is required only at the AQL layer; `transactions.amount` and `sort_order` have no SQL default; `cleared` defaults to 1 (desktop new rows write 0); rule JSON uses internal field names; `schedules` has more columns and no `next_date`; `zero_budget_months` exists; 14 widget types. Missing tables and columns are listed in §3.3. | §3, §4, §6 | PLAN.md Appendix A is rewritten as a short cheat sheet pointing here. |

---

## 3. Schema and interop

### 3.1 How a fresh budget is made

- `createBudget` copies `fs.bundledDatabasePath` to `<budgetDir>/db.sqlite` (`lc/server/budgetfiles/app.ts:479`). It writes `metadata.json` as `{ id, budgetName }` (`lc/server/budgetfiles/app.ts:482-485`, `lc/server/prefs.ts:89-91`), then calls `_loadBudget` (`lc/server/budgetfiles/app.ts:488`).
- For the api and electron builds, `bundledDatabasePath` is `default-db.sqlite` and `migrationsPath` is `migrations/` (`lc/platform/server/fs/index.api.ts:18-26`).
- **`reference/actual/packages/loot-core/default-db.sqlite` is not an empty `init.sql`.** It is a binary file (inspected with `sqlite3 -readonly`). Its schema matches `lc/server/sql/init.sql:1-88`, with the same column order per table, and `__migrations__` is empty. It carries seed rows:
  - `db_version`: one row, `'0.0.1'`. The first migration drops the table.
  - `category_groups`, 3 rows:
    - `fc3825fd-b982-4b72-b768-5b30844cf832` "Usual Expenses", is_income 0, sort 16384
    - `2E1F5BDB-209B-43F9-AF2C-3CE28E380C00` "Income", is_income 1, sort 32768
    - `a137772f-cf2f-4089-9432-822d2ddc1466` "Investments and Savings", is_income 0, sort 32768
  - `categories`, 7 rows:
    - Food `541836f1-…`, General `af375fd4-…`, Bills `d4b0f075-…` and Bills (Flexible) `29ec2c58-…`, in Usual Expenses.
    - Income `3c1699a5-…` and Starting Balances `506e8d9d-…`, both is_income=1, in the Income group.
    - Savings `6bbd8472-…`, in Investments and Savings.
  - `category_mapping`: 7 identity rows (`id = transferId`), one per category.
  - **Every Actual budget starts with these same hard-coded ids.**
  - File pragmas: page_size 4096, journal_mode delete, UTF-8, user_version 0.
- `_loadBudget` then runs `updateVersion()`, which applies the migrations and then the views (`lc/server/budgetfiles/app.ts:623`, `lc/server/update.ts:34-37`).
- How the schema in §3.3 was derived: copy `default-db.sqlite`, run every `.sql` migration in id order with `sqlite3`, replay the DDL of the six `.js` migrations by hand, read back with `PRAGMA table_info`. [unverified] until P0.3 diffs it against a budget made with `@actual-app/api` `createBudget` (row data differs by uuids; the schema should not). The research copy (`build/fresh.sqlite`) is not committed. The pinned template db (decision 1) supersedes it.

### 3.2 Migration side effects that matter for the schema

In id order, each cited at the line that has the effect.

- `1548957970627` drops `db_version` (`mig/1548957970627_remove-db-version.sql:3`).
- `1550601598648` creates `payees`, `payee_rules` and `payee_mapping` (`mig/1550601598648_payees.sql:3-21`). `payee_rules` is dropped later by `1682974838138` (`mig/1682974838138_remove_payee_rules.sql:3`).
- `1555786194328` rebuilds `category_groups` without `UNIQUE(name)` (`mig/1555786194328_remove_category_group_unique.sql:14-19`).
- `1632571489012` (JS):
  - It creates `zero_budget_months`, `zero_budgets`, `reflect_budgets`, `notes`, `kvcache` and `kvcache_key` (`mig/1632571489012_remove_cache.js:6-31`).
  - It drops `spreadsheet_cells`, then runs `ANALYZE; VACUUM` (`mig/1632571489012_remove_cache.js:131-135`).
  - `ANALYZE` also runs in `1612625548236` (`mig/1612625548236_optimize.sql:5`), so `sqlite_stat1` exists in a fresh file.
- `1615745967948` creates `__meta__` and drops the old `v_*` views (`mig/1615745967948_meta.sql:3-8`).
- `1686139660866` drops `accounts.type` (`mig/1686139660866_remove_account_type.sql:3`). `1688841238000` re-adds it at the end of the table (`mig/1688841238000_add_account_type.sql:3`), so `type` is physically the 15th column.
- `1722804019000` (JS) creates `dashboard`. When no custom reports exist, it inserts the 12 widgets of `DEFAULT_DASHBOARD_STATE` with uuid ids (`mig/1722804019000_create_dashboard_table.js:13-43`, `lc/shared/dashboard.ts:3`).
- `1723665565000` (JS) creates `preferences`. It copies synced keys out of `metadata.json`; a fresh file has none (`mig/1723665565000_prefs.js:20-55`).
- `1738491452000` uses `RENAME COLUMN` and `DROP COLUMN` on `custom_reports.sort_by` (`mig/1738491452000_sorting_rename.sql:3-11`). The bundled SQLite must therefore be ≥ 3.35.
- `1754611200000` adds `categories.template_settings JSON DEFAULT '{"source": "notes"}'`. The follow-up `UPDATE … WHERE IS NULL` never matches, because `ADD COLUMN` back-fills the default. So the 7 seed categories end up with `{"source": "notes"}` (`mig/1754611200000_add_category_template_settings.sql:3-5`).
- `1759842823172` is a no-op whose id is still recorded (`mig/1759842823172_add_isGlobal_to_preferences.sql:1`).
- `1765518577215` (JS) creates `dashboard_pages`, inserts one page named `'Main'` and points every widget at it (`mig/1765518577215_multiple_dashboards.js:6-28`).
- `1788468782000` (JS) creates `messages_pending` from `PENDING_MESSAGES_TABLE_SQL` (`mig/1788468782000_add_messages_pending.js:4`, `lc/server/sync/messages-pending.ts:9-18`).

### 3.3 Final tables (as SQLite stores them after all migrations)

Notation: each column gives its type, a `NULL` or `NOT NULL` marker, and its default, followed by its source. PK means primary key. Unless marked NOT NULL, a column is nullable, including the `TEXT PRIMARY KEY` columns, since SQLite doesn't force NOT NULL on non-INTEGER PKs. SQLite has no booleans: `BOOLEAN`, `JSON` and `INT` are declared types with NUMERIC/NUMERIC/INTEGER affinity.

```sql
-- ===== Core budget data =====
CREATE TABLE accounts (                              -- init.sql:14-28
  id                 TEXT PRIMARY KEY,
  account_id         TEXT    NULL,                   -- bank-sync external id
  name               TEXT    NULL,
  balance_current    INTEGER NULL,
  balance_available  INTEGER NULL,
  balance_limit      INTEGER NULL,
  mask               TEXT    NULL,
  official_name      TEXT    NULL,
  subtype            TEXT    NULL,
  bank               TEXT    NULL,                   -- -> banks.id
  offbudget          INTEGER NULL DEFAULT 0,
  closed             INTEGER NULL DEFAULT 0,
  tombstone          INTEGER NULL DEFAULT 0,
  sort_order         REAL    NULL,                   -- 1616167010796_accounts_order.sql:3
  type               TEXT    NULL,                   -- 1688841238000_add_account_type.sql:3 (re-added)
  account_sync_source TEXT   NULL,                   -- 1704572023730:3
  last_sync          TEXT    NULL,                   -- 1739139550000_bank_sync_page.sql:3
  last_reconciled    TEXT    NULL,                   -- 1740506588539:3
  bank_sync_status   TEXT    NULL,                   -- 1780606215000:3
  account_group_id   TEXT    NULL DEFAULT NULL       -- 1787013118115_add_account_groups.sql:9
);

CREATE TABLE account_groups (                        -- 1787013118115_add_account_groups.sql:3-7
  id TEXT PRIMARY KEY, name TEXT NULL, sort_order REAL NULL, tombstone INTEGER NULL DEFAULT 0
);

CREATE TABLE banks (                                 -- init.sql:8-12
  id TEXT PRIMARY KEY, bank_id TEXT NULL, name TEXT NULL, tombstone INTEGER NULL DEFAULT 0
);

CREATE TABLE transactions (                          -- init.sql:38-56
  id                    TEXT PRIMARY KEY,            -- child ids historically "<parent>/<x>"
  isParent              INTEGER NULL DEFAULT 0,
  isChild               INTEGER NULL DEFAULT 0,
  acct                  TEXT    NULL,                -- -> accounts.id
  category              TEXT    NULL,                -- -> category_mapping.id
  amount                INTEGER NULL,                -- NO SQL default (AQL default 0)
  description           TEXT    NULL,                -- payee id -> payee_mapping.id
  notes                 TEXT    NULL,
  date                  INTEGER NULL,                -- YYYYMMDD
  financial_id          TEXT    NULL,                -- imported_id
  type                  TEXT    NULL,                -- legacy, unused by AQL
  location              TEXT    NULL,                -- legacy, unused by AQL
  error                 TEXT    NULL,                -- JSON
  imported_description  TEXT    NULL,
  starting_balance_flag INTEGER NULL DEFAULT 0,
  transferred_id        TEXT    NULL,
  sort_order            REAL    NULL,                -- NO SQL default (AQL default Date.now())
  tombstone             INTEGER NULL DEFAULT 0,
  cleared               INTEGER NULL DEFAULT 1,      -- 1582384163573_cleared.sql:3
  pending               INTEGER NULL DEFAULT 0,      -- 1582384163573_cleared.sql:4 (not in AQL schema)
  parent_id             TEXT    NULL,                -- 1608652596043_parent_field.sql:3
  schedule              TEXT    NULL,                -- 1618975177358_schedules.sql:26
  reconciled            INTEGER NULL DEFAULT 0,      -- 1697046240000_add_reconciled.sql:3
  raw_synced_data       TEXT    NULL                 -- 1739139550000_bank_sync_page.sql:5
);

CREATE TABLE categories (                            -- init.sql:58-64
  id                TEXT PRIMARY KEY,
  name              TEXT    NULL,
  is_income         INTEGER NULL DEFAULT 0,
  cat_group         TEXT    NULL,                    -- -> category_groups.id
  sort_order        REAL    NULL,
  tombstone         INTEGER NULL DEFAULT 0,
  hidden            BOOLEAN NOT NULL DEFAULT 0,      -- 1685007876842_add_category_hidden.sql:3
  goal_def          TEXT    NULL DEFAULT NULL,       -- 1694438752000_add_goal_targets.sql:5
  template_settings JSON    NULL DEFAULT '{"source": "notes"}', -- 1754611200000:3
  cleanup_def       TEXT    NULL DEFAULT NULL        -- 1778510362740:3
);

CREATE TABLE category_groups (                       -- 1555786194328:14-19 (rebuilt, UNIQUE dropped)
  id         TEXT PRIMARY KEY,
  name       TEXT    NULL,
  is_income  INTEGER NULL DEFAULT 0,
  sort_order REAL    NULL,
  tombstone  INTEGER NULL DEFAULT 0,
  hidden     BOOLEAN NOT NULL DEFAULT 0              -- 1685007876842:4
);

CREATE TABLE category_mapping (id TEXT PRIMARY KEY, transferId TEXT NULL);   -- init.sql:81-83
CREATE TABLE payee_mapping    (id TEXT PRIMARY KEY, targetId  TEXT NULL);    -- 1550601598648:19-21

CREATE TABLE payees (                                -- 1550601598648_payees.sql:3-8
  id               TEXT PRIMARY KEY,
  name             TEXT    NULL,
  category         TEXT    NULL,                     -- legacy, not in AQL schema
  tombstone        INTEGER NULL DEFAULT 0,
  transfer_acct    TEXT    NULL,                     -- -> accounts.id for transfer payees
  favorite         INTEGER NULL DEFAULT FALSE,       -- 1720664867241:3 "DEFAULT 0 DEFAULT FALSE"; last wins, FALSE = 0
  learn_categories BOOLEAN NULL DEFAULT 1            -- 1737158400000:3
);

CREATE TABLE payee_locations (                       -- 1768872504000_add_payee_locations.sql:3-10
  id TEXT PRIMARY KEY, payee_id TEXT NULL, latitude REAL NULL, longitude REAL NULL,
  created_at INTEGER NULL, tombstone INTEGER NULL DEFAULT 0
);

CREATE TABLE tags (                                  -- 1749799110000_add_tags.sql:3-8
  id          TEXT PRIMARY KEY,
  tag         TEXT NULL UNIQUE,                      -- the only UNIQUE on a synced table
  color       TEXT NULL,
  description TEXT NULL,
  tombstone   INTEGER NULL DEFAULT 0,                -- 1749799110001:3
  hidden      BOOLEAN NULL DEFAULT 0                 -- 1780327681000:3
);

CREATE TABLE notes (id TEXT PRIMARY KEY, note TEXT NULL);   -- 1632571489012_remove_cache.js:25-27

-- ===== Budget =====
CREATE TABLE zero_budgets (                          -- 1632571489012_remove_cache.js:11-16
  id        TEXT PRIMARY KEY,                        -- see §3.8
  month     INTEGER NULL,                            -- YYYYMM
  category  TEXT    NULL,
  amount    INTEGER NULL DEFAULT 0,
  carryover INTEGER NULL DEFAULT 0,
  goal      INTEGER NULL DEFAULT NULL,               -- 1694438752000:3
  long_goal INTEGER NULL DEFAULT NULL                -- 1720665000000_goal_context.sql:3
);
CREATE TABLE reflect_budgets ( ...identical columns/defaults to zero_budgets... );  -- remove_cache.js:18-23, 1694438752000:4, 1720665000000:4
CREATE TABLE zero_budget_months (                    -- 1632571489012_remove_cache.js:7-9
  id TEXT PRIMARY KEY,                               -- 'YYYY-MM'
  buffered INTEGER NULL DEFAULT 0
);

-- ===== Rules, schedules =====
CREATE TABLE rules (                                 -- 1597756566448_rules.sql:3-8
  id TEXT PRIMARY KEY, stage TEXT NULL, conditions TEXT NULL, actions TEXT NULL,
  tombstone INTEGER NULL DEFAULT 0,
  conditions_op TEXT NULL DEFAULT 'and'              -- 1679728867040:3
);

CREATE TABLE schedules (                             -- 1618975177358_schedules.sql:3-9
  id                     TEXT PRIMARY KEY,
  rule                   TEXT    NULL,               -- required only at the AQL layer
  active                 INTEGER NULL DEFAULT 0,     -- not in AQL schema
  completed              INTEGER NULL DEFAULT 0,
  posts_transaction      INTEGER NULL DEFAULT 0,
  tombstone              INTEGER NULL DEFAULT 0,
  name                   TEXT    NULL DEFAULT NULL,  -- 1681115033845:3
  custom_upcoming_length TEXT    NULL DEFAULT NULL,  -- 1769000000000:3
  sort_order             REAL    NULL DEFAULT 0      -- 1783004650757:3
);

CREATE TABLE schedules_next_date (                   -- 1618975177358_schedules.sql:11-17
  id TEXT PRIMARY KEY, schedule_id TEXT NULL,
  local_next_date INTEGER NULL, local_next_date_ts INTEGER NULL,
  base_next_date  INTEGER NULL, base_next_date_ts  INTEGER NULL,
  tombstone INTEGER NULL DEFAULT 0                   -- 1691233396000:3
);

CREATE TABLE schedules_json_paths (                  -- 1618975177358_schedules.sql:19-24 (derived, not synced via db.insert)
  schedule_id TEXT PRIMARY KEY, payee TEXT NULL, account TEXT NULL, amount TEXT NULL, date TEXT NULL
);

CREATE TABLE transaction_filters (                   -- 1688749527273_transaction_filters.sql:3-8
  id TEXT PRIMARY KEY, name TEXT NULL, conditions TEXT NULL,
  conditions_op TEXT NULL DEFAULT 'and', tombstone INTEGER NULL DEFAULT 0
);

CREATE TABLE cleanup_groups (                        -- 1778510362740:5-8
  id TEXT PRIMARY KEY, name TEXT NOT NULL, tombstone INTEGER NULL DEFAULT 0
);

-- ===== Reports / dashboard =====
CREATE TABLE custom_reports (                        -- 1707267033000_reports.sql:3-26
  id TEXT PRIMARY KEY, name TEXT NULL, start_date TEXT NULL, end_date TEXT NULL,
  date_static INTEGER NULL DEFAULT 0, date_range TEXT NULL,
  mode TEXT NULL DEFAULT 'total', group_by TEXT NULL DEFAULT 'Category',
  balance_type TEXT NULL DEFAULT 'Expense',
  show_empty INTEGER NULL DEFAULT 0, show_offbudget INTEGER NULL DEFAULT 0,
  show_hidden INTEGER NULL DEFAULT 0, show_uncategorized INTEGER NULL DEFAULT 0,
  selected_categories TEXT NULL,                     -- nulled by 1722717601000 js:52-54
  graph_type TEXT NULL DEFAULT 'BarGraph', conditions TEXT NULL,
  conditions_op TEXT NULL DEFAULT 'and', metadata TEXT NULL,
  interval TEXT NULL DEFAULT 'Monthly', color_scheme TEXT NULL,
  tombstone INTEGER NULL DEFAULT 0,
  include_current INTEGER NULL DEFAULT 0,            -- 1716359441000:3
  sort_by TEXT NULL DEFAULT 'desc',                  -- 1738491452000:4 (renamed/re-added)
  trim_intervals INTEGER NULL DEFAULT 0,             -- 1759260219000:4
  show_trend_lines INTEGER NULL DEFAULT 0            -- 1780099200000:3
);

CREATE TABLE dashboard (                             -- 1722804019000_create_dashboard_table.js:14-22
  id TEXT PRIMARY KEY, type TEXT NULL, width INTEGER NULL, height INTEGER NULL,
  x INTEGER NULL, y INTEGER NULL, meta TEXT NULL, tombstone INTEGER NULL DEFAULT 0,
  dashboard_page_id TEXT NULL                        -- 1765518577215_multiple_dashboards.js:15
);
CREATE TABLE dashboard_pages (                       -- 1765518577215_multiple_dashboards.js:7-10
  id TEXT PRIMARY KEY, name TEXT NULL, tombstone INTEGER NULL DEFAULT 0
);

-- ===== Settings =====
CREATE TABLE preferences (id TEXT PRIMARY KEY, value TEXT NULL);   -- 1723665565000_prefs.js:21-23

-- ===== Sync / internal =====
CREATE TABLE messages_crdt (                         -- init.sql:73-79
  id        INTEGER PRIMARY KEY,                     -- rowid alias
  timestamp TEXT NOT NULL UNIQUE,
  dataset   TEXT NOT NULL,
  row       TEXT NOT NULL,
  column    TEXT NOT NULL,
  value     BLOB NOT NULL                            -- serialized "S:…", "N:…", "0:"
);
CREATE TABLE messages_clock (id INTEGER PRIMARY KEY, clock TEXT NULL);   -- init.sql:85
CREATE TABLE messages_pending (                      -- messages-pending.ts:10-17 (NON-synced)
  dataset TEXT NOT NULL, row TEXT NOT NULL, column TEXT NOT NULL,
  timestamp TEXT NOT NULL, value TEXT NOT NULL,
  PRIMARY KEY (dataset, row, column)
);
CREATE TABLE kvcache     (key TEXT PRIMARY KEY, value TEXT NULL);   -- remove_cache.js:29
CREATE TABLE kvcache_key (id INTEGER PRIMARY KEY, key REAL NULL);   -- remove_cache.js:30
CREATE TABLE __meta__       (key TEXT PRIMARY KEY, value TEXT NULL);    -- 1615745967948_meta.sql:3
CREATE TABLE __migrations__ (id INT PRIMARY KEY NOT NULL);              -- init.sql:88

-- ===== Legacy, never written by v26.11.0 app code =====
CREATE TABLE created_budgets (month TEXT PRIMARY KEY);                  -- init.sql:1 (only referenced in mocks/setup.ts:99)
CREATE TABLE pending_transactions (                                     -- init.sql:30-36
  id TEXT PRIMARY KEY, acct INTEGER NULL, amount INTEGER NULL, description TEXT NULL, date TEXT NULL,
  FOREIGN KEY(acct) REFERENCES accounts(id)
);
-- sqlite_stat1 (tbl, idx, stat) exists because migrations run ANALYZE.
-- Dropped along the way: db_version, spreadsheet_cells, payee_rules.
```

Indexes (other than the autoindexes on PK and UNIQUE columns):

| Index | Columns | Source |
|---|---|---|
| `trans_category_date` | `transactions(category, date)` | `mig/1561751833510_indexes.sql:3` |
| `trans_category` | `transactions(category)` | `mig/1561751833510_indexes.sql:4` |
| `trans_date` | `transactions(date)` | `mig/1561751833510_indexes.sql:5` |
| `trans_parent_id` | `transactions(parent_id)` | `mig/1608652596043_parent_field.sql:11` |
| `trans_sorted` | `transactions(date desc, starting_balance_flag, sort_order desc, id)` | `mig/1608652596044_trans_views.sql:31` |
| `messages_crdt_search` | `messages_crdt(dataset, row, column, timestamp)` | `mig/1612625548236_optimize.sql:3` |
| `idx_payee_locations_payee_id` | `payee_locations(payee_id)` | `mig/1768872504000_add_payee_locations.sql:13` |
| `idx_payee_locations_tombstone_payee_created` | `payee_locations(tombstone, payee_id, created_at)` | `mig/1768872504000_add_payee_locations.sql:16` |
| `idx_payee_locations_geo_tombstone` | `payee_locations(tombstone, latitude, longitude)` | `mig/1768872504000_add_payee_locations.sql:19` |
| `idx_transactions_acct_tombstone` | `transactions(acct, tombstone)` | `mig/1780606215001_add_performance_indexes.sql:3` |
| `idx_transactions_schedule` | `transactions(schedule)` | `mig/1780606215001_add_performance_indexes.sql:4` |

### 3.4 Rows present in a fresh budget after creation and first load

| Table | Rows | Source |
|---|---|---|
| `category_groups` / `categories` / `category_mapping` | 3 / 7 / 7 seed rows | `reference/actual/packages/loot-core/default-db.sqlite` (see §3.1) |
| `dashboard` | 12 widgets, uuid ids, all with the same `dashboard_page_id` | `mig/1722804019000_create_dashboard_table.js:25-42`, `mig/1765518577215_multiple_dashboards.js:26-28` |
| `dashboard_pages` | 1, `name='Main'` | `mig/1765518577215_multiple_dashboards.js:19-23` |
| `__migrations__` | 60 ids (§3.7) | `lc/server/migrate/migrations.ts:143-145` |
| `__meta__` | `('view-hash', md5(view SQL))` | `lc/server/update.ts:15-31` |
| `messages_clock` | 1 row `(1, '{"timestamp":…,"merkle":{}}')` written by `loadClock` | `lc/server/db/index.ts:99-115` |
| `kvcache_key` | 1 row `(1, <random REAL>)` written whenever the spreadsheet is marked clean. On desktop and api, `kvcache` itself lives in a sibling `cache.sqlite`. In the browser it lives in `db.sqlite`. | `lc/server/sheet.ts:55-79`, `lc/server/sheet.ts:120-140` |
| `schedules_json_paths` | rebuilt with `INSERT OR REPLACE` for every rule with a `link-schedule` action (none in a fresh budget) | `lc/server/schedules/app.ts:536-574`, `lc/server/schedules/app.ts:858` |
| `preferences`, `zero_budgets`, `zero_budget_months`, `payees`, `accounts`, `messages_crdt` | empty | none |

### 3.5 How Actual writes rows, and the defaults it applies

- AQL field names map to physical columns: `is_parent→isParent`, `is_child→isChild`, `account→acct`, `imported_id→financial_id`, `imported_payee→imported_description`, `transfer_id→transferred_id`, `payee→description` (`lc/server/aql/schema/index.ts:374-382`).
- Every write goes out as one CRDT message per column, with no batching into a single row write (`lc/server/db/index.ts:210-261`). The first message for a row runs `INSERT INTO t (id, col) VALUES (?, ?)`; each later one runs `UPDATE t SET col=? WHERE id=?` (`lc/server/sync/index.ts:146-161`). The net effect is a column-wise upsert: **a column that no message touches keeps its SQL default.**
- Deletes are always `tombstone = 1` (`lc/server/db/index.ts:263-273`). Booleans are written as `1`/`0` (`lc/server/aql/schema-helpers.ts:38-39`).
- `convertForInsert` fills the AQL schema defaults, then drops null fields (`skipNull`), so a null column is never written and the SQL default applies (`lc/server/aql/schema-helpers.ts:151-184`). A Veritas writer that wants Actual-identical rows must apply the AQL defaults itself, because they differ from the SQL ones:
  - `transactions` (`lc/server/aql/schema/index.ts:35-59`): `amount` defaults to 0 and is required; the SQL column has no default (`:42`). `sort_order` defaults to `Date.now()` (epoch ms); the SQL column has no default (`:51`). `cleared` defaults to true (SQL default also 1). `reconciled` defaults to false. `account` and `date` are required (`:40-45`).
  - `payees.name` is required (`lc/server/aql/schema/index.ts:62`). `accounts.name` is required (`:70`).
  - `categories.template_settings` defaults to `{source:'notes'}` (`lc/server/aql/schema/index.ts:97`).
  - `schedules.rule` is required (`lc/server/aql/schema/index.ts:117`). The SQL column is nullable.
  - `custom_reports` (`lc/server/aql/schema/index.ts:162-181`): `start_date '2023-06'` and `end_date '2023-09'`, which have no SQL default. The other defaults match SQL.
  - `dashboard` requires `type`, `width`, `height`, `x` and `y` (`lc/server/aql/schema/index.ts:211-215`).
  - `payee_locations` requires `payee_id`, `latitude`, `longitude` and `created_at` (`lc/server/aql/schema/index.ts:221-224`).

**A new transaction insert** goes `db.insertTransaction` → `insertWithSchema` → `convertForInsert` (`lc/server/db/index.ts:297-308`, `:907-911`).

Schema defaults that are always written (`lc/server/aql/schema/index.ts:35-59`):

| Column | Default |
|---|---|
| `amount` | `0` |
| `sort_order` | `Date.now()` |
| `cleared` | `1` |
| `reconciled` | `0` |

Columns that are never written when null or absent, so the SQL default applies:

| Column | SQL default | Source |
|---|---|---|
| `tombstone` | 0 | `lc/server/sql/init.sql:56` |
| `isParent` | 0 | `lc/server/sql/init.sql:40` |
| `isChild` | 0 | `lc/server/sql/init.sql:41` |
| `starting_balance_flag` | 0 | `lc/server/sql/init.sql:53` |
| `category`, `description`, `notes`, `transferred_id`, `parent_id`, `schedule`, `error`, `financial_id`, `imported_description` | NULL | |

Overrides by path:

- The desktop new-row template has `cleared: false`, date = the last date entered or today, and category = the current category view (`dc/components/transactions/table/utils.ts:224-241`).
- Transfer counterparts get `cleared: false` (§4.2).
- Schedule posts get `cleared: false` (`lc/server/schedules/app.ts:602-613`).
- Imports take `cleared` from the option, default true (`lc/server/accounts/sync.ts:765-771`).
- `addTransactions` uses `cleared ?? true` (`lc/server/accounts/sync.ts:1059-1064`).

### 3.6 Views, tombstones and mapped reads

- `updateViews()` generates the SQL with `makeViews(schema, schemaConfig)`, hashes it with md5 and compares the hash to `__meta__['view-hash']`. It runs `DROP VIEW IF EXISTS …; CREATE VIEW …` **only when the hash differs**, then writes the new hash with `INSERT OR REPLACE` (`lc/server/update.ts:14-32`, `lc/server/aql/views.ts:19-57`). A tool that edits `v_*` but leaves the hash alone leaves Actual running with broken views. Veritas never touches `v_*` or `__meta__`.
- Views are not synced and are rebuilt on startup (`mig/README.md:20`).
- The views are defined in `lc/server/aql/schema/index.ts:313-433`:

| View | Built from | What it does |
|---|---|---|
| `v_payees` | `payees` | Every payee field. `name` becomes `COALESCE(__accounts.name, _.name)`. Transfer payees whose account is tombstoned are excluded. Tombstoned payees are **not** filtered (`:315-327`). |
| `v_categories` | `categories` | Every category field plus `cat_group AS "group"`. No tombstone filter (`:335-338`). |
| `v_schedules` | `schedules`, `schedules_next_date`, `schedules_json_paths`, `rules`, `payee_mapping` | Adds `next_date` (local or base, depending on the `_ts` match), `_payee`, `_account`, `_amount`, `_amountOp`, `_date`, `_conditions`, `_actions` and `_has_splits` (`:341-371`). |
| `v_transactions_internal` | `transactions` | Renames columns to their public names. Maps `category` through `category_mapping` (NULL for parents) and `payee` through `payee_mapping`. `amount` becomes `IFNULL(amount,0)`. Requires `date` and `acct` to be non-null and children to have a `parent_id` (`:385-403`). |
| `v_transactions_internal_alive` | `v_transactions_internal` | Drops tombstoned rows and children whose parent is tombstoned (`:408-412`). |
| `v_transactions` | `v_transactions_internal_alive` | Joins to live payees, categories and accounts, so references to dead rows become NULL. Ordered by `date desc, starting_balance_flag, sort_order desc, id` (`:414-431`). |

Further view rules:

- `v_transactions_internal` treats `parent_id` as null unless `isChild = 1`, hides children whose `parent_id` is null, and **always returns `category = NULL` for parents**, whatever the column holds (`lc/server/aql/schema/index.ts:384-402`).
- A child is alive only if its parent is not tombstoned (`lc/server/aql/schema/index.ts:404-411`).
- Budget sums use the alive view, so they use the mapped category even when the target is tombstoned (`lc/server/budget/base.ts:138-143`). Schedules resolve their payee condition through `payee_mapping` (`lc/server/aql/schema/index.ts:369`).

**Tables with a `tombstone` column:**

- From `init.sql`: `banks`, `accounts`, `transactions`, `categories`, `category_groups` (`lc/server/sql/init.sql:12`, `:28`, `:56`, `:64`, `:71`).
- From migrations: `payees` and `payee_rules` (`mig/1550601598648_payees.sql:7`, `:15`), `rules` (`mig/1597756566448_rules.sql:8`), `schedules` (`mig/1618975177358_schedules.sql:9`), `transaction_filters` (`mig/1688749527273_transaction_filters.sql:8`), `schedules_next_date` (`mig/1691233396000_add_schedule_next_date_tombstone.sql:3`), `custom_reports` (`mig/1707267033000_reports.sql:25`), `dashboard` (`mig/1722804019000_create_dashboard_table.js:22`), `tags` (`mig/1749799110001_tags_tombstone.sql:3`), `dashboard_pages` (`mig/1765518577215_multiple_dashboards.js:10`), `payee_locations` (`mig/1768872504000_add_payee_locations.sql:9`), `cleanup_groups` (`mig/1778510362740_add_cleanup_groups_and_def.sql:8`), `account_groups` (`mig/1787013118115_add_account_groups.sql:7`).
- **No tombstone:** `category_mapping`, `payee_mapping` (`lc/server/sql/init.sql:81-83`, `mig/1550601598648_payees.sql:19-21`), `zero_budgets`, `zero_budget_months` (Actual never deletes those rows).

### 3.7 `__migrations__`: manifest, cutoff and validity check

**Ordered ids in a fresh v26.11.0 budget (60 rows).**

Migration files are the `*.sql` and `*.js` files in `migrations/`, sorted by their numeric prefix (`lc/server/migrate/migrations.ts:79-95`). `README.md` is not one; there are 61 directory entries but 60 migrations. `applyMigration` inserts each id after it succeeds (`lc/server/migrate/migrations.ts:143-145`). The test asserts 57 of them are ≤ the cutoff (`lc/server/migrate/additive-migrations.test.ts:70`).

```
1548957970627 1550601598648 1555786194328 1561751833510 1567699552727
1582384163573 1597756566448 1608652596043 1608652596044 1612625548236
1614782639336 1615745967948 1616167010796 1618975177358 1632571489012
1679728867040 1681115033845 1682974838138 1685007876842 1686139660866
1688749527273 1688841238000 1691233396000 1694438752000 1697046240000
1704572023730 1704572023731 1707267033000 1712784523000 1716359441000
1720310586000 1720664867241 1720665000000 1722717601000 1722804019000
1723665565000 1730744182000 1736640000000 1737158400000 1738491452000
1739139550000 1740506588539 1745425408000 1749799110000 1749799110001
1754611200000 1759260219000 1759842823172 1762178745667 1765518577215
1768872504000 1769000000000 1778510362740 1780099200000 1780327681000
1780606215000 1780606215001            <- last pre-cutoff (== cutoff, counts as pre)
1783004650757 1787013118115 1788468782000   <- post-cutoff (additive-only era)
```

The JS migrations are 1632571489012, 1722717601000, 1722804019000, 1723665565000, 1765518577215 and 1788468782000 (`lc/server/migrate/migrations.ts:20-27`).

Two other id rules:
- An id is parsed as `parseInt(name.match(/^(\d)+/)[0])` (`lc/server/migrate/migrations.ts:43-45`).
- `patchBadMigrations` rewrites the legacy id `1685375406832` to `1688749527273` before any check runs (`lc/server/migrate/migrations.ts:55-67`).

**`ADDITIVE_ONLY_CUTOFF`.**

`ADDITIVE_ONLY_CUTOFF = 1780606215001` (`lc/server/migrate/migrations.ts:155`). Migrations after it may only add things: new tables, nullable columns or `NOT NULL DEFAULT` columns, non-unique indexes, and view changes (`mig/README.md:11-20`). `messages_pending` is the one non-synced table (`lc/server/migrate/additive-migrations.test.ts:34`).

**`checkDatabaseValidity(appliedIds, available)`, exactly.**

Source: `lc/server/migrate/migrations.ts:157-246`. It runs inside `migrate()` after `patchBadMigrations` (`lc/server/migrate/migrations.ts:248-262`). Every failure throws `Error('out-of-sync-migrations')`, which `_loadBudget` turns into `{error:'out-of-sync-migrations'}` and then closes the budget (`lc/server/budgetfiles/app.ts:627-639`). There are five rules, checked in order:

1. If `available.length === 0 && appliedIds.length > 0`, the file is invalid (`:164-169`).
2. `unknownIds = applied − available`. If any unknown id is `≤ CUTOFF`, the file is invalid (`:171-184`).
3. If `unknownIds.length > 0` and the number of known applied ids is not equal to `available.length`, the file is invalid. In other words, a file touched by a newer Actual must contain **every** migration this version knows (`:186-202`).
4. If any applied id is `> CUTOFF` while some pre-cutoff migration is missing, the file is invalid (`:208-230`).
5. The pre-cutoff applied ids must be an **exact ordered prefix** of the pre-cutoff available ids, compared index by index. Any gap or mismatch makes the file invalid (`:231-245`).

After the check, `getPending` applies every available id that isn't recorded yet, in id order (`lc/server/migrate/migrations.ts:97-102`, `:255-259`). Post-cutoff gaps are allowed and simply get applied.

**Newer and older files.**

- **Older file**: no error. Its missing ids form a valid prefix, so they are applied on load.
- **Newer file**: it has unknown ids `> CUTOFF` (for example from v26.12). It opens **only if** it also has all 60 known ids (rules 2 and 3). Columns it doesn't know about are ignored, and incoming sync messages that touch unknown schema are deferred into `messages_pending` (`lc/server/sync/index.ts:93-113`).
- There is no explicit "newer/older" flag. There is no `user_version`, and `db_version` was dropped.

**What this means for Veritas:**

- If a file leaves out one of these ids while the schema already contains its effects, Actual re-runs that migration on load. The `ALTER TABLE … ADD COLUMN` statements then fail on a column that already exists, giving `error:'loading-budget'` (`lc/server/budgetfiles/app.ts:631-636`).
- If a file leaves out a pre-cutoff id while a later one is present, Actual fails it with `out-of-sync-migrations`.
- So a Veritas-created budget must hold all 60 ids exactly. Decision 1 gets there without Veritas ever writing `__migrations__`: new budgets start from a pinned template db that Actual produced.
- Veritas's own guard (R-IO-7) matches Actual's rules: exact match → read/write; a strict prefix → "upgrade in Actual first"; unknown ids → read-only.

### 3.8 Encodings

**Amounts:**
- Amounts are stored as an `INTEGER`. `IntegerAmount` is described as "the amount with the decimal point removed, e.g. 123.45 → 12345" (`lc/shared/util.ts:413-417`).
- Conversions use `amountToInteger(amount, decimalPlaces = 2)`, which is `Math.round(amount * 100)`, and `integerToAmount`, which divides by 100 (`lc/shared/util.ts:541-555`).
- Arithmetic is bounded to ±(2^51 − 1) by `safeNumber` (`lc/shared/util.ts:426-441`).
- Some currencies have `decimalPlaces: 0`, for example JPY (`lc/shared/currencies.ts:50`). But the only helper that honours that, `amountToCurrencyInteger`, has no callers (`lc/shared/util.ts:485-487`). In practice storage is ×100 for every currency, and the decimal count only affects display. [unverified]: the oracle checks a JPY budget.

**Sign:** outflow is negative and inflow is positive. The transaction table maps `amount < 0` to debit and `amount > 0` to credit (`dc/components/transactions/table/utils.ts:37-38`). The default "Total Expenses" widget filters on `amount lt 0` (`lc/shared/dashboard.ts:59-63`).

**Dates (`transactions.date`):**
- Stored as an `INTEGER` `YYYYMMDD`: `toDateRepr('2026-11-09')` gives `20261109` and `fromDateRepr` reverses it (`lc/server/models.ts:42-63`).
- The same representation is used wherever the AQL type is `date`, including `schedules_next_date.*_next_date`, which the schema exposes as `next_date: f('date')` (`lc/server/aql/schema/index.ts:118`).

**Months:**
- `zero_budgets.month` and `reflect_budgets.month` are `INTEGER` `YYYYMM`: `dbMonth('2026-11')` gives `202611`, and `monthFromDbMonth` pads it back (`lc/server/budget/actions.ts:58-65`).
- The spreadsheet sheet name is `'budget' + 'YYYYMM'`, for example `budget202611` (`lc/shared/months.ts:434-436`).

**`zero_budgets.id` and `reflect_budgets.id`:**
- Rows written now use `` `${dbMonth}-${categoryId}` ``, for example `202611-541836f1-e756-…` (`lc/server/budget/actions.ts:147-152`, and the same in `setGoal` at `:170-176`).
- Rows created by migration 1632571489012 use `` `${'YYYY-MM'}-${cat}` ``, for example `2019-03-<uuid>` (`mig/1632571489012_remove_cache.js:50`, `:71`).
- All readers and writers look rows up **by `(month, category)`, never by id** (`lc/server/budget/actions.ts:119-122`, `:138-146`). Veritas must do the same and accept both id shapes.

**`zero_budget_months.id`:** this is the `'YYYY-MM'` string, for example `2026-11` (`lc/server/budget/actions.ts:179-191`; read back with `sheetForMonth(budgetMonth.id)` at `lc/server/sheet.ts:262-264`).

**Booleans:** stored as INTEGER 0 or 1. `cleared` defaults to 1. `tombstone = 1` marks a soft delete (`lc/server/db/index.ts:263-268`).

**CRDT values** in `messages_crdt.value`: `'0:'` for null, `'N:<num>'` for numbers and `'S:<str>'` for strings (`lc/server/sync/serialization.ts:22-36`).

### 3.9 Export and import

**`exportBuffer()`** (`lc/server/cloud-storage.ts:145-210`):

1. It reads `{id, budgetName}` from the in-memory prefs. If `budgetName` is falsy it **returns null**, meaning no export at all (`:146-149`).
2. Inside `runMutator`, so it is serialized with other writes, it reads `db.sqlite` from disk. It opens those bytes as an in-memory db and runs `DELETE FROM kvcache; DELETE FROM kvcache_key;` (`:157-173`).
3. It re-serializes with `sqlite.exportDatabase`. On electron and api that means `db.backup(tmpfile)`, reading the file back and deleting the temp file (`:175`, `lc/platform/server/sqlite/index.electron.ts:145-156`). **The zipped db is a SQLite backup copy, not the original bytes.** Every table, extra ones included, survives, but the bytes and page layout may differ.
4. It reads `metadata.json` **from disk**, parses it, sets `resetClock = true` and re-stringifies it. Every other field in the file is carried over unchanged (`:181-186`).
5. The zip has exactly two entries at the root, `db.sqlite` and `metadata.json` (`:188-193`). It is made with fflate `zipSync` at default settings, after a safe-name check (`lc/server/util/zip.ts:99-104`).
6. Warnings: `exceeds-import-size-limit` if the archive or the total size is over 500 MB, and `may-exceed-available-memory` (`:196-209`, `lc/server/util/zip.ts:9`, `:106-123`).

Fields in `metadata.json` on export are whatever the file holds. The possible keys come from `MetadataPrefs` (`lc/types/prefs.ts:83-94`): `budgetName, id, lastUploaded, cloudFileId, groupId, encryptKeyId, lastSyncedTimestamp, resetClock, lastScheduleRun, userId`. A purely local budget typically holds:
- `id` and `budgetName`, from creation (`lc/server/prefs.ts:89-91`).
- `userId`, written on load when it's missing. If there is no token the value is `undefined`, so the key disappears (`lc/server/budgetfiles/app.ts:617-620`).
- `resetClock:false`, after any load of a file that had `resetClock` set (`lc/server/budgetfiles/app.ts:671`).
- `lastScheduleRun:'YYYY-MM-DD'`, written after a successful "sync" event (see the table below).
- Export then forces `resetClock:true` in the copy.

**`importBuffer(fileData, buffer, localId?)`** (`lc/server/cloud-storage.ts:212-296`):

**Zip rules** (`lc/server/util/zip.ts:43-97`, `lc/server/cloud-storage.ts:213-247`):
- Unzipping goes through `safeUnzip`:
  - The archive, each entry and the total uncompressed size are each capped at 500 MB.
  - Unsafe entry names are rejected: NUL, backslash, a drive letter, a leading `/`, or a `..` segment.
  - Duplicate names are rejected, compared **case-insensitively**.
  - Breaking any of these gives `zip-too-large`. Any other unzip failure gives `not-zip-file`.
- These checks apply to **every** entry, not only the two that matter.
- Every entry named `db.sqlite` or ending in `/db.sqlite` is a db candidate, and every entry named `metadata.json` or ending in `/metadata.json` is a metadata candidate.
- The chosen directory must contain both files. The root (`''`) wins if it qualifies. Otherwise exactly one shared directory must exist; anything else gives `invalid-zip-file`. Other entries are ignored.
- `metadata.json` must parse as JSON, otherwise `invalid-meta-file`. **No field is required or validated** (`:249-254`).

**Rewrites** (`:258-275`):
- The new `id` is a fresh `uuidv4()`, which is also the budget directory name. The archive's id is never used.
- `meta` becomes `{...archiveMeta, id, cloudFileId: fileData.fileId, groupId: fileData.groupId, lastUploaded: currentDay(), encryptKeyId: fileData.encryptMeta ? … : null}`.
- The importer for local files passes `{cloudFileId: null, groupId: null}` (`lc/server/importers/actual.ts:16-19`). Because it names the field `cloudFileId` instead of `fileId`, `cloudFileId` comes out **`undefined` and is dropped by `JSON.stringify`**, while `groupId` is written as `null` and `encryptKeyId` as `null`.
- `lastUploaded` is set to today as `'YYYY-MM-DD'`.
- **`resetClock`, `lastScheduleRun`, `budgetName`, `lastSyncedTimestamp` and `userId` are carried over from the archive unchanged.**

**Write** (`:276-293`): the importer creates the directory, or deletes the old `db.sqlite` and `metadata.json` while keeping backups. It writes the db bytes verbatim and writes `JSON.stringify(meta)`.

**Then `importActual` writes to the db** before loading it (`lc/server/importers/actual.ts:27-39`):
- It opens the new file and runs `DELETE FROM kvcache; DELETE FROM kvcache_key;`.
- It then runs `load-budget`, `get-budget-bounds`, `waitOnSpreadsheet`, and `upload()` with errors ignored (`:43-48`).
- **The result of `load-budget` is not checked** (`:43`). If migrations fail, the next call throws and the import as a whole returns `'internal-error'` (`lc/server/budgetfiles/app.ts:550-553`). The budget directory stays on disk. [unverified]: nothing on that path deletes it.

**What load does** (`_loadBudget`, `lc/server/budgetfiles/app.ts:579-726`), in order, with every db or file write:

1. `loadPrefs(id)` parses `metadata.json`. If parsing fails it falls back to `{id, budgetName:id}`. It always forces `id` to the directory id (`lc/server/prefs.ts:24-45`). Then the db is opened directly, with no WAL (`lc/server/db/index.ts:72-81`).
2. If `userId` is missing, it calls `savePrefs({userId})`, which **rewrites `metadata.json`** (`:617-620`, `lc/server/prefs.ts:75-78`).
3. `updateVersion()` runs `migrate()`, which does the validity check plus the pending migrations (§3.7), and then `updateViews()`, which rebuilds views only if `__meta__.view-hash` differs (§3.6) (`:623`, `lc/server/update.ts:10-37`).
4. `replayPendingMessages()` runs only when a sync server is configured (`:647-649`).
5. `loadClock()` inserts `messages_clock (1, clock)` if no row exists (`:656`, `lc/server/db/index.ts:99-115`).
6. If `prefs.resetClock` is set, it assigns a new 16-hex client id with `CRDT.makeClientId()`, runs `INSERT OR REPLACE INTO messages_clock (1, …)`, and calls `savePrefs({resetClock:false})`, which **rewrites `metadata.json`** (`:658-672`, `reference/actual/packages/crdt/src/crdt/timestamp.ts:79-81`).
7. `startBackupService` runs on non-browser builds outside tests. It writes backups next to the db, not into it (`:674-676`).
8. `loadSpreadsheet` checks the kvcache:
   - If the cache is clean, meaning `kvcache_key` in `db.sqlite` equals the one in `cache.sqlite` **and** `kvcache` is non-empty, it loads cell values from the cache **without recomputing**.
   - Otherwise it calls `loadUserBudgets`, which reads `preferences.budgetType`, then `zero_budgets` or `reflect_budgets`, then `zero_budget_months` (`:679`, `lc/server/sheet.ts:81-110`, `:112-189`, `:218-272`).
   - Later recomputes write `kvcache` and set `kvcache_key` in both dbs (`lc/server/sheet.ts:33-79`).
9. It reads `budgetType` (default `'envelope'`) and calls `createAllBudgets()`, which builds the month range of §5.2 (`:686-693`, `lc/server/budget/base.ts:20-39`, `:422-447`).
   - This is **in-memory only** (`meta.createdMonths`). **It writes no rows** to `zero_budgets`, `zero_budget_months` or `created_budgets`.
   - `created_budgets` is never written by app code. The only reference is `lc/mocks/setup.ts:99`.
10. `loadMappings`, `loadRules`, `syncMigrations.listen()` and `startServices()` run next. Services include `trackJSONPaths`, which runs `INSERT OR REPLACE INTO schedules_json_paths` for each schedule-linked rule (`:696-699`, `lc/server/schedules/app.ts:551-560`, `:565-574`).
11. The syncing mode becomes `'enabled'` if `getServer()` returns a server, otherwise `'disabled'`. `possiblyUpload()` runs (`:704-717`).

**Writes after load:**
- Any "sync" event of type success, error or unauthorized, when `lastScheduleRun !== today`, runs `advanceSchedulesService`. That can post transactions and advance `schedules_next_date`.
- On success it also calls `savePrefs({lastScheduleRun: today})` (`lc/server/schedules/app.ts:860-880`).
- `fullSync` emits a `'success'` event even when syncing is disabled, because `_fullSync` returns `[]` early (`lc/server/sync/index.ts:802-876`, `:892-897`). The desktop app triggers `fullSync` on `app-focused` (`lc/server/main.ts:121-126`). So **even a local-only budget can auto-post schedules once a day when opened in the desktop app**.
- An imported `lastScheduleRun` equal to today suppresses that run on the importing device.
- `initialFullSync` also runs on budget load (`lc/server/budgetfiles/app.ts:264`), so for a local-only budget the schedule service runs **once a day, on open** (§6.3).

**`metadata.json` fields across export, import and load:**

| Field | Export | Import | Load |
|---|---|---|---|
| `resetClock` | forced `true` (`cloud-storage.ts:185`) | carried over from the archive | if truthy, new node id, `messages_clock` replaced, set to `false` and `metadata.json` rewritten (`budgetfiles/app.ts:658-672`) |
| `lastScheduleRun` | carried over | carried over | set to today after a successful "sync" event (`schedules/app.ts:870-878`); cleared when a posting schedule is created or edited (`schedules/app.ts:325-333`) |
| `groupId` | carried over | set to `null` (`fileData.groupId`, `cloud-storage.ts:272`) | none (sync identity only) |
| `cloudFileId` | carried over | removed: value is `undefined` and gets dropped (`cloud-storage.ts:271` with `importers/actual.ts:17`) | none |
| `encryptKeyId` | carried over | `null` (`cloud-storage.ts:274`) | none |
| `lastUploaded` | carried over | today, `'YYYY-MM-DD'` (`cloud-storage.ts:273`) | none |

**Actual's validity checks.**

On import, Actual checks only the zip safety rules, that both files sit in the same directory, and that the metadata is valid JSON (import rules above). It does no schema or integrity check. The only structural check is `checkDatabaseValidity` on load (§3.7). After that, any failure inside a migration or view rebuild comes back as `loading-budget`, and a failure in `openDatabase` or `loadSpreadsheet` as `opening-budget` (`lc/server/budgetfiles/app.ts:604-613`, `:678-684`). It never runs `PRAGMA integrity_check`, checks foreign keys, or checks `user_version`.

### 3.10 `kvcache`, `messages_crdt`, `messages_clock`

**`kvcache` / `kvcache_key`** cache the computed spreadsheet:
- `kvcache` holds `key = "<sheet>!<cell>"` and `value = JSON` (`lc/server/sheet.ts:33-53`).
- `kvcache_key` holds one row, `(1, random REAL)`, that acts as a "clean" token.
- On desktop and api the cache table lives in `<budgetDir>/cache.sqlite`, but the token is **also written into `db.sqlite`'s `kvcache_key`** (`lc/server/sheet.ts:60-75`, `:120-136`).
- Marking the cache dirty only deletes the token in the cache db (`lc/server/sheet.ts:76-78`).
- The cache is trusted if the tokens match and `kvcache` is non-empty (`lc/server/sheet.ts:81-110`).
- Export and import both empty the tables (`lc/server/cloud-storage.ts:167-173`, `lc/server/importers/actual.ts:29-38`).
- **Risk:** if Veritas edits a `db.sqlite` in place inside an Actual budget directory without clearing `kvcache_key`, Actual will show stale budget numbers. Clearing it is safe because the table is a cache, not a synced Actual row. The zip path doesn't have this problem. Decision 2 (zip only) rules this out for Veritas.

**`messages_crdt`:**
- A row is inserted for each applied message **only when `checkSyncingMode('enabled')`**, which means mode `'enabled'` or `'offline'` (`lc/server/sync/index.ts:73-77`, `:520-528`).
- `_loadBudget` sets the mode to `'enabled'` exactly when a sync server URL is configured on the device (`getServer()` is non-null), **whether or not this file was ever uploaded** (`lc/server/budgetfiles/app.ts:708-712`).
- Import mode applies messages directly with no CRDT log (`lc/server/sync/index.ts:372-390`, `:403-406`).
- A local-only budget (no server configured) therefore never writes `messages_crdt`. Archives from synced budgets carry their full CRDT log, and import doesn't strip it.

**`messages_clock`:**
- Holds a single row, `id=1`, with `clock = JSON {timestamp, merkle}` (`reference/actual/packages/crdt/src/crdt/timestamp.ts:49-54`).
- Written by `loadClock` on the first load of any budget, local-only included (`lc/server/db/index.ts:105-114`).
- Written on load when `resetClock` is set (`lc/server/budgetfiles/app.ts:665-669`).
- Written after each applied batch **only when sync is enabled** (`lc/server/sync/index.ts:533-541`).

**Local-only behaviour:** in a local-only budget the in-memory clock still produces timestamps for each message, but nothing is logged. `messages_clock` keeps whatever was written at the last load. Undo still works from in-memory state (`lc/server/sync/index.ts:549`).

**`messages_pending`** holds newer-version messages that touch schema this version doesn't have. It is replayed only when a server is configured (`lc/server/sync/index.ts:93-113`, `lc/server/budgetfiles/app.ts:647-649`).

### 3.11 Preferences and budget type

Synced preferences live in `preferences(id, value TEXT)`. Every value is a string (`lc/server/preferences/app.ts:47-61`). A fresh budget has **no rows**, and readers apply defaults.

| Key | Values | Effect | Source |
|---|---|---|---|
| `budgetType` | `'envelope'` (the default when the row is missing) or `'tracking'` | Picks the table, `zero_budgets` or `reflect_budgets`, and the spreadsheet model. Migration 1745425408000 rewrote the old values `'report'` → `'tracking'` and anything else → `'envelope'`. | `lc/server/budgetfiles/app.ts:687-692`, `lc/server/budget/actions.ts:45-56`, `lc/server/sheet.ts:224-230`, `mig/1745425408000_update_budgetType_pref.sql:3-5` |
| `defaultCurrencyCode` | ISO code | Formatting and formulas | `lc/server/preferences/app.ts:31-37`, `lc/server/formulas/customFunctionsPreferences.ts:123` |
| `numberFormat`, `hideFraction`, `currencySymbolPosition`, `currencySpaceBetweenAmountAndSymbol` | strings | Number formatting | `lc/server/preferences/app.ts:31-37` |
| `dateFormat`, `firstDayOfWeekIdx` | strings | Display | `lc/types/prefs.ts:24-25` |
| `upcomingScheduledTransactionLength` | `'1' \| '7' \| '14' \| '30'` | How far ahead upcoming schedules are shown | `lc/server/prefs.ts:13-18`, `lc/server/schedules/app.ts:700` |
| `learn-categories` | string boolean | Rule learning from category edits | `lc/types/prefs.ts:73` |
| `flags.<FeatureFlag>` | string boolean | Feature flags, e.g. `goalTemplatesEnabled`, `currency` | `lc/types/prefs.ts:1-15`, `:72` |
| per-account and per-import keys | various | UI and import state | `lc/types/prefs.ts:32-71` |

The full key set is `SyncedPrefs` (`lc/types/prefs.ts:20-77`). The `budgetName` prefs message syncs through dataset `'prefs'`, which isn't a table (`lc/server/prefs.ts:55-68`, `lc/server/sync/index.ts:127-128`). Changing `budgetType` through sync calls `setType`, which resets `createdMonths` and recomputes (`lc/server/sync/index.ts:508-510`, `:551-553`, `lc/server/budget/base.ts:449-470`).

**Budget type.** A file is tracking **iff `preferences` has `id='budgetType'` with `value='tracking'`**. Any other value, or no row, means envelope (`lc/server/budget/actions.ts:49-56`, `lc/server/sheet.ts:224-230`, `lc/server/budget/base.ts:15-18`, `lc/server/prefs.ts:10-11`, `lc/server/budgetfiles/app.ts:686-692`).

- Legacy values were normalized by `mig/1745425408000_update_budgetType_pref.sql:3-5`: `'report'` → `'tracking'`, anything else → `'envelope'`. The preference was originally copied out of `metadata.json` (`mig/1723665565000_prefs.js:1-17`, `:36-55`).
- Envelope stores budgets in `zero_budgets`, tracking in `reflect_budgets` (same columns) (`lc/server/budget/actions.ts:45-47`). Tracking has no `zero_budget_months`. Both tables exist in every file. Switching type keeps both and only rebuilds cells (`lc/server/budget/base.ts:449-475`).
- Tracking sheets are also named `budgetYYYYMM`.
- Syncing the `budgetType` pref triggers `setType` (`lc/server/sync/index.ts:508-509`, `:551-552`).

---

## 4. Transactions, accounts, payees, categories

### 4.1 The server write path

How rows land is in §3.5.

- The central server write path is `batchUpdateTransactions` (`lc/server/transactions/index.ts:40-217`). It runs in this order:
  1. Snapshot the old payees of updated rows (`:72-82`).
  2. Insert the added rows. A parent, or a row in an off-budget account, gets `category = null` (`:87-98`).
  3. Tombstone the deleted rows. The deleted ids are expanded first to include their children (`:55-57`, `:100-110`).
  4. Apply the updates. If an update carries `account` and the row is a parent or the account is off-budget, it gets `category = null` (`:112-127`).
  5. Re-read every changed row from `v_transactions_internal` (`:137-151`).
  6. Run the transfer hooks: `onInsert` on every added row, `onUpdate` on every updated row, `onDelete` on every deleted row (`:161-172`). Skip this step when `runTransfers` is false.
  7. If `learnCategories` is set, call `updateCategoryRules` (`:174-186`).
  8. Detect orphaned payees (`:188-207`).
- The handlers `transaction-add`, `transaction-update` and `transaction-delete` all call this path. `transactions-batch-update` is also undoable (`lc/server/transactions/app.ts:54-67`, `:155-163`).

### 4.2 Transfers

**Creating a pair: `addTransfer`** (`lc/server/transactions/transfer.ts:47-93`).

`onInsert` fires when the new row's payee has a `transfer_acct`. It reads that from `v_payees`, using the mapped payee id (`lc/server/transactions/transfer.ts:12-22`, `:140-146`).

1. If the row `is_parent`, return without creating a transfer (`:48-54`). The comment says transfers for splits are created from the children.
2. Look up `fromPayee`, the transfer payee whose `transfer_acct` is the source account (`:56-59`).
3. Build a draft counterpart (`:61-70`):
   - `account` = the transfer account
   - `amount` = `-amount`
   - `payee` = `fromPayee`
   - `date` = the source date
   - `transfer_id` = the source id
   - `notes` = the source notes, or null
   - `schedule` = the source schedule
   - `cleared` = **false**
4. **Run rules on the counterpart.** Only `notes`, `cleared` and `schedule` from the result are kept (`:71-72`).
5. Insert the counterpart with `db.insertTransaction` (`:74-79`). This bypasses `batchUpdateTransactions`, so no category is written and the off-budget clear does not run. Schema defaults still fill `sort_order = Date.now()` and `reconciled = 0`. `isChild` is never set, so the counterpart is always a standalone row.
6. Update the source row with `transferred_id = <counterpart id>`, plus `schedule` if the rules matched one (`:81-85`).
7. Call `clearCategory`. If **both accounts have the same `offbudget` value**, set the source row's `category` to null. It also clears `transaction.transfer_id`'s category, but on the insert path that value is still unset (`:24-45`, `:86`).

**Updating: `onUpdate`** (`lc/server/transactions/transfer.ts:154-172`).

Source: `lc/server/transactions/transfer.ts:154-172`. `onUpdate` runs for every updated row, whatever field changed.

- The row is now a parent: `removeTransfer` (`:157-159`).
- The payee is a transfer payee and `transfer_id` is empty: `addTransfer` (`:161-163`).
- The payee is no longer a transfer payee and `transfer_id` is set: `removeTransfer` (`:165-167`).
- Both are set: `updateTransfer` (`:169-171`). It writes these fields to the counterpart (`:120-138`):
  - `account` = the transfer account
  - `payee` = the transfer payee of the source's current account, so moving the source to another account repoints the other side
  - `notes` = the source notes (**this overwrites the counterpart's own notes**)
  - `amount` = `-amount`
  - `schedule` = the source schedule

  Then `clearCategory` runs again.

`updateTransfer` does **not** sync these:

- **`date`.** The snapshot shows the source moving to 20170105 while the counterpart stays at 20170101 (`lc/server/transactions/__snapshots__/transfer.test.ts.snap`, "inserted/updated/deleted 3"; test at `lc/server/transactions/transfer.test.ts:87-95`). Date sync exists only in the desktop client and only behind the synced pref `sync-transfer-date`, which is off by default. The client sends a second batch with `{id, date}` for the counterpart(s), and for the counterpart's parent if the counterpart is a split child, with `runTransfers: false` (`dc/components/transactions/TransactionsTable.tsx:1131-1133`, `:1286-1327`; the pref is declared at `lc/types/prefs.ts:47`).
- **`cleared`**, **`reconciled`** and the counterpart's **`category`**, apart from the `clearCategory` case.

Changing the payee from transfer A to transfer B moves the existing counterpart into account B. It is not deleted and re-created (snapshot "inserted/updated/deleted 4": the counterpart's account changes from `two` to `three`).

The relationship is symmetric. Both rows carry a transfer payee and point at each other, so editing either side drives the other. Changing **either** side's payee to an ordinary payee tombstones the other side (`removeTransfer`).

**Deleting: `removeTransfer`** (`lc/server/transactions/transfer.ts:95-118`, called from `onDelete` at `:148-152`).

- It reads the counterpart through `v_transactions`, which shows live rows only. A counterpart that is already deleted is skipped.
- **If the counterpart is a split child**, it is not deleted. Instead it gets `transferred_id = null` and `description = null`, which turns it into an ordinary child (`:103-111`).
- Otherwise the counterpart is tombstoned (`:113`).
- Finally the row itself gets `transferred_id = null`. On the delete path this writes to the row that was just tombstoned (`:116`).

**Transfer payees.**

- **On account create:** `createAccount` inserts the account, then calls `db.insertPayee({ name: '', transfer_acct: id })` (`lc/server/accounts/app.ts:557-577`). `insertPayee` writes the payee row and a self-mapping `payee_mapping {id, targetId: id}` in one batch (`lc/server/db/index.ts:576-586`). The bank-link creators do the same (`lc/server/accounts/app.ts:215`, `:292`, `:371`, `:446`, `:527`). Only the columns `name` and `transfer_acct` are written. `learn_categories` and `favorite` come from the SQL defaults (1 and 0: `mig/1737158400000_add_learn_categories_to_payees.sql:3`, `mig/1720664867241_add_payee_favorite.sql:3`).
- **On account rename:** nothing is written to `payees`. The display name is `COALESCE(accounts.name, payees.name)` in `v_payees` and `getPayees` (`lc/server/aql/schema/index.ts:314-327`, `lc/server/db/index.ts:658-665`). `account-update` writes only `name`, `last_reconciled` and `account_group_id` (`lc/server/accounts/app.ts:95-111`).
- **Protection:** `deletePayee` refuses to delete a transfer payee (`lc/server/db/index.ts:588-606`). `mergePayees` filters transfer payees out of both the target and the sources (`lc/server/db/index.ts:626-630`).
- **On account close or delete:** see §4.9. A normal close leaves the payee in place. A forced close tombstones it with `deleteTransferPayee` (`lc/server/accounts/app.ts:667`). Deleting an account with no transactions leaves the payee alive, and `v_payees` hides it because the account is tombstoned (`lc/server/aql/schema/index.ts:323-326`).

**Category rules for transfers.**

- **On→on or off→off.** The server clears the category on the edited row, and on its counterpart when `transfer_id` is known (`lc/server/transactions/transfer.ts:32-43`). The register then shows "Transfer", or "Needs Repair" when a category is still set (`dc/components/transactions/TransactionsTable.tsx:1887-1903`).
- **On→off.** The server **leaves the category alone**. Nothing on the server requires one.
  - The on-budget side's category cell is editable. The cell is read-only only when the row is itself off-budget, or when the other account is on-budget (`dc/components/transactions/TransactionsTable.tsx:1365-1370`, `:1887`).
  - An uncategorized on→off row appears in the "uncategorized" list: on-budget account, `category` null, not a parent, and the transfer account is off-budget or absent (`dc/queries/index.ts:45-55`, `:125-135`).
  - The only place that **requires** a category is the Close Account modal, when the balance goes from on-budget to off-budget (`dc/components/modals/CloseAccountModal.tsx:33-40`, `:86-94`).
- **The off-budget side** (the counterpart in an off-budget account) never gets a category. `addTransfer` writes none. Any update carrying an off-budget `account` nulls it (`lc/server/transactions/index.ts:115-121`). The desktop client also sets `category = undefined` when a row moves into an off-budget account (`dc/components/transactions/TransactionsTable.tsx:1238-1245`).
- **Off→on.** The counterpart lands in the on-budget account with no category. It is editable there and counts as uncategorized until the user sets one. This follows from the same rules.
- **A transfer edited from on→off to on→on**, by changing the payee or the account, has its category cleared automatically on both sides. That is the `clearCategory` path through `updateTransfer` (`lc/server/transactions/transfer.ts:120-138`; snapshot "de-categorized 3" shows `category "1" → null`).
- **An account flipping between on and off budget.** No Actual UI path exists. `account-update` cannot write `offbudget` (`lc/server/accounts/app.ts:95-111`). Only `api/account-update` writes it raw, through `db.updateAccount`, with no cascade to transactions or categories (`lc/server/api.ts:706-710`). So Actual does nothing when an account flips.

### 4.3 Splits

**Columns:**

- The parent has `isParent = 1` and `isChild = 0`.
- Each child has `isChild = 1` and `parent_id = <parent id>`. `parent_id` was added by a migration (`mig/1608652596043_parent_field.sql:3-11`).
- Views treat `parent_id` as null unless `isChild = 1`, hide children whose `parent_id` is null, and **always return `category` = NULL for parents**, whatever the column holds (`lc/server/aql/schema/index.ts:384-402`).
- A child is "alive" only if its parent is not tombstoned (`lc/server/aql/schema/index.ts:404-411`).

**Amounts:**

- The parent's `amount` is the user's total. The children should sum to it.
- The server never enforces this. The client computes an `error` JSON on the parent, `{type:'SplitTransactionError', version:1, difference}`, or null when the split balances (`lc/shared/transactions.ts:21-29`, `:97-111`). The value is persisted in the `error` column as JSON (`lc/server/aql/schema/index.ts:47`).
- Balances and aggregates exclude parents (`isParent = 0`) (`lc/server/accounts/app.ts:142-170`, `lc/server/aql/schema/executors.ts:113-126`).

**Creating a split:**

- `splitTransaction` sets the parent's `is_parent = true` and `payee = null` (`lc/shared/transactions.ts:362-391`).
- Children come from `makeChild`. Each child inherits `account`, `date`, `cleared`, `reconciled` and `starting_balance_flag` from the parent, and also its `category` and `payee` unless given (`lc/shared/transactions.ts:33-59`).
- The diff touches only changed fields (`lc/shared/util.ts:11-32`, `:128-154`). The parent's old `category` column is therefore usually **left in place**. The views mask it, but it reappears if the transaction is un-split later. [unverified]
- When an existing transaction is edited, edits to the parent propagate `account`, `date`, `cleared` and `reconciled` to every child through `makeChild`. A child's payee changes only if it matched the parent's old payee (`lc/shared/transactions.ts:269-302`).

**Deleting:**

- Deleting the parent cascades on the server: `idsWithChildren` tombstones every child (`lc/server/transactions/index.ts:13-23`, `:55-57`).
- Deleting a child in the client:
  - If it was the only child, the parent becomes a normal row (`is_parent false`, `error null`).
  - Otherwise the remaining children are kept and `error` is recomputed (`lc/shared/transactions.ts:334-360`).
  - The parent never ends up with zero children while still marked as a parent: if every child goes, the parent is deleted (`lc/shared/transactions.ts:218-223`).
- "Make as non-split" re-homes children as standalone rows and fixes the parent amount (`lc/shared/transactions.ts:415-472`).

**sort_order:**

- Children get relative negative values: `-1`, `-2` from `makeEmptySplitSubtransactions`, then `prev - 1` for each added child (`lc/shared/transactions.ts:61-68`, `:249-267`). `realizeTempTransactions` keeps them (`lc/shared/transactions.ts:393-413`).
- Manual reordering (`moveSubtransactions`) rewrites the children evenly into the gap between the parent and the next sibling, using fractional REAL values (`lc/server/db/index.ts:1047-1092`).

**Can a split child be a transfer? Yes.**

- `addTransfer` refuses only `is_parent`. The comment says *"For split transactions, we should create transfers using child transactions"* (`lc/server/transactions/transfer.ts:48-54`).
- A child is not a parent, so `onInsert`/`onUpdate` treat it like any other row. It gets a real standalone counterpart in the other account, and `transferred_id` is set on both rows.
- Test "split transfers are retained on child transactions" (`lc/server/transactions/transfer.test.ts:172-219`): the snapshot shows child `id7` (`is_child 1`, `parent_id id5`, `transfer_id id8`) and counterpart `id8` (`is_child 0`, amount -2000, `transfer_id id7`).
- `removeTransfer` handles a child counterpart specially (`lc/server/transactions/transfer.ts:103-111`).
- The client's date-sync and reconcile checks look at `subtransactions[].transfer_id` (`dc/components/transactions/TransactionsTable.tsx:1287-1290`, `:1662-1665`).
- Category for a child transfer: the same `clearCategory` rule applies. A checking (on-budget) → loan (off-budget) principal child **keeps** its category. An on→on child loses it.
- Note for Fred: an on-budget row with a category counts in that category's budget activity (`sum-amount` sums `v_transactions_internal_alive` rows in on-budget accounts by mapped category, `lc/server/budget/base.ts:133-149`). So in Actual the principal child looks like spending in its category.

### 4.4 Category delete

`category-delete`, an undoable handler (`lc/server/budget/app.ts:153`, `:378-422`), runs in one batch:

1. It validates that the row and `transferId` both exist, and **rejects a transfer between income and expense** (`:386-410`).
2. If the category is an expense category and `transferId` is set, it calls `budget.doTransfer([id], transferId)`. For **every created month**, the target's budget becomes `sum(deleted budgets) + target budget` (`lc/server/budget/base.ts:305-326`). The deleted category's own budget rows are **not** zeroed or tombstoned. They just stop being read.
3. `db.deleteCategory` (`lc/server/db/index.ts:538-562`):
   - If `transferId` is set, every `category_mapping` row whose `transferId` equals the deleted id is repointed to the new target. This forwards chains of earlier deletes.
   - Then the deleted category's own mapping is set: `category_mapping {id, transferId}`.
   - Finally the category is **tombstoned** (`delete_`). It is never hard-deleted.
4. **Transaction rows are never touched.** `transactions.category` keeps the old id. Every read resolves it through `LEFT JOIN category_mapping cm ON cm.id = _.category`, which yields `cm.transferId` (`lc/server/aql/schema/index.ts:394-397`). The budget sheet also maps transactions through `category_mapping` (`lc/server/sync/index.ts:189-198`) and recomputes both the old and new target when a mapping changes (`lc/server/budget/base.ts:202-214`).
5. **Without `transferId`:** the mapping stays a self-map and the category is tombstoned. `v_transactions` then returns `category = NULL` through the `tombstone = 0` join, so the transactions become uncategorized (`lc/server/aql/schema/index.ts:421-430`). The UI forces a transfer when `must-category-transfer` is true, meaning the category has transactions or any non-zero budget (`lc/server/budget/app.ts:494-521`, `dc/hooks/useDeleteCategoryFlow.ts:21-55`). The API passes `transferCategoryId` through as optional (`lc/server/api.ts:839-848`).

**Group delete** (`lc/server/budget/app.ts:471-492`):

- `doTransfer` runs over the group's live categories into `transferId`. There is **no income/expense check** here.
- `db.deleteCategoryGroup` then calls `deleteCategory(cat, transferId)` for **every** category with that `cat_group`, tombstoned ones included because the query has no tombstone filter, and tombstones the group (`lc/server/db/index.ts:436-448`).

**Verdict: this confirms D9's hypothesis.** Deleting a category with a transfer target re-files **all history at read time**, across every month, into the target. The budget amounts are merged into the target too. Archiving means `hidden = 1` (`mig/1685007876842_add_category_hidden.sql:3`), and that is the non-destructive alternative.

### 4.5 Payee merge, mappings and `learn_categories`

**Merge** (`lc/server/db/index.ts:618-656`, called from `lc/server/payees/app.ts:93-101`):

1. If the target is a transfer payee, return. Transfer payees are removed from the sources.
2. In one batch, for each source id: repoint every `payee_mapping` row with `targetId = source` to `target` (chains). Then set `payee_mapping {id: source, targetId: target}` and tombstone the source payee.
3. Transaction rows keep their old `description`. Views map it with `payee = pm.targetId` (`lc/server/aql/schema/index.ts:389`, `:397`).
4. Rules are not rewritten in the DB. The in-memory copies are remapped through `migrateIds` whenever a mapping table changes (`lc/server/transactions/transaction-rules.ts:284-291`, `lc/server/rules/rule-utils.ts:112-142`, `lc/server/db/mappings.ts:22-59`). Veritas must apply the same mapping when it evaluates rules.

**Delete and insert:**

- Deleting a payee only tombstones it. The mapping is left as is (`lc/server/db/index.ts:588-606`). `v_transactions` shows `payee = NULL` for a tombstoned target (`lc/server/aql/schema/index.ts:425`).
- Every new payee gets a self-mapping (`lc/server/db/index.ts:576-586`).
- Every new category also gets `category_mapping {id, transferId: id}` (`lc/server/db/index.ts:497-499`).

**`learn_categories` is rules-based.** Actual has no "last category" column.

- Trigger: `batchUpdateTransactions({learnCategories: true})` calls `updateCategoryRules` for both **added and updated** transactions that carry a category (`lc/server/transactions/index.ts:174-186`, `lc/server/transactions/transaction-rules.ts:952-1047`). The desktop table and mobile pass the synced pref `learn-categories`, which defaults to `'true'` (`dc/components/transactions/TransactionList.tsx:182-197`, `:212`; `dc/components/mobile/transactions/TransactionEdit.tsx:1625`, `:1918`). `addTransactions` and the API's `transactions-add` default it to false (`lc/server/accounts/sync.ts:1042`, `lc/server/api.ts:610-622`).
- Window: for each payee of the saved rows, read `v_transactions` from 180 days before the oldest saved date to 180 days after today, sorted by date DESC. Keep rows that are not parents, are in open accounts, and whose payee has `learn_categories = 1` (`lc/server/transactions/transaction-rules.ts:977-984`; closed accounts excluded at `:981`). Take that payee's **5 latest rows**, and continue only if one of them was just saved (`:989-1003`).
- `getProbableCategory` counts categories. The winner is the first row, in date-DESC order, with a strictly higher count, and it needs **≥ 3** (`:932-950`). Quirk: if the newest of the 5 has no category, the winner's score is `undefined`, nothing can beat it, and no category is learned.
- Rules written (`:1005-1046`):
  - **Every** existing default-stage rule with exactly one `payee is|isNot <payee>` condition and one `set category` action gets the new category (`getIsSetterRules`, `:841-867`, `:1007-1028`).
  - Otherwise, unless a `payee oneOf […]` rule already sets that category, insert `{stage:null, conditionsOp:'and', conditions:[{op:'is',field:'payee',value}], actions:[{op:'set',field:'category',value}]}` (`:1029-1044`).
- The category then appears on the next manual add, because the client runs rules on new rows (§6.2).
- Payee merge rename (`updatePayeeRenameRule`, `lc/server/transactions/transaction-rules.ts:896-930`, called from `dc/components/modals/MergeUnusedPayeesModal.tsx:68`): extend an existing pre-stage rule shaped `imported_payee oneOf […] → set payee <to>`, or create one.
- Actual has **no "last amount" memory**. Mobile only defaults the date and account from the last transaction (`dc/components/mobile/transactions/TransactionEdit.tsx:1705-1712`).

### 4.6 `sort_order` conventions

Every `sort_order` is REAL: `transactions` (`lc/server/sql/init.sql:55`), `categories`/`category_groups` (`:63`, `:70`), `accounts` (`mig/1616167010796_accounts_order.sql:3`). Constants: `SORT_INCREMENT = 16384` and `TRANSACTION_SORT_INCREMENT = 1024` (`lc/server/db/sort.ts:1-5`).

**Transactions:**

- The schema default is `Date.now()` in epoch milliseconds (`lc/server/aql/schema/index.ts:51`).
- The desktop client sets `sort_order = Date.now()` on new parents (`realizeTempTransactions`, `lc/shared/transactions.ts:396-400`) and **whenever the date changes** (`dc/components/transactions/TransactionList.tsx:209-211`).
- Imports use `now - index*1024` (`lc/server/accounts/sync.ts:782-785`, `:1084-1086`).
- Manual moves use `shoveSortOrdersDescending` within account and date (children within their parent). That means a midpoint rounded with `Math.round`, or shoving neighbours down when the gap is 2 or less (`lc/server/db/index.ts:929-998`, `lc/server/db/sort.ts:78-189`).
- The list is displayed in descending `sort_order`.

**Categories** (`lc/server/db/index.ts:450-503`):

- A new category is inserted **at the top of its group**: `shoveSortOrders(groupCats, firstId)`. The new value is `first.sort_order / 2`, with neighbours shoved if the gap is 2 or less, or `16384` for an empty group.
- With the `atEnd` option, the value is `max(sort_order over ALL live categories) + 16384`. That maximum is global, not per group.
- Moves use the `shoveSortOrders` midpoint `(below+above)/2`, unrounded (`lc/server/db/sort.ts:7-67`, `lc/server/db/index.ts:517-536`).

**Category groups and account groups:** a new row gets `max(live sort_order) + 16384`. Moves use the same midpoint (`lc/server/db/index.ts:378-383`, `:421-434`, `:819-823`).

**Accounts:**

- A new account is appended among accounts with the same `offbudget` value: last + 16384, or 16384 when there are none. The query has **no tombstone or closed filter** (`lc/server/db/index.ts:744-755`).
- Moves scope to closed accounts, or to live accounts with the same `offbudget` value (`:766-800`).

### 4.7 Reconcile

- **`cleared`** is an INTEGER with SQL default 1 (`mig/1582384163573_cleared.sql:3`). **`reconciled`** is an INTEGER with default 0 (`mig/1697046240000_add_reconciled.sql:3`).
- The register status is `reconciled`, else `cleared`, else none (`dc/components/transactions/TransactionsTable.tsx:2080-2100`).
- The cleared balance is the sum over `cleared = true` with `splits: 'none'` (`dc/accounts/reconciliation.ts:71-80`).

**Finishing a reconcile** (`dc/accounts/reconciliation.ts:82-93`):

- **Locking runs only if** `statement - cleared === 0`. It sets `reconciled = true` on every `cleared && !reconciled` row in the account. The change goes through `updateTransaction`, so a parent's children inherit it (`dc/accounts/reconciliation.ts:18-45`).
- `accounts.last_reconciled = String(Date.now())` is written either way (`dc/components/accounts/Account.tsx:1107-1129`).

**What "locked" means.** None of this is enforced by the server.

- Editing amount, payee, account or date on a reconciled row, or on a row whose transfer counterpart is reconciled, shows a confirmation (`dc/components/transactions/TransactionsTable.tsx:1146-1210`).
- Clicking the status icon offers to unlock, which sets `reconciled = false` and leaves `cleared` alone (`:1212-1227`).
- Moving the row to another account sets `reconciled = false` (`:1260-1262`).
- Batch "cleared" edits skip reconciled rows (`dc/hooks/useTransactionBatchActions.ts:107-118`).
- Import and bank sync never update a matched reconciled row (`lc/server/accounts/sync.ts:680-685`).

**Adjustment transaction** (`dc/accounts/reconciliation.ts:95-124`):

1. Create a temp row `{account, cleared: true, reconciled: false, amount: diff, date: today, notes: t('Reconciliation balance adjustment')}`. It has **no payee and no category**. The notes string is localized.
2. Realize it, which gives it a new uuid and `sort_order = Date.now()`.
3. **Run `rules-run` on it.**
4. Send it through `transactions-batch-update` (`added`).

In an on-budget account it therefore starts uncategorized unless a rule sets a category.

### 4.8 Starting balance on account creation

Source: `lc/server/accounts/app.ts:557-594`. Writes happen in this order:

1. `insertAccount {name, offbudget, closed, sort_order}`.
2. Insert the transfer payee, with its mapping.
3. If `balance != 0`, call `getStartingBalancePayee()`:
   - Find the category `is_income = 1 AND LOWER(name) = 'starting balances'`, else the first live income category, else null (`lc/server/accounts/payees.ts:18-36`).
   - Reuse a live payee named "Starting Balance" (case-insensitive), or create one with a mapping (`lc/server/accounts/payees.ts:3-16`).
4. Call `db.insertTransaction` **directly**, with no rules, no transfer hook and no learn step:
   - `account`
   - `amount = amountToInteger(balance)` (the handler takes decimal units: `lc/shared/util.ts:541-547`; the API converts its integer input, `lc/server/api.ts:690-703`)
   - `category = offBudget ? null : startingCategory`
   - `payee`
   - `date = today`
   - `cleared = true`
   - `starting_balance_flag = true`

   Schema defaults add `sort_order = Date.now()` and `reconciled = 0`.

The bundled new-budget DB already contains an income category "Starting Balances" in the "Income" group (`reference/actual/packages/loot-core/default-db.sqlite`, checked with `sqlite3`). `createBudget` copies that file (`lc/server/budgetfiles/app.ts:478-479`). `starting_balance_flag` also comes second in the `v_transactions` ordering (`lc/server/aql/schema/index.ts:429`).

### 4.9 Account close

Source: `lc/server/accounts/app.ts:596-706`. The handler is `mutator` without `undoable`. It wraps `withUndo` itself.

1. Unlink bank sync (outside undo).
2. If the account is missing or already closed, return.
3. Compute `balance` (`isParent=0`, live rows) and `numTransactions` (live rows) (`lc/server/accounts/app.ts:156-170`).
4. Then branch:
   - **`numTransactions === 0`:** tombstone the account. This is a delete, not a close. The transfer payee is left in place and hidden by `v_payees` (`:627-628`).
   - **`forced`:** in one batch, for every row in the account:
     - if it has a counterpart, set the counterpart's `description = null` and `transferred_id = null`;
     - tombstone the row.

     Then tombstone the account and its transfer payee (`:629-668`).
   - **Normal close:**
     - A zero balance is **not required**. If `balance != 0`, `transferAccountId` is required (APIError), and it can't be the account itself (`:670-676`).
     - Write `accounts.closed = 1` **first** (`:678`).
     - If the balance is not zero, add a transaction through `transaction-add`: `{id, payee: <transfer payee of target>, amount: -balance, account: closingAcct, date: today, notes: 'Closing account', category: categoryId}` (`:682-703`). That path runs the transfer hook, which creates the counterpart in the target and applies the category-clear rule. It does not run rules.
     - The UI requires `categoryId` only for on-budget → off-budget (`dc/components/modals/CloseAccountModal.tsx:33-40`, `:86-94`).
5. Reopening writes only `closed = 0` (`lc/server/accounts/app.ts:708-710`).
6. Closed accounts are excluded from category learning (`lc/server/transactions/transaction-rules.ts:981`).

---

## 5. Envelope budget math, goals and arithmetic

### 5.1 The cell engine

Budget type and tables are in §3.11.

- Every month is a "sheet" named `budget` + `YYYYMM` (`lc/shared/months.ts:434-436`). Cells are either static values or dynamic formulas over other cells (`lc/server/spreadsheet/spreadsheet.ts:379-443`). A dynamic cell with no `run` behaves as a static value (`spreadsheet.ts:165-186`).
- Coercion inside formulas is `number(v)`: a number stays as is, a string goes through `parseFloat` (NaN becomes 0), and anything else (null, undefined, bool) becomes 0 (`lc/server/spreadsheet/globals.ts:1-13`). Every arithmetic result goes through `safeNumber`, which throws unless the value is an integer with |v| ≤ 2^51−1 (`lc/shared/util.ts:426-441`). A throw stops the whole recompute queue (`spreadsheet.ts:187-193`).
- A cell that was never created reads as `null`, which counts as 0 or false in formulas.

### 5.2 Which months exist ("created months")

`createAllBudgets` (`lc/server/budget/base.ts:422-447`), run on every budget load (`lc/server/budgetfiles/app.ts:693`) and on `get-budget-bounds` (`lc/server/budget/app.ts:205-207`, which desktop calls on budget page mount, `dc/components/budget/index.tsx:61,79`):

```
earliest = SELECT * FROM transactions WHERE isChild=0 AND date IS NOT NULL ORDER BY date ASC LIMIT 1
           -- no tombstone filter, no account filter (base.ts:423-425)
s = month(earliest.date) or currentMonth()
e = currentMonth()
if s > e: s = e                       (base.ts:27-29)
start = s − 3 months; end = e + 12 months   (base.ts:35-36)
created months = rangeInclusive(start, end)
```

- **The first month is `start`.** The month before it is a "blank sheet" where every cell is a static 0 or false: `to-budget=0`, `buffered=0`, and for each category `carryover=false`, `leftover=0`, `leftover-pos=0` (`lc/server/budget/envelope.ts:11-31, 228-238`). `buffered-selected` is *not* created on the blank sheet, so it reads null (0).
- **Budget rows outside `[start, end]` are ignored.** They get loaded into cells that no formula depends on (`lc/server/sheet.ts:231-255`).
- The set of created months lives in memory only (`sheet.meta().createdMonths`, `base.ts:336,402`). The `created_budgets` table exists (`lc/server/sql/init.sql:1`) but no server code reads or writes it. Only `lc/mocks/setup.ts:99` mentions it.
- The range depends on today's date, so **the oracle and Veritas must pin "current month"** for differential tests.

### 5.3 Category universe

`createBudget` uses `aqlQuery(q('category_groups').select('*'))` (`base.ts:328-332`):

- The AQL executor drops tombstoned groups and categories (`lc/server/aql/compiler.ts:27-33`, `lc/server/aql/schema/executors.ts:313-327`).
- **Hidden categories and hidden groups are included.** No hidden filter is applied. Actual's own test confirms this: `lc/server/budget/base.test.ts:231,283`.
- Groups come back ordered by `is_income, sort_order, id` (`lc/server/aql/schema/index.ts:286-287`).
- A category counts as **expense** when `cat.is_income` is falsy and as **income** when it is truthy (`envelope.ts:34,99-101`).
- Live categories in a tombstoned group get no cells.

### 5.4 Per-category cells (month M, previous month P = M−1)

**`sum-amount-<c>` ("spent" or "received")** (`base.ts:133-150`, cold-build seed `base.ts:69-92`):

```sql
SELECT SUM(amount) FROM v_transactions_internal_alive t
  LEFT JOIN accounts a ON a.id = t.account
 WHERE t.date >= YYYYMM01 AND t.date <= YYYYMM<last> AND category = '<c>' AND a.offbudget = 0
-- NULL → 0
```

The view definitions are at `lc/server/aql/schema/index.ts:385-412`. They give these rules:

- `category` is `NULL` for split **parents** (`isParent=1`), so parents never count and **children count** (`index.ts:389`). A child counts only if `parent_id` is non-null and its parent row exists with `tombstone = 0` (`index.ts:401, 408-412`).
- `category` is resolved through `category_mapping.transferId`, so a merged or deleted-with-transfer category's transactions land on the target. A transaction whose category id has no `category_mapping` row gets NULL and is not counted anywhere (`index.ts:389,396`, mapping rows written at `lc/server/db/index.ts:499,551,558`).
- Excluded: tombstoned transactions (`IFNULL(tombstone,0)=0`), rows with `date IS NULL` or `acct IS NULL`.
- **Off-budget accounts are excluded** (`a.offbudget = 0`). So are rows whose account row is missing (`NULL = 0` is false). Closed accounts count. The account's tombstone is *not* checked.
- **Transfers get no special case.** An on↔on transfer normally has a NULL category and is therefore excluded. An on→off transfer carries a category and counts.
- Uncategorized transactions (NULL category) count nowhere: not in spent, not in income, not in to-budget.
- Starting-balance transactions count when they are categorized (normally to an income category, which makes them income).
- If the category's mapping target is a tombstoned category, its transactions count nowhere, because that category has no cells.

**Expense categories only** (`envelope.ts:33-74`):

| cell | formula |
|---|---|
| `budget-<c>` | `zero_budgets.amount` for (`month=YYYYMM`, `category=c`). No row → 0. NULL amount → coerced to 0 (`envelope.ts:35-44`, loaded at `sheet.ts:241-244`, live-updated at `base.ts:221-238`) |
| `carryover-<c>` | `zero_budgets.carryover === 1` for that month's row. No row → false (`envelope.ts:46`, `sheet.ts:245-248`) |
| `leftover-<c>` (**balance / "available"**) | `budget[M] + spent[M] + (carryover[P] ? leftover[P] : leftover-pos[P])` (`envelope.ts:48-64`) |
| `leftover-pos-<c>` | `leftover < 0 ? 0 : leftover` (`envelope.ts:66-72`) |
| `goal-<c>`, `long-goal-<c>` | raw `zero_budgets.goal`, `zero_budgets.long_goal` (null when unset). Display only (§5.9) (`sheet.ts:249-253`, `base.ts:233-236`) |

`spent` is signed: expenses are negative, refunds positive.

**Rollover semantics:**

- A positive balance always carries into next month.
- A negative balance carries only when **that month's** `carryover` flag is on. Otherwise the category starts next month at 0, and the deficit is charged to next month's To Budget through `last-month-overspent`.
- The flag stored on month P decides what happens at the P→M boundary.

**Income categories** have only `sum-amount-<c>`. If a `zero_budgets` row exists for them, `carryover-<c>` is also loaded and means "auto hold" (see `buffered-auto`). Income categories get no `budget-` or `leftover-` cells in envelope mode.

### 5.5 Group cells (`envelope.ts:76-96`)

- `group-sum-amount-<g>` = Σ `sum-amount-<c>` for c in g (both income and expense groups).
- Expense groups only:
  - `group-budget-<g>` = Σ `budget-<c>`.
  - `group-leftover-<g>` = Σ `leftover-<c>`.

### 5.6 Month cells (`envelope.ts:98-226`)

```
incomeGroup          = first group with is_income (order: is_income, sort_order, id)      (envelope.ts:99)
total-income         = group-sum-amount[incomeGroup]                                         (116-120)
from-last-month      = to-budget[P] + buffered-selected[P]                                    (105-113)
available-funds      = total-income + from-last-month                                         (122-127)
last-month-overspent = Σ over expense cats c: (carryover[P,c] ? 0 : min(0, leftover[P,c]))   (129-148)
total-budgeted       = −Σ over expense groups g: group-budget[g]                              (150-159)
buffered             = zero_budget_months.buffered where id = 'YYYY-MM', else 0               (103,161; sheet.ts:258-266; base.ts:216-219)
buffered-auto        = Σ over income cats c: (carryover[M,c] ? sum-amount[M,c] : 0)          (162-181)
buffered-selected    = (buffered !== 0) ? buffered : buffered-auto                            (182-191)   -- UI "For next month"
to-budget            = available-funds + last-month-overspent + total-budgeted − buffered-selected   (193-209)
total-spent          = Σ over expense groups: group-sum-amount                                (211-217)
total-leftover       = Σ over expense groups: group-leftover                                  (219-225)
```

Notes:

- **Negative To Budget carries forward** through `from-last-month`.
- Held money (`buffered-selected`) leaves this month's To Budget and comes back next month.
- `buffered-selected` uses strict `!== 0`. If `zero_budget_months.buffered` is SQL NULL, the result is null, which counts as 0 in `to-budget`, and the auto hold is ignored.
- **Only the first income group** feeds `total-income`. A second income group's categories are ignored.
- If there is no income group at all, `incomeGroup.id` throws.
- UI labels map to cells as follows (`dc/spreadsheet/bindings.ts:170-196`, `dc/components/budget/envelope/budgetsummary/TotalsList.tsx:97-158`):
  - "Available funds" = `available-funds`
  - "Overspent in <prev>" = `last-month-overspent`
  - "Budgeted" = `total-budgeted`
  - "For next month" = `buffered-selected`, displayed with inverted sign
- **Oracle caveat:** the API's `budget-month` returns `forNextMonth: value('buffered')`, the manual hold only, not `buffered-selected` (`lc/server/api.ts:474`). Every income group's `received` is `total-income` (`api.ts:502-505`). The API does not expose `buffered-auto` or `buffered-selected`.

**`zero_budget_months` and the hold**, beyond the formulas above:

- `buffered` is integer cents. Only envelope budgets read the table (`lc/server/sheet.ts:257-266`).
- `setBuffer(month, amount)` updates the row if it exists, otherwise inserts it (`lc/server/budget/actions.ts:179-191`).
- `holdForNextMonth` stores `calcBufferedAmount = buffered + clamp(amount, −buffered, max(toBudget, 0))`, so the buffer never goes negative and never exceeds what is available to budget (`lc/server/budget/actions.ts:34-41`, `:486-512`).
- The month before the first budget month gets static `to-budget = 0` and `buffered = 0` (`lc/server/budget/envelope.ts:25-31`).
- Changes to `zero_budget_months` rows reach the spreadsheet through `triggerBudgetChanges` → `handleBudgetMonthChange` (`lc/server/budget/base.ts:252-253`).

### 5.7 Worked recurrence for one expense category (for the Rust port)

```
for M in created months ascending:
  L[M]   = B[M] + S[M] + (CF[P] ? L[P] : max(0, L[P]))      # L[blank] = 0
  OS[M]  = Σc (CF[P,c] ? 0 : min(0, L[P,c]))
  TB[M]  = Inc[M] + TB[P] + BS[P] + OS[M] − ΣB[M] − BS[M]   # TB[blank] = 0, BS[blank] = 0
```

Here B = budgeted, S = spent, CF = carryover flag, BS = buffered-selected, ΣB = sum of expense budgets, Inc = total income.

### 5.8 Budget mutations and what they write

Rows land as column-wise upserts (§3.5). Untouched columns keep their DB defaults: `zero_budgets` `amount 0`, `carryover 0`, `goal NULL`, `long_goal NULL`; `zero_budget_months` `buffered 0`. New `zero_budgets` rows get the id `"<YYYYMM>-<categoryId>"` (`lc/server/budget/actions.ts:148`, `:171`, `:209`), rows migrated from legacy files have `"<YYYY-MM>-<categoryId>"` (`mig/1632571489012_remove_cache.js:71`), and lookups always go by `(month, category)` (`lc/server/budget/actions.ts:119-122`, `:138-143`). Veritas matches the same way and keeps the existing id when updating.

All actions are registered at `lc/server/budget/app.ts:74-144` and dispatched from the client at `dc/budget/mutations.ts:780-900`.

| Action (handler) | Exact behavior | Writes |
|---|---|---|
| set budget amount (`budget/budget-amount`, `actions.ts:126-153`) | `amount = safeNumber(typeof amount==='number' ? amount : 0)` (throws if non-integer or above 2^51). Upsert for (month, category). | `zero_budgets.amount` (insert also sets `month`, `category`) |
| copy last month (`budget/copy-previous-month`, `actions.ts:218-242`) | For each `zero_budgets` row of P whose category is alive: skip income cats (envelope), skip if `c.hidden=1` or `group.hidden=1`, then `setBudget(cat, M, prev.amount)`. Categories with no row in P are left untouched (not zeroed). | `zero_budgets.amount` |
| copy single category (`budget/copy-single-month`, `actions.ts:244-259`) | `setBudget(cat, M, budget-cell[P])` | `zero_budgets.amount` |
| set zero (`budget/set-zero`, `actions.ts:261-274`) | Every `v_categories` row with `tombstone=0` (hidden ones too), skipping income: amount 0 | `zero_budgets.amount` |
| set 3/6/12-month average (`actions.ts:276-357`, single category `actions.ts:359-386`) | Bulk version covers cats with `c.tombstone=0 AND c.hidden=0 AND g.hidden=0`, skipping income. `avg = getCategoryAverage(M, N, cat)`. Expense cats get `avg *= −1`, then setBudget. The single-category variant does not skip income. | `zero_budgets.amount` |
| ↳ `getCategoryAverage` (`actions.ts:388-484`) | `first = prev(M)`, or `prev(currentMonth)` if `prev(M) ≥ currentMonth`. `firstActivity = MIN(month)` over the cat's budget rows (month ≤ first) ∪ its on-budget alive transactions (month ≤ first). Walk back from `first` for up to N months, stopping before `firstActivity` if it is non-null. Result `Math.round(Σ sum-amount / count)`, or 0 if there are no months. | — |
| copy until year end (`actions.ts:679-702`) | `amount = budget[M,cat]`; for created months `m` with `M < m ≤ YYYY-12`: setBudget | `zero_budgets.amount` |
| hold for next month (`budget/hold-for-next-month`, `actions.ts:486-512`, clamp `34-41`) | `b = zero_budget_months.buffered` (or 0), `T = to-budget[M]`. If `T ≤ 0`: do nothing, return false. Else `buffered = b + min(max(amount, −b), max(T,0))`. Holds are **additive**. | `zero_budget_months.buffered` (`id='YYYY-MM'`, `actions.ts:179-191`) |
| reset hold (`actions.ts:514-516`) | `buffered = 0` | `zero_budget_months.buffered` |
| transfer available (To Budget → category, "Fund goal") (`actions.ts:578-593`) | `amt = max(min(amount, to-budget[M]), 0)`; `budget[cat] += amt` | `zero_budgets.amount`. **No note written.** |
| move budget between categories (`budget/transfer-category`, `actions.ts:643-677`) | `budget[from] −= amount` (no clamping, so negative amounts work). If `to !== 'to-budget'`, `budget[to] += amount`. Then appends a movement note. | `zero_budgets.amount` ×1–2, `notes` |
| cover overspending (`budget/cover-overspending`, `actions.ts:518-576`) | `need = amount ? −amount : leftover[to]` (UI passes a positive amount; 0 or undefined means "all"). `src = from==='to-budget' ? to-budget[M] : leftover[from]`. If `need ≥ 0` or `src ≤ 0`: do nothing. Else `x = min(\|need\|, src)`; if `from !== 'to-budget'`, `budget[from] −= x`; `budget[to] += x`; append note. | `zero_budgets.amount`, `notes` |
| cover overbudgeted (To Budget < 0, from a category) (`actions.ts:595-641`) | `need = amount ? −amount : to-budget[M]`. If `need ≥ 0` or `leftover[cat] ≤ 0`: do nothing. `x = min(\|need\|, leftover[cat])`; `budget[cat] −= x`; note "→ Overbudgeted" | `zero_budgets.amount`, `notes` |
| set carryover (rollover overspending) (`budget/set-carryover`, `actions.ts:704-721`, helper `193-214`) | For **every month from `startMonth` to the latest created month** (`getAllMonths`, `actions.ts:98-107`), upsert `carryover = flag?1:0`. Months created later (as time passes) have no row, so they are **false** and the flag silently lapses after about 12 months. | `zero_budgets.carryover` (rows inserted with `amount` default 0) |
| reset income carryover (`actions.ts:785-800`) | For every live income category, `carryover=0` for **month M only** | `zero_budgets.carryover` |
| income "auto hold" toggle (UI, `dc/components/budget/envelope/IncomeMenu.tsx:38-45`) | Turning it on first calls `reset-hold(M)`, then `set-carryover(M→end, incomeCat, true)` | as above |
| UI "Hold for next month" (`dc/components/budget/envelope/budgetsummary/ToBudgetMenu.tsx:45-52,98-101`) | Offered only when `buffered-auto === 0 && to-budget > 0`. Calls `reset-income-carryover(M)` and then `hold`. | as above |

**The movement note** (`actions.ts:723-783`):

- It goes to `notes` row `id = 'budget-YYYY-MM'`.
- New text is `existing + "\n" + "- Reassigned <integerToCurrency(x)> from <FromName|To Budget> → <ToName|To Budget|Overbudgeted> on <MMMM dd>"`. With no existing note there is no leading newline.
- The date in the text is today's date, not the budget month.

UI input conversions for these menus:

- CoverMenu evaluates its text with `evalArithmetic` and submits nothing when the result is 0 or null (`dc/components/budget/envelope/CoverMenu.tsx:56-65`).
- TransferMenu submits only when the amount is > 0 (`dc/components/budget/envelope/TransferMenu.tsx:50-60`).
- "To Budget" appears as a pseudo-category `id='to-budget'` (`dc/components/budget/util.ts:24-39`).

### 5.9 Goals and templates

**Storage:**

- `categories.goal_def TEXT` (`mig/1694438752000_add_goal_targets.sql:5`) holds the JSON array of parsed templates (`lc/server/budget/goal-template.ts:52`).
- `categories.template_settings JSON DEFAULT '{"source": "notes"}'` (`mig/1754611200000_add_category_template_settings.sql:3`).
- `zero_budgets.goal` and `long_goal` are written per month only when templates are applied (`goal-template.ts:198-209, 340-347`, `actions.ts:155-177`).

**Where templates come from:**

- When `source` ≠ `'ui'`, the category note (`notes.id = category id`) is the source of truth.
- Every template apply first runs `storeNoteTemplates` (`goal-template.ts:76,87,105,126`). It re-parses notes containing `#template` or `#goal` into `goal_def` (`lc/server/budget/template-notes.ts:17-25,72-159`, `lc/server/budget/statements.ts:34-57`).
- It also **sets `goal_def = NULL` for every non-`'ui'` category whose note lacks `#template` or `#goal`** (`statements.ts:6-26`).
- A template line is recognized when the text from the first `#` starts with the literal `#template` or `#goal`. That check is case-sensitive (`template-notes.ts:90-93`).
- When `source = 'ui'`, `goal_def` is authoritative and notes are ignored for that category.

**Long-term goal grammar:**

- `'#goal'i amount`, where `amount = currencySymbol? _? '-'?\d+('.'\d\d?)?`, in major units with no thousands separators (`lc/server/budget/goal-template.pegjs:29,90,96`).
- It parses to `{type:'goal', amount, priority:null, directive:'goal'}` (`goal-template.pegjs:29`, test `lc/server/budget/template-notes.test.ts:107-117`).
- Only one `#goal` is allowed per category (`lc/server/budget/category-template-context.ts:664-666`).
- On apply, a goal-only category keeps its budgeted amount and writes `goal = amountToInteger(amount, currency dp)` and `long_goal = 1` (`category-template-context.ts:111-118, 457-468`, `goal-template.ts:343`).
- Apply without overwrite only processes categories whose budgeted amount is 0 (`goal-template.ts:249`). A goal on a funded category is written only by "overwrite" or by single-category apply (force).
- Categories without templates that still have a goal get `goal`/`long_goal` reset to NULL for that month (`goal-template.ts:270-276`).

**Display** (`dc/components/budget/GoalStatus.tsx:36-50`, colors `dc/components/budget/BalanceWithCarryover.tsx:92-103`, `dc/components/budget/util.ts:71-104`):

- It only renders when the experimental flag `flags.goalTemplatesEnabled` (a preferences row, value `'true'`) is on. The default is **false** (`dc/hooks/useFeatureFlag.ts:5-27`).
- It also requires `goal-<c>` (that month's `zero_budgets.goal`) to be non-null.
- `have = long_goal===1 ? balance(leftover) : budgeted`, `diff = have − goal`, `progress = diff<0 ? floor(max(0, have/goal)*100) : 100`.
- Labels: "Fully funded" / "Overfunded (x)" / "Underfunded (x)". Goal type reads "Goal" when long_goal=1, otherwise "Automation".
- Goal values are stored **per month**, so a month where templates were never applied shows nothing.

**Can a Veritas goal be expressed natively?**

- **Target amount: yes, as a long-term goal.** Any of these work:
  1. Note line `#goal 1000` (or `#goal 1234.56`), keeping `template_settings` at `{"source":"notes"}`.
  2. `goal_def = [{"type":"goal","amount":1000,"priority":null,"directive":"goal"}]` with `template_settings = {"source":"ui"}`. Without `"ui"`, Actual nulls `goal_def` on the next apply unless the note also has the line.
  3. For immediate display, a `zero_budgets` row per month with `goal = 100000` (minor units) and `long_goal = 1`. This is exactly what Actual's apply writes. Actual's progress then equals the envelope balance, which matches R-GOAL-2.
- **Target date: no.** `GoalTemplate` has no date (`lc/types/models/templates.ts:101-105`). The nearest construct is `#template 1000 by 2027-06`. That is a monthly-funding *automation*: it budgets `round((target − alreadyBudgeted)/(monthsLeft+1))` (`category-template-context.ts:836-848`), displays a monthly goal against budgeted, and has long_goal null. Its semantics differ, and it only does anything when the user applies templates. The date should stay in `veritas_*`.
- In every case Actual shows nothing unless Ryan turns on the experimental flag.

### 5.10 Arithmetic: `evalArithmetic(expression, defaultValue = null)` (`lc/shared/arithmetic.ts:1-170`)

1. If `expression === ''`, return `defaultValue` (`arithmetic.ts:156-158`).
2. Strip **all JS `\s`** characters (this includes U+00A0 and U+202F) before parsing (`arithmetic.ts:124`).
3. Grammar, a recursive descent with no end-of-input check (`arithmetic.ts:47-121`):

```
additive       := multiplicative (('+'|'-') multiplicative)*        left-assoc
multiplicative := exponent (('*'|'/') exponent)*                    left-assoc
exponent       := parens ('^' exponent)?                            RIGHT-assoc
parens         := '(' additive ')' | primary                         missing ')' → error
primary        := '-'? NUMCHARS+                                     unary minus only directly before a number
NUMCHARS       := [0-9 , . ' U+2019 U+00A0 U+202F space] | \p{Sc}    (arithmetic.ts:58)
```

4. Number token value: `currencyToAmount(token)`. If that returns null, it is an error ("Invalid number format"). An empty token is also an error ("Unexpected character") (`arithmetic.ts:67-75`).
5. Evaluation uses f64 `+ − * /` and `Math.pow` (`arithmetic.ts:128-149`).
6. Any thrown error returns `defaultValue`. A `NaN` result returns `defaultValue` (`arithmetic.ts:160-169`).
7. **`Infinity` is returned as is** (e.g. `1/0`), and `0/0` gives the default.

**Consequences:**

- Trailing junk is ignored: `1+2)foo` = 3, `1+2(3` = 3, `5%` = 5, `1e5` = 1.
- A leading `+` fails. So do `-(5)`, `--5`, `$-5` and `.` alone.
- `10 - -4` = 14.
- `-2^2` = 4: unary minus binds to the literal.
- `2^-1` = 0.5.
- The Unicode minus U+2212 is not an operator. It only works inside `currencyToAmount`.

### 5.11 `currencyToAmount(s)` (`lc/shared/util.ts:500-522`)

The number format is a module-level global. `useFormat` keeps it equal to the `numberFormat` pref (`dc/hooks/useFormat.ts:132-138`). Format → thousands separator (`util.ts:337-363`): `comma-dot` ',', `dot-comma` '.', `space-comma` U+202F, `apostrophe-dot` U+2019, `comma-dot-in` ','. Unknown formats fall back to comma-dot (`util.ts:303-314`).

```
s = s.replace(U+2212 → '-')
m = last [,.] in s
if no m, or (m == thousandsSep AND m.index + 4 <= len(s)):   # ≥3 chars after the separator
    int = s with everything except [0-9-] removed; frac = null
else:
    int = s[..m] with everything except [0-9-] removed; frac = s[m+1..] RAW
v = JS parseFloat(int + '.' + frac)       # frac null → the string "null", so "1234.null" → 1234
return isNaN(v) ? null : v
```

Examples:

- comma-dot: `1,234` → 1234, but `1,23` → 1.23 and `1.2.3` → 12.3.
- dot-comma: `1.234` → 1234.
- apostrophe-dot and space-comma: both `,` and `.` are always decimal (`1,234` → 1.234).

The port needs a JS-compatible `parseFloat` (longest-prefix `[+-]?(\d+\.?\d*|\.\d+)`; no exponent can arise here).

### 5.12 Arithmetic unit test vectors (verbatim, `lc/shared/arithmetic.test.ts:1-112`)

`setNumberFormat` is module state that persists across tests in file order. Tests before line 53 run under the default `comma-dot`. Everything from line 59 on runs under `apostrophe-dot`.

```ts
import { evalArithmetic } from './arithmetic';
import { setNumberFormat } from './util';

describe('arithmetic', () => {
  test('handles negative numbers', () => {
    expect(evalArithmetic('-4')).toBe(-4);
    expect(evalArithmetic('10 + -4')).toBe(6);
  });

  test('handles simple addition', () => {
    expect(evalArithmetic('10 + 10')).toEqual(20);
    expect(evalArithmetic('1.5 + 1.5')).toEqual(3);
    expect(evalArithmetic('(12 + 3) + (10)')).toEqual(25);
    expect(evalArithmetic('10 + 20 + 30 + 40')).toEqual(100);
  });

  test('handles simple subtraction', () => {
    expect(evalArithmetic('10 - 10')).toEqual(0);
    expect(evalArithmetic('4.5 - 1.5')).toEqual(3);
    expect(evalArithmetic('(12 - 3) - (10)')).toEqual(-1);
    expect(evalArithmetic('10 - 20 - 30 - 40')).toEqual(-80);
  });

  test('handles multiplication', () => {
    expect(evalArithmetic('10 * 10')).toEqual(100);
    expect(evalArithmetic('1.5 * 1.5')).toEqual(2.25);
    expect(evalArithmetic('10 * 20 * 30 * 40')).toEqual(240000);
  });

  test('handles division', () => {
    expect(evalArithmetic('10 / 10')).toEqual(1);
    expect(evalArithmetic('1.5 / .5')).toEqual(3);
    expect(evalArithmetic('2400 / 2 / 5')).toEqual(240);
  });

  test('handles order of operations', () => {
    expect(evalArithmetic('(5 + 3) * 10')).toEqual(80);
    expect(evalArithmetic('5 + 3 * 10')).toEqual(35);
    expect(evalArithmetic('20^3 - 5 * (10 / 2)')).toEqual(7975);
  });

  test('handles exponent as right-associative', () => {
    expect(evalArithmetic('2^3^2')).toEqual(512);
  });

  test('handles same-precedence operators left-to-right', () => {
    expect(evalArithmetic('24 / 3 * 2')).toEqual(16);
    expect(evalArithmetic('24 * 3 / 2')).toEqual(36);
    expect(evalArithmetic('10 - 2 + 1')).toEqual(9);
    expect(evalArithmetic('10 + 2 - 1')).toEqual(11);
  });

  test('respects current number format', () => {
    expect(evalArithmetic('1,222.45')).toEqual(1222.45);

    setNumberFormat({ format: 'space-comma', hideFraction: false });
    expect(evalArithmetic('1 222,45')).toEqual(1222.45);

    setNumberFormat({ format: 'apostrophe-dot', hideFraction: false });
    expect(evalArithmetic(`1’222.45`)).toEqual(1222.45);
  });

  test('ignores leftover characters', () => {
    expect(evalArithmetic('1+2)')).toBe(3);
    expect(evalArithmetic('1+2)foo')).toBe(3);
    expect(evalArithmetic('(1+2)x')).toBe(3);
    expect(evalArithmetic('10+20 trailing')).toBe(30);
    expect(evalArithmetic('1+2(3')).toBe(3);
  });

  test('handles apostrophe-dot format with keyboard apostrophe (U+0027)', () => {
    setNumberFormat({ format: 'apostrophe-dot', hideFraction: false });

    // Test with keyboard apostrophe (U+0027) - what users type
    const keyboardApostrophe = '12'345.67';
    expect(keyboardApostrophe.charCodeAt(2)).toBe(0x0027); // Verify it's U+0027
    expect(evalArithmetic(keyboardApostrophe)).toBe(12345.67);

    // More test cases with keyboard apostrophe
    expect(evalArithmetic('1'234.56')).toBe(1234.56);
    expect(evalArithmetic('1'000.33')).toBe(1000.33);
    expect(evalArithmetic('100'000.99')).toBe(100000.99);
    expect(evalArithmetic('1'000'000.50')).toBe(1000000.5);
  });

  test('handles apostrophe-dot format with typographic apostrophe (U+2019)', () => {
    setNumberFormat({ format: 'apostrophe-dot', hideFraction: false });

    // Test with right single quotation mark (U+2019) - what Intl.NumberFormat outputs
    const intlApostrophe = '12’345.67';
    expect(intlApostrophe.charCodeAt(2)).toBe(0x2019); // Verify it's U+2019
    expect(evalArithmetic(intlApostrophe)).toBe(12345.67);

    // More test cases with typographic apostrophe
    expect(evalArithmetic('1’234.56')).toBe(1234.56);
    expect(evalArithmetic('1’000.33')).toBe(1000.33);
  });

  test('handles apostrophe-dot format in arithmetic expressions', () => {
    setNumberFormat({ format: 'apostrophe-dot', hideFraction: false });

    // Test arithmetic operations with keyboard apostrophe
    expect(evalArithmetic('1'000 + 2'000')).toBe(3000);
    expect(evalArithmetic('10'000 - 2'500')).toBe(7500);
    expect(evalArithmetic('1'000 * 2')).toBe(2000);
    expect(evalArithmetic('4'000 / 2')).toBe(2000);

    // Test arithmetic operations with typographic apostrophe
    expect(evalArithmetic('1’000 + 2’000')).toBe(3000);
    expect(evalArithmetic('10’000 - 2’500')).toBe(7500);
  });
});
```

### 5.13 Text → integer in the UI, and rounding

**Rounding.** `amountToInteger(x, dp=2) = Math.round(x * 10^dp)` (`lc/shared/util.ts:541-547`). JS `Math.round` rounds half toward +∞. In Rust: `let f = v.floor(); if v - f >= 0.5 { f + 1.0 } else { f }`. Also note that `^` uses V8's `Math.pow`, so `powf` may differ by an ULP, which matters only at .5 boundaries.

**`useFormat().fromEdit(text, default = null)`** (`dc/hooks/useFormat.ts:249-280`):

1. Trim the text. Empty → `default`.
2. Strip bidi marks (U+200E/F, U+202A–E, U+2066–9) and **all letters `\p{L}+`**.
3. Try `evalArithmetic(t, null)`. If that gives null or NaN, **fall back to `currencyToAmount(t)`**. If still null, return `default`.
4. Otherwise `Math.round(v * 10^currencyDecimals)`.

So `+12.34` → 1234 through the fallback, `(5` → 500, and `1e5` → 1500 (letters stripped).

**`forEdit(int)`** = `Intl.NumberFormat(locale-of-numberFormat, {min=max fraction digits = hideFraction ? 0 : currency dp}).format(int/10^dp)` (`useFormat.ts:230-247`, `util.ts:320-400`):

- Grouping separators are included, e.g. `1,234.56`.
- There is **no currency symbol**.
- A negative value shows as `-1,234.56`.
- `-0` shows as `0`.

**Rounding in every port.** Every port of `Math.round` (here, the rule `isapprox` threshold, split percent and remainder math, `getScheduledAmount` for between amounts, `amountToInteger` in formulas) uses the floor-and-compare form above, so halves go toward +∞. Rust's `f64::round` rounds −2.5 to −3 where JS gives −2, so `getScheduledAmount({-1001,-1000})` is −1000 in JS and −1001 in naive Rust. The shortcut `(x + 0.5).floor()` is also wrong: it gives 1 for 0.49999999999999994 (JS gives 0) and 2^52 + 2 for 2^52 + 1 (checked with Node).

### 5.14 Where fields become inputs

**Desktop budget cell** (`dc/components/budget/envelope/EnvelopeBudgetComponents.tsx:395-434`):

- It uses `SheetCell` (`dc/components/table.tsx:722-786`), which renders `InputValue` (`table.tsx:333-401`) with the initial text `format.forEdit(budgeted)`.
- On expose, `useProperFocus` focuses the input and **selects all** (`setSelectionRange(0, 10000)`, `dc/hooks/useProperFocus.tsx:25-40`).
- **Single-operator rule:** if the new text is exactly one of `+ - * / ^`, the value becomes `<original text> + op` (`table.tsx:371-383`).
- Saving happens on Enter or Tab (`table.tsx:353-363,431-438`) or on blur (`reference/actual/packages/component-library/src/Input.tsx:151-154`). Every blur saves, even when nothing changed.
- `onSave(fromEdit(text))` → `amount: parsed ?? 0` → `budget/budget-amount`.
- **Invalid or digit-less text therefore saves 0, not the old value.** `Infinity` reaches `setBudget` and throws in `safeNumber`.

**So "100.00" + typing "+12.34" → 112.34 (11234), yes.** Typing `+` replaces the selection, the single-operator rule turns that into `100.00+`, then `12.34` is appended and `evalArithmetic('100.00+12.34')` = 112.34. The same result comes from placing the caret at the end. In comma-dot, `1,234.56+10` = 1244.56.

**Transaction debit/credit** (`dc/components/transactions/TransactionsTable.tsx:1985-2050`, `dc/components/transactions/table/utils.ts:31-93`):

- It uses `InputCell` → `InputValue`, so the same select-all and single-operator rule apply.
- Initial text is `integerToCurrencyWithDecimal(|amount|)`: cents are kept when non-zero, there is no symbol, and an empty debit shows `0.00`.
- On save it calls `evalArithmetic(debit|credit, null)`; debit is negated. Then `amountToInteger(v)` with **2 dp fixed**.
- **If the result is null, the old amount is kept** (`utils.ts:83-86`).
- There is no currencyToAmount fallback and no letter stripping here. So `+5` typed into an empty field gives null and the old value is kept.

**`FinancialInput`** (`dc/components/util/FinancialInput.tsx:54-120`), used by the hold and transfer menus:

- On focus it shows `forEdit` and selects all. There is **no** single-operator rule.
- Blur or Enter computes `fromEdit(text, 0) ?? 0`. Invalid text gives 0, and `+12.34` gives 12.34, replacing the old value.

---

## 6. Rules, schedules, custom reports, dashboard

### 6.1 Rules

**Storage.**

- Table `rules(id TEXT PK, stage TEXT, conditions TEXT, actions TEXT, tombstone INTEGER DEFAULT 0)` (`mig/1597756566448_rules.sql:3-8`), plus `conditions_op TEXT DEFAULT 'and'` (`mig/1679728867040_rules_conditions.sql:3`).
- `stage` is `'pre'`, `NULL` (the default stage) or `'post'`. Validation rejects anything else (`lc/server/transactions/transaction-rules.ts:127-135`). Legacy `'cleanup'` and `'modify'` are read as `'pre'` when rules load (`transaction-rules.ts:200-203`).
- `conditions_op` is `'and'` or `'or'` (`transaction-rules.ts:136-140`). In JS the model field is `conditionsOp` (`transaction-rules.ts:145-167`).
- Rules load only `WHERE conditions IS NOT NULL AND actions IS NOT NULL AND tombstone = 0` (`transaction-rules.ts:193-196`). A rule whose conditions or actions fail to parse (`RuleError`) is dropped silently. It isn't deleted, it just never runs (`transaction-rules.ts:170-180`).
- **Stored field names are the internal column names.** `serializeConditionsOrActions` maps each item's `field` through `schemaConfig.views.transactions.fields` (`transaction-rules.ts:84-121`). That map is `account→acct`, `payee→description`, `imported_payee→imported_description`, `is_parent→isParent`, `is_child→isChild`, `imported_id→financial_id` and `transfer_id→transferred_id` (`lc/server/aql/schema/index.ts:375-383`). Reading does the reverse (`fromInternalField`). So the stored JSON has `"field":"acct"`, `"description"` and `"imported_description"`, while `amount`, `date`, `notes`, `category` and the others pass through unchanged. `extractScheduleConds` tolerates both spellings (`lc/shared/schedules.ts:257-266`). [unverified]
- A condition serializes as `{op, field, value, type, options?}` (`lc/server/rules/condition.ts:453-460`). An action serializes as `{op, field, value, type, options?}` (`lc/server/rules/action.ts:403-410`). Both shapes allow extra keys (`customName`, `queryFilter`, `conditionsOp` on a condition: `lc/types/models/rule.ts:47-56`). That's one more reason to keep the raw JSON byte-identical.
- A rule that a schedule references can't be deleted (`deleteRule` returns `false`, `transaction-rules.ts:236-248`).

**Condition fields and their types.**

Fields and types come from `lc/shared/rules.ts:62-88`. The ops allowed per type come from `lc/shared/rules.ts:6-51` (client), mirrored by `lc/server/rules/condition.ts:29-191` (server). `isValidOp` is `type ops ∖ disallowedOps ∪ internalOps` (`shared/rules.ts:99-108`).

| field | type | ops |
|---|---|---|
| `imported_payee` | string | is, contains, matches, oneOf, isNot, doesNotContain, notOneOf (not hasTags or hasAnyTag) |
| `payee` | id | is, contains, matches, oneOf, isNot, doesNotContain, notOneOf (not onBudget or offBudget) |
| `payee_name` | string | the full string set |
| `account` | id | the full id set, including **onBudget and offBudget** |
| `category`, `category_group` | id | the id set without onBudget and offBudget, plus the internal op `and` |
| `notes` | string | is, contains, matches, isNot, doesNotContain, hasTags, hasAnyTag (not oneOf or notOneOf) |
| `amount` | number | is, isapprox, isbetween, gt, gte, lt, lte |
| `date` | date | is, isapprox, gt, gte, lt, lte (**no isbetween**, although the TS type at `types/models/rule.ts:96-99` lists it, and `isValidOp` rejects it) |
| `cleared`, `reconciled`, `transfer`, `parent` | boolean | is |
| `saved` | `saved` | none. `CONDITION_TYPES` has no `saved` entry, so constructing one throws (`condition.ts:203-214`) |

The UI's condition field list is imported_payee, account, category, category_group, date, payee, notes, amount, amount-inflow, amount-outflow and cleared (`dc/components/rules/RuleEditor.tsx:1004-1018`). `amount-inflow` and `amount-outflow` are UI pseudo-fields. They're stored as `field:"amount"` with `options:{inflow:true}` or `{outflow:true}` (`shared/rules.ts:120-128`). For a recurring date the UI offers only `is` and `isapprox`, and with inflow or outflow it hides `isbetween` (`RuleEditor.tsx:965-976`).

The value types are in `types/models/rule.ts:36-57`. `oneOf` and `notOneOf` take arrays. `isbetween` takes `{num1, num2}`. `options` may carry `inflow`, `outflow`, `month` or `year`.

**Parsing: values are normalized when the rule is built.**

- **string**: `oneOf` and `notOneOf` drop falsy entries and lowercase the rest. Every other op lowercases the value, except `hasTags` and `hasAnyTag`, which keep the raw value. `contains`, `matches`, `doesNotContain` and the tag ops need a non-empty string (`condition.ts:105-142`).
- **id**: the value is kept as-is, with no lowercasing. `oneOf`, `notOneOf` and `and` need an array (`condition.ts:80-90`). `nullable: true`, so `value: null` is legal (`condition.ts:79`).
- **number**: a plain number becomes `{type:'literal', value}`, and `{num1,num2}` becomes `{type:'between'}`. `isbetween` needs the between form and every other op needs a literal (`condition.ts:147-176`, `rule-utils.ts:268-274`). It isn't nullable.
- **date**: a string of length 10 is `{type:'date'}`, length 7 (`YYYY-MM`) is `{type:'month'}`, and length 4 is `{type:'year'}`. An object with `frequency` is `{type:'recur', schedule: RSchedule}` (`rule-utils.ts:220-266`). `isapprox` takes a date or a recur. `gt`, `gte`, `lt` and `lte` take exact dates only (`condition.ts:33-64`).
- **boolean**: the value must be a JS boolean (`condition.ts:181-189`).

**Evaluation in memory** (`Condition.eval`, `condition.ts:239-447`). This is the path `runRules` uses, so it is the one that matters for import, bank sync, manual entry and schedule posting.

Before the op runs:
- When the type is string, `null` or `undefined` becomes `''` (`:243-245`).
- If the field is `undefined` (the key is absent), the condition is **false** (`:247-249`). `runRules` fills in `payee` and `category` as `null` when they're missing (`transaction-rules.ts:345-356`).
- **Any string field value is lowercased**, ids included (`:251-253`). Combined with the lowercased condition values, string comparisons are effectively case-insensitive. Id values are not lowercased, so an id comparison only works because Actual ids are lowercase UUIDs.
- With `type==='number' && options`, `outflow` returns false when `amount > 0` and otherwise flips the amount to `-amount`. `inflow` returns false when `amount < 0` (`:255-267`). **An amount of 0 passes both inflow and outflow in memory.** The SQL path (§6.1) uses strict `< 0` and `> 0`.

Op semantics:
- `is` and `isapprox`:
  - **date, recur, `is`**: `schedule.occursOn({date: parseDate(v)})` (`:288`).
  - **date, recur, `isapprox`**: `schedule.occursBetween(fieldDate−2d, fieldDate+2d)` (`:280-286`). Pinned to ±2 days inclusive by `lc/server/rules/index.test.ts:146-164`. [test]
  - **date, exact, `isapprox`**: `low ≤ v ≤ high`, where low and high are the date ±2 days, compared as `YYYY-MM-DD` strings (`:293-298`). Pinned by `index.test.ts:106-110`. [test]
  - **date, `is` on `date`, `month` or `year`**: an exact string match, `monthFromDate(v)===`, or `yearFromDate(v)===` (`:300-308`).
  - **number, `is`**: `===`.
  - **number, `isapprox`**: `n − t ≤ v ≤ n + t`, where **`t = Math.round(Math.abs(n) * 0.075)`** (`:312-319`, `shared/rules.ts:234-236`). This is an f64 multiply followed by JS `Math.round`, which rounds halves up: n=100 gives 7.5, so t=8. n=−1535 gives t=115. Pinned by `index.test.ts:262-266`. [test]
  - **everything else, `is`**: `fieldValue === value` (`:322`). For an id with `value:null` this matches a null field.
- `isNot`: `fieldValue !== value` (`:324-325`). There's no null guard, so a null field "is not X" is true.
- `isbetween`: sorts `num1` and `num2`, then checks `low ≤ v ≤ high` inclusive (`:326-331`, `shared/rules.ts:153-158`).
- `contains` and `doesNotContain`: `String(v).indexOf(value)` (`:332-341`). The match is literal and case-insensitive through lowercasing. An id field compares against **the raw id string**, not a name, so `payee contains "amzn"` never matches in memory even though the editor offers it (`getValidOps` for id includes `contains`). The SQL path (§6.1) matches on `payee.name`. **This is a real in-memory versus SQL divergence.** [unverified]
- `oneOf` and `notOneOf`: `value.indexOf(v)` with strict equality. A null field is false for both (`:342-346`, `:377-381`).
- `matches`: `new RegExp(this.value).test(v)` with **no flags** (`:418-427`). Both the pattern and the field are lowercased first, so the match is effectively case-insensitive. Lowercasing the pattern also **changes its meaning**: `\D→\d`, `\S→\s`, `\W→\w`, `\B→\b` and `[A-Z]→[a-z]`. An invalid regex gives false. Rust needs JS-regex semantics plus this lowercasing.
- `hasTags`: every tag from `extractTagsForFilter(value)` (`lc/shared/tags.ts:7-19`: the tokens `#*([^#\s]+)`, re-prefixed with one `#` and deduped) must match `(?<!#)<escaped lowercased tag>([\s#]|$)` against the lowercased field. That's case-insensitive in memory. If no tags parse, `hasTags` is vacuously **true** (`:348-360`).
- `hasAnyTag`: at least one tag matches (`:362-375`).
- `gt`, `gte`, `lt`, `lte`: a null field is false. Dates use `isAfter` and `isBefore` on the strings, and `gte`/`lte` also accept equality (`:382-417`).
- `onBudget` and `offBudget`: `object._account.offbudget === 0` or `=== 1`. Without an account it's false (`:429-441`). `_account` is attached by `prepareTransactionForRules` (`transaction-rules.ts:1173-1180`).

The rule as a whole (`lc/server/rules/rule.ts:157-166`): **a rule with zero conditions never matches**. `'or'` means `some` and anything else means `every`.

Synthetic fields that `prepareTransactionForRules` adds (`transaction-rules.ts:1158-1208`): `payee_name` (the payee's name), `_account`, `_account_name`, `balance` (0 unless needed), `_category_name` and `category_group` (the category's `cat_group`). They're all stripped in `finalizeTransactionForRules` (`:1258-1318`).

**The SQL path** (`conditionsToAQL`, `transaction-rules.ts:493-753`).

This path drives "apply to existing" previews, transaction filters, custom reports and widgets, and schedule matching in the UI. It is **not** the same as `eval`:
- Strings use `$transform:'$lower'`, except the tag ops (`:565-573`). `contains` and `doesNotContain` become `UNICODE_LIKE('%v%', NORMALISE(field))` (`lc/server/aql/compiler.ts:733-747`). `NORMALISE` lowercases, maps ł/ø/ß/œ and strips diacritics (`lc/shared/normalisation.ts:9-15`). In the LIKE pattern, **`%` and `?` act as wildcards** (`lc/platform/server/sqlite/unicodeLike.ts:7-31`). `doesNotContain` and `notregexp` also match NULL (`compiler.ts:743,747`).
- An id field with `contains`, `matches` or `doesNotContain` matches on `<field>.name` (`:665-684`).
- `matches` uses `REGEXP` through JS `new RegExp(regex)` (`lc/platform/server/sqlite/regexp.ts:1-12`).
- Tag ops use `makeTagAqlRegex` without lowercasing, so they're **case-sensitive** here (`tags.ts:50-58`). Zero tags matches nothing (`:693-715`).
- Inflow and outflow require `amount > 0` or `amount < 0` strictly (`:547-562`).
- `oneOf` or `notOneOf` with an empty list matches nothing (`:685-723`).
- A recurring date expands to only the **first 100 occurrences** (`recurDateBounds = 100`, `:495,584-602`).
- Special cases (`:460-490`): `category is null` becomes `AND(transfer=false, parent=false)`, and `category isNot null` becomes `AND(parent=false)`.
- `transfer is true` becomes `transfer_id isNot null`, and `parent` becomes `is_parent` (`:528-542`).

**Actions** (`lc/server/rules/action.ts`).

The ops are `set`, `set-split-amount`, `link-schedule`, `prepend-notes`, `append-notes` and `delete-transaction` (`action.ts:27-34`). Any other op throws, so the whole rule is dropped at load.

The JSON shapes are in `types/models/rule.ts:135-183`:
- `{op:'set', field, value, options?:{template?, formula?, splitIndex?}, type?}`
- `{op:'set-split-amount', value:number|null, options:{splitIndex?, method:'fixed-amount'|'fixed-percent'|'formula'|'remainder', formula?}}`
- `{op:'link-schedule', value:<scheduleId>}`
- `{op:'prepend-notes'|'append-notes', value:string}`
- `{op:'delete-transaction', value}`

The UI's action fields are category, payee, payee_name, notes, cleared, account, date and amount. Split actions exclude amount, cleared, account and date (`RuleEditor.tsx:442-458`).

What each action does (`action.ts:99-262`):
- `set`, plain: `obj[field] = value`. Setting `payee_name` also sets `payee = 'new'`. After each rule, `resolvePayeeNameForRules` looks the payee up by name, or **inserts a new payee row**, even during an import preview (`:197-199`, `transaction-rules.ts:1210-1227`). An empty `payee_name` sets `payee = null`.
- `set` with **`options.template`** (Handlebars, `noEscape`, with the context `{...txn, today}` and the helpers in `lc/server/rules/handlebars-helpers.ts`):
  - A number goes through `parseFloat`, and NaN becomes 0. **It is not converted with amountToInteger**, so the template has to produce integer cents.
  - A date that doesn't parse becomes `'9999-12-31'`.
  - A boolean is `=== 'true'`.
  - This branch is at `:158-192`.
- `set` with **`options.formula`** (HyperFormula):
  - `amount`, `balance` and `parent_amount` are exposed as amounts via `integerToAmount`, and a numeric result goes back through `amountToInteger = Math.round(x*100)`.
  - Errors are pushed to `_ruleErrors` and the field is left unchanged.
  - `BALANCE_OF("acct")` is prefetched.
  - This branch is at `:102-157` and `:308-401`.
  - A formula takes precedence over a template.
- `prepend-notes` and `append-notes`: string concatenation, with no separator added. If notes are empty, the value is used as-is (`:246-255`).
- `link-schedule`: `obj.schedule = value` (`:243-245`).
- `delete-transaction`: `obj.tombstone = 1` (`:256-258`).
- Splits (`rule.ts:15-124`): an action with `options.splitIndex` > 0 belongs to split N. The function returns early when there are no split actions, or when the transaction is already a child.
  - Fixed-amount splits are applied first.
  - `fixed-percent` is next: `Math.round(remainder * pct/100)`.
  - `remainder` splits share `Math.round(rem/n)` equally, and the last remainder split absorbs the rounding.
  - Index 1 (splitIndex 0) is the "apply to all" scratch row, and it's spliced out.
  - Confirmed by the oracle (`crates/veritas-rules/tests/oracle_splits.rs`): splitting sets the parent's payee to null and copies it onto the lines, so a later rule sees no payee. An action numbered past the end adds at most one line, so a rule whose first split action is line 2 throws out of `runRules`. A second split rule on an already split transaction still drops index 1, which is now the first real line, so only its own lines survive and the parent keeps the difference as its `error`.
  - **JS `Math.round` rounds −2.5 to −2, while Rust `f64::round` gives −3.** Every port of `Math.round`, including the split math and `getScheduledAmount`, uses the floor-and-compare form in §5.13.

**Ordering, stages and chaining.**

- Candidate selection uses two indexers (`transaction-rules.ts:65-72, 338-343`, `lc/server/rules/rule-indexer.ts`). One is keyed on `imported_payee` by **lowercased first character**, the other on `payee`.
  - A rule is filed by its **first condition on that field**, whatever its op. If that op is `is`, `isNot`, `oneOf` or `notOneOf`, the rule goes under that value's key (each value's, for the lists). Otherwise it goes under `*`, even when a later condition on the field is `is` (`rule-indexer.ts:38-59`). An empty `oneOf` list files it under no key at all, so that indexer never offers it.
  - The candidates are `index[key(txn)] ∪ index['*']` for each indexer, and the two sets are unioned.
  - The key is computed **once, from the incoming transaction**.
  - The quirk: a rule is skipped without evaluation only when it has an is-family condition on **both** `imported_payee` and `payee` and neither key matches. One consequence: `isNot X` rules with both such conditions get pruned even when they would match. Every other rule is always a candidate.
- `rankRules` (`lc/server/rules/rule-utils.ts:87-110`) orders the stages **pre, then null, then post**. Within a stage, `_rankRules` (`:63-85`) sorts by **ascending specificity score**, then by **id string ascending**.
  - Op points: is or isNot 10; oneOf or notOneOf 9; isapprox or isbetween 5; gt, gte, lt or lte 1; contains, doesNotContain, matches, hasTags, hasAnyTag, onBudget or offBudget 0 (`:18-36`).
  - The score is the sum, **×2 if every condition is one of is, isNot, isapprox, oneOf or notOneOf** (`:38-61`). That includes a rule with zero conditions, but such a rule never matches anyway.
  - An unknown op **resets the running total to 0** (the `return 0` inside the reduce at `:40-43`).
  - Ascending order means **more specific rules run later and win**.
- Chaining (`transaction-rules.ts:401-449`): rules run in order against `finalTrans`. Each match `Object.assign`s that rule's changes, so **later rules see earlier rules' changes**, including a `payee` resolved from `payee_name` and a `category_group` that's re-resolved when `category` changes (`:1239-1256`). There's no early exit, and every rule is evaluated.
- Schedule context (`:380-432`): when the incoming transaction already has a `schedule`, that schedule's rule runs its **actions without checking its conditions**. Rules linked to other schedules are skipped, and every other rule is evaluated normally.
- `Rule.execActions` returns only the fields that changed (`rule.ts:168-179`).

### 6.2 When rules run (trigger points)

The server's `batchUpdateTransactions` never runs rules on the rows it is given (`lc/server/transactions/index.ts:40-217`).

**Server-side `runRules`** (`lc/server/transactions/transaction-rules.ts:323`):

1. **Bank sync**: `syncAccount → reconcileTransactions → matchTransactions` calls `runRules` per transaction **before** matching (`lc/server/accounts/sync.ts:865-877`). Matched existing transactions only take `payee`, `category` and `notes` when the existing value is empty (`lc/server/accounts/sync.ts:694-703`).
2. **File import** (CSV, OFX, QIF and so on) goes through `transactions-import`, `bankSync.reconcileTransactions` (`lc/server/accounts/app.ts:1654-1689`) and the same `matchTransactions`. Rules run in preview mode too (`isPreview`). `dc/accounts/mutations.ts` sends `transactions-import`.
3. **`addTransactions`** calls `runRules` per transaction (`lc/server/accounts/sync.ts:1039-1058`). Its callers are `api/transactions-add` (`lc/server/api.ts:610-622`) and **schedule posting** (`lc/server/schedules/app.ts:589-614`). The API defaults `runTransfers = false` and `learnCategories = false`. Schedule posts use the `addTransactions` default `runTransfers = true`.
4. **Transfer counterpart creation** runs `runRules` on the mirror transaction and keeps only `notes`, `cleared` and `schedule` (`lc/server/transactions/transfer.ts:61-79`).
5. **Schedule forecasting**, preview only (`lc/server/forecast/forecast-schedules.ts:226`, `:264`).

**Client-side, via `send('rules-run')`** (`lc/server/rules/app.ts:96`, `:187-193`):

6. **Manual entry on desktop**:
   - Rules run only for **new, temporary-id rows**, on every field edit before save (`dc/components/transactions/TransactionsTable.tsx:3688-3733`, esp. `:3700-3718`; `dc/components/transactions/TransactionList.tsx:265-294`).
   - The result is merged by `applyRulesToTransaction` (`dc/components/transactions/applyRulesToTransaction.ts:44-82`). A rule may only fill **empty** fields (`null`, `''`, `0` or `false`), never a field the user cleared on that row. The client remembers cleared fields for this.
   - Notes are the exception: the rule's output may wrap the user's note through prepend or append, and the merge is idempotent (`dc/components/transactions/table/utils.ts:120-175`).
   - Parent changes propagate to the split children (`dc/components/transactions/applyRulesToTransaction.ts:91-117`).
   - **Editing an existing transaction never runs rules** (`dc/components/transactions/TransactionsTable.tsx:3729-3731` calls `onSaveProp` directly).
7. **Manual entry on mobile**: the same, for new transactions only. Changing the payee re-applies all fields except those the user cleared (`dc/components/mobile/transactions/TransactionEdit.tsx:1750-1800`).
8. **The "Run rules" bulk action** on selected existing transactions applies the whole result (`dc/components/accounts/Account.tsx:734-776`).
9. **The reconciliation adjustment transaction** (`dc/accounts/reconciliation.ts:114-123`, §4.7).
10. **The preview of upcoming schedule transactions** (`dc/hooks/usePreviewTransactions.ts:103-106`).

**Not run:** the starting-balance insert (§4.8) and the account-close balance transfer, which goes through `transaction-add` (§4.9).

**Learn categories** creates rules automatically; see §4.5.

**For Veritas:** port the client merge (`shouldApplyRuleChange`) into the core, not just `runRules`. Run rules on new manual rows, imports, schedule posts, transfer counterparts (notes, cleared and schedule only) and reconciliation adjustments, and never on edits of existing rows.

### 6.3 Schedules

**Tables** (DDL in §3.3):

- `active` is unused, and the AQL schema doesn't list it (`lc/server/aql/schema/index.ts:114-135`). Next dates are `YYYYMMDD` ints (`toDateRepr`).
- `schedules_json_paths(schedule_id PK, payee, account, amount, date)` (`mig/1618975177358_schedules.sql:19-24`) holds JSON paths like `$[2]` into the rule's conditions.
  - Actual regenerates it with **`INSERT OR REPLACE`** for every rule that has a `link-schedule` action. That happens when services start, i.e. on every budget load (`lc/server/schedules/app.ts:536-574,858`, `lc/server/budgetfiles/app.ts:699`), and on rule sync.
  - It's derived data.
- Names are unique among live schedules, compared after trimming. An empty name becomes NULL (`schedules/app.ts:287-305,354-364`).
- The view `v_schedules` (`schema/index.ts:341-370`):
  - `next_date = CASE WHEN local_next_date_ts = base_next_date_ts THEN local_next_date ELSE base_next_date END`
  - `_payee = payee_mapping.targetId`. `_account`, `_amount`, `_amountOp` and `_date` are `json_extract`ed from the rule's conditions through `schedules_json_paths`.
  - `_has_splits` means some action has `options.splitIndex > 0`.
  - **There is no `next_date` column on `schedules`.**
- Veritas maintains `schedules_json_paths` like Actual does, but with an upsert on known columns (decision 4), so `v_schedules` in an exported file is complete before Actual has opened it.

**Each schedule owns a rule.**

`createSchedule` (`schedules/app.ts:335-392`):
- It requires a date condition with a value.
- It inserts the rule `{stage:null, conditionsOp:'and', conditions, actions:[{op:'link-schedule', value:scheduleId}]}`.
- It inserts `schedules_next_date` with local and base both set to `nextDate` and both `_ts` set to `Date.now()`.
- It inserts `schedules{id, rule, …}`.

Further details:
- `updateSchedule` refuses to change `rule` (`:405-407`). It merges conditions through `updateConditions` (`:107-126`), which replaces the payee, account, amount and date conditions in place and appends the missing ones. It also rewrites any plain `set amount` action to `getScheduledAmount(amount)` (`:143-173`).
- `deleteSchedule` tombstones both the rule and the schedule (`:494-503`).
- If the rule goes missing, `fixRuleForSchedule` deletes it and recreates it as `date isapprox today, amount isapprox 0` (`:186-210`).
- The editor builds its conditions in `dc/components/schedules/schedule-edit-utils.ts:45-58`:
  - `payee is` (always present, even when null)
  - `account is`
  - `date` with **`isapprox` by default**. An existing op is kept.
  - `amount` with op `isapprox` (default), `is` or `isbetween` (`{num1,num2}`, integer cents)
- `posts_transaction`, `name` and `custom_upcoming_length` are columns on `schedules` (`dc/components/schedules/ScheduleEditModal.tsx:124-131`).
- **Completed schedules' rules keep running** in `runRules`, since nothing excludes them. `getCompletedScheduleRuleIds` is only used by the payees app (`lc/server/payees/app.ts:77`).

**`RecurConfig`, the value of the date condition.**

The type is `{frequency:'daily'|'weekly'|'monthly'|'yearly', interval?, patterns?:[{value:number, type:'SU'|'MO'|'TU'|'WE'|'TH'|'FR'|'SA'|'day'}], skipWeekend?, start:'YYYY-MM-DD', endMode?:'never'|'after_n_occurrences'|'on_date', endOccurrences?, endDate?, weekendSolveMode?:'before'|'after'}` (`lc/types/models/schedule.ts:5-20`). A one-time schedule stores a plain `'YYYY-MM-DD'` string instead.

The UI always writes every key: start, interval, frequency, patterns, skipWeekend, weekendSolveMode, endMode, endOccurrences and endDate (`dc/components/select/RecurringSchedulePicker.tsx:74-95`).
- Patterns are cleared to `[]` when the frequency isn't monthly (`:171-180`).
- A pattern value is 1..31 or `-1` for "last". A weekday ordinal is capped at 5 (`:34,49,67-72,108-122,310-316`).
- The editor's default weekend mode is `'before'` (`:81`), and `'after'` when starting from a transaction (`ScheduleEditModal.tsx:50-55`).

Mapping to rschedule (`lc/shared/schedules.ts:201-253`):
- base `{start: parseDate(start) at local 12:00, frequency: FREQ.toUpperCase(), byHourOfDay:[12]}`
- `interval` is set only when it's truthy.
- `after_n_occurrences` sets `count = endOccurrences`, and `on_date` sets `end = parseDate(endDate)`. Pinned inclusive by `lc/shared/schedules.test.ts:468-485`. [test]
- Daily, weekly and yearly ignore `patterns`.
- Monthly with patterns produces **up to two rrules**: one `byDayOfMonth:[days…]` and one `byDayOfWeek:[[MO,n],…]`. **Each one carries its own `count`**, so "after N occurrences" with mixed day and weekday patterns can yield up to 2N dates. The union is deduplicated: "after 3" with `day 2` and `MO 1` from 2026-11-01 gives five dates, because Nov 2 is shared (oracle, §6.3 findings).
- Monthly with no patterns and yearly use the start's day of month, or month and day. With rschedule's RFC 5545 normalization, a start on the **29th, 30th or 31st skips months that lack that day**, a yearly start on Feb 29 occurs only in leap years, and a `day: 31` pattern skips short months. `day: -1` is the last day of the month. Confirmed by the oracle (§6.3 findings). Pinned by `lc/server/schedules/occurrences.test.ts:77-87`, which gives 01-31, 02-29, 03-31, 04-30. [test]
- Last weekday patterns like `FR -1` are pinned by `occurrences.test.ts:90-103`.
- All date math happens in local time at noon (`lc/shared/months.ts:12-83`).

**Computing next dates** (`lc/shared/schedules.ts:283-354`).

- `getNextDate(cond, start = today)`:
  - A `date` value returns that date regardless of `start`.
  - A `recur` value takes the first occurrence ≥ `startOfDay(start)`. If there isn't one, it takes the **last occurrence** (`reverse`), so an ended schedule reports a past date. Pinned by `schedules.test.ts:468-485`.
  - With `skipWeekend`, Saturday and Sunday move to `nextMonday` (`after`) or `previousFriday` (`before`), and an unknown mode throws (`:340-354`).
- `getNextDateAfter(cond, after)`:
  - Start at `after − 1d`, then loop up to 7 times.
  - Take the raw occurrence ≥ start. If it's null or earlier than start, return null.
  - Compute the weekend-adjusted date. Return it if it's later than `after`, otherwise move start to the raw occurrence + 1d.
  - Pinned by `schedules.test.ts:560-612`.
- `getNextDateFrom(cond, today)` (`schedules/app.ts:216-224`) is `min(getNextDate(today), getNextDateAfter(today − 1))`. It catches occurrences that an `after` solve moved onto today or later.
- `setNextDate({id, conditions?, reset?, advance?})` (`schedules/app.ts:226-283`):
  - It computes `advance ? getNextDateAfter(next_date) : getNextDateFrom(today)` and writes only when the value changed.
  - `reset` writes `base_next_date = new` and `base_next_date_ts = Date.now()`, so base wins.
  - Otherwise it writes `local_next_date = new` and `local_next_date_ts = base_next_date_ts`, so local wins.
- `updateSchedule` resets the next date when `resetNextDate` is passed or the account or date condition changed (`:461-477`).

**Status, and the advance and auto-post service.**

- `getStatus` (`lc/shared/schedules.ts:52-76`) returns one of completed, paid (has a transaction), due (`next_date == today`), upcoming (within `getUpcomingDays`), missed (`< today`) or scheduled.
- `custom_upcoming_length` overrides the pref `upcomingScheduledTransactionLength`, which defaults to `'7'`. Values are presets or `'N-day|week|month|year'` (`:11-50,371-410`).
- "Has a transaction" (`getHasTransactionsQuery`, `:165-193`; `getScheduleOccurrenceMatchStartDate`, `:90-102`) means some transaction (splits included) has `schedule = id` and `date ≥ start`.
  - `start` is `next_date` when the date op is `is` or the schedule auto-posts. Otherwise it's `next_date − 2d`.
  - **There is no upper bound.**
  - Pinned by `schedules.test.ts:278-462`.
- `advanceSchedulesService` (`schedules/app.ts:680-825`) looks at every schedule that isn't completed and whose account is open:
  - **posts_transaction**, with status paid (recurring only), due or missed: loop. On paid with `next_date == today`, stop. On paid, advance. On due or missed, post through `postTransactionForSchedule`, then stop if due or one-time, else advance and loop. This catches up missed recurring occurrences one by one.
  - **Without auto-post and paid**: recurring calls `setNextDate()` (from today). A one-time schedule whose date is before today becomes `completed = true`.
- When it runs (`:860-881`): on a `sync` event of type success, error or unauthorized, when `metadata.json` `lastScheduleRun != today`. It records `lastScheduleRun = today` only on success.
  - Local-only budgets: `_fullSync` returns `[]` and emits `success` (`lc/server/sync/index.ts:802-875,886-897`). `initialFullSync` runs on budget load (`budgetfiles/app.ts:264`). So for Ryan this means **once a day, on open**.
  - Creating or editing an auto-post schedule clears `lastScheduleRun` so the service runs again today (`schedules/app.ts:318-333,389,489`).
- Posting (`:589-614`):
  - The transaction is `{payee:_payee, account:_account, amount:getScheduledAmount(_amount), date: today ? currentDay() : next_date, schedule:id, cleared:false}`, sent through `addTransactions`. That runs the rules: the schedule's own rule actions run unconditionally, and transfers run.
  - `getScheduledAmount` returns the number, or `Math.round((num1+num2)/2)` for `isbetween` (`lc/shared/schedules.ts:356-369`).
  - It never posts without an account.
- UI actions (`dc/components/schedules/index.tsx:48-75`):
  - **Post transaction** calls `schedule/post-transaction` and dates the transaction `next_date`.
  - **Post transaction today** dates it `currentDay()`. [unverified] A transaction dated before the match start doesn't count as paid, so posting "today" well before the due date leaves the schedule unpaid.
  - **Skip next date** calls `setNextDate({advance:true})` (`schedules/app.ts:505-507`). On a one-time schedule it's a no-op, because `getNextDateAfter` returns null.
  - **Complete** only sets `completed = 1`.
  - **Restart** sets `completed: false` and `resetNextDate: true`.
- Estimate bills: the amount condition `isapprox` matches ±`round(|n|·0.075)` (§6.1), and the date `isapprox` matches ±2 days around any occurrence.

**Findings from the oracle (WS-RULES).** `crates/veritas-rules/tests/oracle_schedules.rs` creates generated schedules through Actual and compares `next_date` and the upcoming dates; `tests/oracle_rules.rs` runs generated rules over transactions added through `addTransactions`. Over 19,100 schedules and 5,200 transactions they confirmed or added:

1. The rrule union is deduplicated (above), and the 29th–31st, Feb 29 and `day: -1` behavior above holds.
2. **A missing nth weekday spills and skips.** rschedule's `getNthWeekdayOfMonth` (`@rschedule/core` `rules.js`, `getNextWeekdaysOfMonth`) doesn't keep the date in the month, so a fifth Friday in a month with four becomes the next month's first Friday. The monthly `ByDayOfWeek` pipe jumps to that spilled date and only then looks again, so earlier pattern dates of the next month are skipped: `[FR 5, TU 1]` from 2028-03-29 never yields Tuesday 2028-05-02, because April's spilled Friday is May 5. The pipe runs before the interval check, so a month outside the interval spills too (`interval: 2` from March skips Thursday 2026-01-01 after December spills to January 2). The result depends on where the walk starts, so a query from 2028-05-01 does return May 2. The reverse walk that finds an ended schedule's last date never sees spilled dates.
3. **`api.getScheduleDates` checks more than the app.** It runs `validateScheduleDate` first (`lc/server/api.ts:132-159`): a strict `YYYY-MM-DD` start and end date, `interval` a positive integer when present, and `endOccurrences` a positive integer. `createSchedule` doesn't, so a recurring start of `2026-02-29` is stored, rolls over to March 1 in `parseDate`, and gets a `next_date`, but the API refuses to expand it.
4. **`transfer` and `parent` conditions never match in memory.** The transaction object `runRules` evaluates has no `transfer` or `parent` key, so `Condition.eval` returns false for both (`condition.ts:247-249`). Only the SQL path maps them (§6.1).
5. **`onBudget`, `offBudget` and `payee_name` read the starting transaction.** `_account` and `payee_name` are set once in `prepareTransactionForRules` (`transaction-rules.ts:1158-1208`) and aren't refreshed when a rule sets `account` or `payee`. Only a `set payee_name` action rewrites `payee_name`, and later rules see the new name. `category_group` is refreshed when `category` changes (`:1239-1256`).
6. **`addTransactions` overrides two fields after the rules.** It sets `account` back to the account being added to and defaults `cleared` to true when the result has none (`sync.ts:1059-1064`). A `set account` action has no effect there.
7. **Spec wording for the indexer was loose.** The first condition on the field decides, as corrected above.

### 6.4 Custom reports

The table is `custom_reports` (`mig/1707267033000_reports.sql:3-26`), with columns added in `mig/1716359441000_include_current.sql:3`, `mig/1738491452000_sorting_rename.sql:3-11`, `mig/1759260219000_add_trim_interval_report_setting.sql:4` and `mig/1780099200000_add_show_trend_lines_report_setting.sql:3`.

Columns, with their mapping in `reportModel` (`lc/server/reports/app.ts:47-102`):

| column | JS field | values and semantics |
|---|---|---|
| `id`, `name` | id, name | Required, and unique among live reports (`:124-190`) |
| `start_date`, `end_date` | startDate, endDate | `YYYY-MM-DD`. Used when `date_static = 1`. AQL defaults `'2023-06'` and `'2023-09'` (`schema/index.ts:162-163`) |
| `date_static` | isDateStatic | `1` uses the stored dates. `0` takes a live range from `date_range` |
| `date_range` | dateRange | This week, Last week, This month, Last month, Current quarter, Previous quarter, Last 30 days, Last 3 months, Last 6 months, Last 12 months, Year to date, Last year, Prior year to date, All time (`dc/components/reports/ReportOptions.ts:91-232`). Resolved by `getLiveRange` (`dc/components/reports/getLiveRange.ts:8-111`) |
| `mode` | mode | `'total'` or `'time'` (`dc/components/reports/disabledList.ts:82-180`) |
| `group_by` | groupBy | Category, Group, CategoryGroup (donut only), Payee, Account, Tag, Interval (`ReportOptions.ts:59-67`) |
| `sort_by` | sortBy | asc, desc, name, budget. Defaults to `'desc'` |
| `balance_type` | balanceType | Payment (totalDebts), Deposit (totalAssets), Net (totalTotals), Net Payment (netDebts), Net Deposit (netAssets), Budgeted (totalBudgeted) (`ReportOptions.ts:38-57`). The **DB default `'Expense'` is legacy**. Tag plus Budgeted is rejected (`reports/app.ts:28-30`) |
| `show_empty`, `show_offbudget`, `show_hidden`, `show_uncategorized`, `trim_intervals`, `show_trend_lines`, `include_current` | the matching booleans | 0 or 1 |
| `graph_type` | graphType | Total mode: TableGraph, BarGraph, AreaGraph, DonutGraph. Time mode: TableGraph, StackedBarGraph, LineGraph. Plus BarLineGraph in `ChooseGraph.tsx:133` |
| `conditions` | conditions | JSON array of rule conditions with **public** field names (not run through toInternalField), evaluated by `conditionsToAQL` (§6.1) |
| `conditions_op` | conditionsOp | `and` or `or` |
| `metadata` | metadata (+ `tagScope`) | JSON. `tagScope` is `{mode:'all'}` or `{mode:'selected', tagIds:[]}` and is split out on read (`reports/app.ts:48,100`) |
| `interval` | interval | Daily, Weekly, Monthly, Yearly (`ReportOptions.ts:246-275`) |
| `color_scheme` | not mapped | Never written by `fromJS`. Pass it through |
| `selected_categories` | not mapped | Legacy. A migration moved it into `conditions` as `category oneOf` and nulled it (`mig/1722717601000_reports_move_selected_categories.js:14-54`) |

The live date range (`getLiveRange.ts`):
- Numeric ranges (`Last N months`, `This month` = 0, `Last month` = 1, the weeks) use `getSpecificRange(N, includeCurrent ? N : N−1)`.
- That means the start is the first of the month N months ago, and the end is the end of the current month, or of the previous month when `include_current = 0`. "This" ranges ignore `include_current` (`dc/components/reports/reportRanges.ts:179-203`).
- Named ranges are clamped to the earliest transaction (`validateRange`).

### 6.5 Dashboard

**Tables.**

- `dashboard(id PK, type TEXT, width INTEGER, height INTEGER, x INTEGER, y INTEGER, meta TEXT, tombstone INTEGER DEFAULT 0)` (`mig/1722804019000_create_dashboard_table.js:13-23`), plus `dashboard_page_id TEXT` (`mig/1765518577215_multiple_dashboards.js:14-16`).
- `dashboard_pages(id PK, name TEXT, tombstone INTEGER DEFAULT 0)`. A migration creates `'Main'` (`mig/1765518577215_multiple_dashboards.js:7-28`). The last page can't be deleted, and deleting a page tombstones its widgets (`lc/server/dashboard/app.ts:116-137`).
- Rows with `type IS NULL` were tombstoned (`mig/1730744182000_fix_dashboard_table.sql:3-5`).
- AQL requires type, width, height, x and y. `meta` is JSON (`schema/index.ts:203-218`).
- Layout: 12-column react-grid-layout with `rowHeight: 100` on desktop, a single column on mobile (`dc/components/reports/Overview.tsx:774-777`).
  - Minimum height: 1 for custom-report, markdown and formula; 3 for sankey; 2 for everything else.
  - Minimum width: 1 for formula; 2 for custom-report, markdown, cash-flow and summary; 3 for everything else (`Overview.tsx:73-98`).
  - The layout is written back only in edit mode (`Overview.tsx:268-282`).
  - New-widget placement: `dashboard/app.ts:191-223`.

**Widget types and `meta`.**

The known types (`lc/server/dashboard/app.ts:31-50`) and the meta shapes (`lc/types/models/dashboard.ts:41-497`). `meta` may be `null` for every type except custom-report and markdown.

| type | meta |
|---|---|
| `net-worth-card` | `{name?, conditions?, conditionsOp?, timeFrame?, interval?:'Daily'\|'Weekly'\|'Monthly'\|'Yearly', mode?:'trend'\|'stacked'}` |
| `cash-flow-card` | `{name?, conditions?, conditionsOp?, timeFrame?, showBalance?}` |
| `spending-card` | `{name?, conditions?, conditionsOp?, compare?, compareTo?, isLive?, mode?:'single-month'\|'budget'\|'average', averageRange?:{mode:'last-n-months',months:3\|6\|12}\|{mode:'year-to-date'}\|{mode:'all-time'}}` |
| `budget-analysis-card` | `{name?, conditions?, conditionsOp?, timeFrame?, interval?, graphType?:'Line'\|'Bar', showBalance?, balanceOnly?, showHiddenCategories?}` |
| `custom-report` | `{id}`, which points at `custom_reports.id` |
| `crossover-card` | `{name?, expenseCategoryIds?, incomeAccountIds?, timeFrame?, safeWithdrawalRate?, estimatedReturn?, expectedContribution?, projectionType?:'hampel'\|'median'\|'mean', showHiddenCategories?, expenseAdjustmentFactor?}` |
| `markdown-card` | `{content, text_align?:'left'\|'right'\|'center'}` |
| `summary-card` | `{name?, conditions?, conditionsOp?, timeFrame?, content?}`. **`content` is a JSON string**: `{type:'sum'\|'avgPerMonth'\|'avgPerYear'\|'avgPerTransact', fontSize?}` or `{type:'percentage', divisorConditions, divisorConditionsOp, divisorAllTimeDateRange?, fontSize?}` (`:407-431`) |
| `calendar-card` | `{name?, conditions?, conditionsOp?, timeFrame?}` |
| `formula-card` | `{name?, formula?, fontSize?, fontSizeMode?, staticFontSize?, showTitle?, colorFormula?, queriesVersion?, queries?: Record<string,{conditions?,conditionsOp?,timeFrame?}>}` |
| `sankey-card` | `{name?, conditions?, conditionsOp?, timeFrame?, mode?:'budgeted'\|'spent', topNcategories?, categorySort?:'per-group'\|'global'\|'budget-order', showPercentages?, groupAccounts?, showTransfers?, layerFrom?, layerTo?}` |
| `balance-forecast-card` | `{name?, startDate?, endDate?, accounts?, conditions?, conditionsOp?, timeFrame?, granularity?:'Daily'\|'Monthly', yAxisBase?, source?}` |
| `age-of-money-card` | `{name?, conditions?, conditionsOp?, timeFrame?, granularity?:'daily'\|'weekly'\|'monthly'}` |
| `monte-carlo-card` | Large. See `dashboard.ts:127-353` |

`TimeFrame` is `{start, end, mode:'sliding-window'|'static'|'full'|'lastMonth'|'lastYear'|'yearToDate'|'priorYearToDate'|'currentQuarter'|'previousQuarter'}` (`dashboard.ts:11-24`), resolved by `calculateTimeRange` (`reportRanges.ts:250-345`). The default dashboard is defined in `lc/shared/dashboard.ts:3-260`. Some types sit behind feature flags in the add-widget menu (`dc/components/reports/getDashboardWidgetItems.ts:85-110`).

---

## 7. Decisions taken

The Architect settled these after the research. They are binding for every workstream.

1. **New budgets start from a pinned template db.** The template is Actual's `default-db.sqlite` after all 60 migrations, exactly as Actual produces it, generated by the oracle. It carries the seed rows, the 12 default widgets, the `Main` page and `sqlite_stat1` (§3.1, §3.4). Veritas never writes `__migrations__` (§3.7).
2. **Import and export go through the zip only.** Veritas never edits an Actual budget folder in place, so it never has to manage `kvcache_key` (§3.10), Actual's `metadata.json` rewrites (§3.9) or Actual's backups.
3. **The exported db uses `journal_mode=DELETE`.** Actual never uses WAL (`lc/server/db/index.ts:72-81`). The Veritas working copy may use WAL. Whether a WAL-header file actually breaks Actual's import or export is [unverified]; P0.3 checks it.
4. **`schedules_json_paths` is maintained like Actual maintains it**, one row per schedule-linked rule (§6.3), but written with `INSERT … ON CONFLICT(schedule_id) DO UPDATE SET` on the known columns, never `INSERT OR REPLACE`.
5. **Veritas syncs a transfer's date across both sides** (R-XFER-3) and leaves Actual's `sync-transfer-date` preference untouched. Actual files hold both matched and mismatched dates, so this is safe in both directions (§4.2).
6. **Move budget and cover overspending write Actual's "Reassigned…" budget note** to `notes['budget-YYYY-MM']`, in Actual's exact format (§5.8), for parity. Close goal's release is a Move budget, so it writes one too.
7. **The rollover flag's horizon matches Actual.** Setting it writes `carryover` from the start month through the latest created month (current + 12), and Veritas reads the flag strictly per month (§5.8).
8. **JS rounding.** Every port of `Math.round` rounds halves toward +∞ with the floor-and-compare form in §5.13, never Rust's `f64::round` or `(x + 0.5).floor()`.
9. **Differential tests pin "today".** The created-month range, `getCategoryAverage`, schedule next dates and the "Reassigned…" note text all depend on the current date (§5.2, §5.8, §6.3).

---

## 8. Open questions for Ryan

1. **Should Veritas write a goal's target into Actual natively?** Actual has three places for a target amount, none for a target date, and shows goal progress only when the experimental flag `flags.goalTemplatesEnabled` is on (§5.9):
   - (a) a `#goal <amount>` line in the category's note (major units, no thousands separators);
   - (b) `goal_def` plus `template_settings = {"source":"ui"}` (without `"ui"`, Actual erases `goal_def` on its next template apply unless the note has the line);
   - (c) per-month `zero_budgets.goal` (minor units) and `long_goal = 1` rows, which is what Actual's own apply writes and the only thing its display reads. This goes a step beyond "templates preserved, not executed" (D15).

   The research leaned toward (a) plus (c) for the months Veritas touches, with a note telling you to turn the flag on. Not decided.
2. **What should an amount field do with input it can't evaluate?** Actual depends on the field: transaction amounts keep the old value; budget cells and the hold/transfer inputs save 0. `(5` gives 5 in a budget cell and keeps the old value in a transaction field; `1/0` makes Actual's budget write throw (§5.14). Veritas can copy Actual per field kind, or pick one rule everywhere (the research suggested "keep the old value" everywhere, recorded as a deviation for budget cells). Not decided.
3. **Keep R-ACC-4's "last amount" memory?** Actual has no such thing; its payee memory is only the category-learning rules (§4.5). Prefilling the last amount would be a Veritas addition, stored in `veritas_*`. Not decided.

### Design calls still open (for the Architect to route)

These came out of the research and aren't settled by PLAN.md or §7:

- **Deleting one side of a transfer whose counterpart is a split child.** Actual unlinks the child instead of deleting it (§4.2). R-XFER-3 says "deleting one side deletes both", which would unbalance the split. This matters for loan payments (R-DEBT-2).
- **Which of the 14 widget types render natively** and which show "Open in Actual" (§6.5, R-REP-4). R-REP-6 and R-FC-1 overlap Actual's own Sankey and forecast cards.
- **Paid with a date before `next_date`** doesn't count as paid in Actual (§6.3). Constrain the date in R-BILL-3, or advance `schedules_next_date` with the local write.
- **`lastScheduleRun` in exports** (R-BILL-2). Actual reruns its auto-post service when the value isn't today; the rerun is harmless only if Veritas posted with the same `schedule` link and a date ≥ `next_date`.
- **R-BILL-4 month-end input.** Map "end of month" to `patterns: [{type: 'day', value: -1}]` and warn on days 29–31.
- **Oracle dump hold values** (§6.4 of PLAN): carry both `buffered` and `buffered-selected`, since the API exposes only the first.
