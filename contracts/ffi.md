# FFI surface (contract v3)

`veritas-ffi` is the only crate the apps link. It exports with UniFFI proc macros, and `cargo xtask bindings` generates the Swift module `VeritasCore`, with its C module `VeritasCoreFFI`, into `apps/macos/Packages/VeritasKit`. Swift never sees SQL, Actual column names or business rules (PLAN §6.1, Appendix C).

## Rules for every export

- Every export returns `Result<_, FfiError>` and goes through `guard`, which catches panics into `FfiError::Internal`. UniFFI turns an infallible export into `try!` in Swift, so a panic there would crash the app.
- Coarse calls only. The UI asks for whole screen models ("budget month 2027-03"), never one cell at a time (PLAN §6.5).
- Records are plain data. Money crosses as `i64` minor units, and VeritasKit formats it for display. Dates cross as `"YYYY-MM-DD"` strings, months as `Month`, and moments as UTC `"YYYY-MM-DDTHH:MM:SSZ"`.
- Long work (open, create, import, export, and later reports and forecasts) is `async` in Swift and runs on a core thread. Cancelling the Swift task stops it before its one visible step. A cancelled create, import or export leaves nothing behind, and a cancelled open closes the budget again (`FfiError::Cancelled`).
- A panic inside a session call reloads the budget from disk before the `Internal` error comes back, so memory always matches the file (R-REL-7).

## Free functions

| Rust | Swift | Notes |
|---|---|---|
| `core_version()` | `coreVersion() throws -> String` | Proves the bridge links. |
| `parse_month(text)` | `parseMonth(text:) throws -> Month` | `"YYYY-MM"` → `Month { year, month }`. |
| `list_budgets(budgets_dir)` | `listBudgets(budgetsDir:) throws -> [BudgetListing]` | Every budget folder in the directory: `id`, `name`, `folder`, `lastOpened`, most recently opened first. Hidden folders, including budgets still being built, are skipped. |
| `currencies()` | `currencies() throws -> [Currency]` | Actual's currency table, in its order: `code` (empty for "None"), `name`, `symbol`, `decimalPlaces`, `numberFormat` (the style Actual suggests with it) and `symbolFirst`. |
| `evaluate_amount(text, style)` | `evaluateAmount(text:style:) throws -> Int64` | What was typed in an amount field, arithmetic included, the way Actual evaluates it (R-BUD-3): `"12.50+3"` is 1550. `style` decides the decimal separator. Text that isn't a storable amount is `Invalid` with a message to show beside the field, and the field keeps its old value (D37). Too large is `Overflow`. |
| `amount_edit_text(amount, style)` | `amountEditText(amount:style:) throws -> String` | What an amount field shows when it opens (R-BUD-3): Actual's `forEdit`, with grouping separators, no currency symbol and a hyphen for negatives, so typing `+12.34` after it adds to the value. |
| `amount_field_text(original, typed)` | `amountFieldText(original:typed:) throws -> String` | Actual's single-operator rule, run on every keystroke: when the whole text becomes one operator typed over the selected value, it becomes `original` plus that operator. |

## `Session` (object)

One open budget folder. A second window or process on the same folder opens read-only (R-REL-6).

| Call | Notes |
|---|---|
| `Session.open(directory:) async throws` | Usable as soon as it returns. The full integrity check, the invariant audit and the open snapshot run in the background. |
| `Session.createBudget(name:directory:format:options:) async throws` | A brand-new budget (R-IO-0, D52 to D56). `NumberFormat { currency, style, hideDecimals }`, where `style` is one of `commaDot`, `dotComma`, `spaceComma`, `apostropheDot` and `commaDotIn`. `NewBudgetOptions { retirement401k, hsa, accounts }`, where each `NewAccount { name, kind, startingBalance }` is in cents. For a `creditCard`, `lineOfCredit`, `loan` or `mortgage`, the balance is what is owed, as a positive amount, and the core stores it negative. The core decides each kind's budget flag and side (D8, docs/finance/statements.md ST-9.2): `checking`, `savings`, `cash`, `creditCard` and `lineOfCredit` are on budget; `investment`, `retirement`, `property`, `loan` and `mortgage` are off budget. A blank account name is `Invalid`. Built aside and renamed into place. Answers `Invalid` ("not available in this build yet") until veritas-store's `create_budget` lands. |
| `Session.importActualZip(zip:intoDirectory:) async throws` | Copies an Actual export into a new folder and opens it. The zip is only read. A copy that SQLite finds damaged is refused with `AuditFailed`. |
| `state() throws -> SessionState` | Everything the shell's chrome shows in one call: `directory`, `readOnly` (the banner's reason), `canUndo`, `canRedo`, `checking` (background check still running), `snapshotProblem`, `skippedRows`, `auditFindings`, `backupFolder`, `lastBackup`. |
| `budgetMonth(month:today:) throws -> BudgetMonth` | The Budget screen for a month (R-BUD-1), clamped to the months Actual has created; `today` is the local civil date, `YYYY-MM-DD`, and decides that range. `BudgetMonth` has `month`, `firstMonth`, `lastMonth`, `toBudget`, `income`, `fromLastMonth`, `overspentLastMonth`, `budgeted`, `forNextMonth`, `spent`, `available` and the groups in Actual's order, income last. Each `EnvelopeGroup` has `id`, `name`, `isIncome`, `hidden`, totals and its `envelopes`; each `EnvelopeRow` has `id`, `name`, `hidden`, `budgeted`, `spent`, `available`, `carryover`, `note`, a `tone` (`available`, `low` at 20% or less left, `overspent`, `income`, `neutral` at zero) and the drain bar's `fill`, available ÷ (carried in + budgeted), 0 to 1 (design/tokens.md). |
| `overview() throws -> Overview` | `budgetName`, `numberFormat` (`nil` until the core reads Actual's preferences, so use Actual's defaults), and the four sidebar groups (`cash`, `credit`, `investments`, `loans`, decided by docs/finance/statements.md ST-9), each with its accounts (`id`, `name`, `balance` in cents, `closed`, `deleted`, `placedByBalance`) and `total`. |
| `auditReport() throws -> [AuditFinding]?` | `nil` until the background check finishes. Findings name rows by id only. Only `damage` makes the budget read-only (D51). |
| `perform(command:) throws -> Change` | Each `Command` is one SQLite transaction (R-REL-1) and one undo step (R-REL-5). v1 has `renameAccount(accountId:name:)`. v3 adds the budget's: `setBudget(month:categoryId:amount:)`; `moveBudget(month:amount:fromCategoryId:toCategoryId:)`, where a `nil` id is To Budget, never a transaction (R-BUD-4); `coverOverspending(month:categoryId:fromCategoryId:amount:)`, where `nil` covers all of it from To Budget or the whole overspend; `holdForNextMonth(month:amount:)` and `resetHold(month:)`; `setCarryover(month:categoryId:carryover:)`; and `setCategoryNote(categoryId:text:)`, where blank text removes the note (R-BUD-6). Each runs Actual's own budget action through veritas-engine. Workstreams add commands through `contract` PRs. |
| `undo() throws -> Change`, `redo() throws -> Change` | Unlimited for the session. |
| `subscribe(listener:) throws -> UInt64`, `unsubscribe(subscription:)` | `ChangeListener` is a Swift protocol. `changed(change:)` is called after every commit, undo, redo, restore or reload, on the calling thread, after the session is released, so the listener may call back in. |
| `snapshots() throws -> [SnapshotInfo]` | Newest first: `id`, `takenAt`, `reason`. |
| `restoreSnapshot(id:) throws -> Change` | Snapshots the current state first, clears undo history, and returns `everything: true`. |
| `retention()`, `setRetention(retention:)` | `Retention { hourly, daily, monthly, operationDays }`, default 48, 30, 12 and 30. |
| `exportActualZip(to:) async throws -> [AuditFinding]` | R-IO-2. Snapshots a writable budget first, audits, writes beside `to` and renames over it. **In the sandbox, `to` is a file in the app's temporary directory, and Swift then moves it to where the user chose**, through the save panel's URL and the security-scoped access Swift holds. The core writes only in the budgets directory (each budget's folder and, while creating or importing, a hidden folder beside it), the backup folder and the app's temporary directory. |
| `setBackupFolder(folder:)`, `backUpNow() throws -> String` | R-REL-4. Swift holds the folder's security-scoped access and sets the folder each time it opens the budget. `backUpNow` writes `YYYY-MM-DD-<budget name>.zip`. The app schedules it; the core never does. Failures land in `state().lastBackup.problem` and never block editing. |
| `maintain() throws -> Change` | The app calls this on a timer, every few minutes. It collects the background check and takes the hourly snapshot when one is due. **After it returns, the shell refetches `state()`**: the read-only reason, `checking`, `auditFindings` and `snapshotProblem` may have changed even when the returned change is empty. |
| `close() throws` | Releases the connection and the writer lock now. Later calls fail with `Invalid`. |

## Change events

`Change { entities: [EntityRef], everything: Bool }`. `EntityRef { kind, id, month }` names one row: `kind` is an `EntityKind`, `id` is absent only for a month's hold, and `month` is set for budget cells and holds. A change naming an account or transaction means the overview's balances changed. `everything` (after a restore, a reload, or the background check turning the budget read-only) means refetch every screen and `state()`.

## Planned next

Queries returning screen models: `budgetMonth(Month)`, `register(accountId, filter)`, `bills()`, `goals()`, `netProfit(range)`, `sankey(range)`, `forecast(horizon, scenarios)`, `wrapped(year)`, `statements(range)` and `taxSummary(year)`. Each arrives through a `contract` PR from the workstream that owns it.
