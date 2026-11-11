# FFI surface (contract v1)

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

## `Session` (object)

One open budget folder. A second window or process on the same folder opens read-only (R-REL-6).

| Call | Notes |
|---|---|
| `Session.open(directory:) async throws` | Usable as soon as it returns. The full integrity check, the invariant audit and the open snapshot run in the background. |
| `Session.createBudget(name:directory:format:options:) async throws` | A brand-new budget (R-IO-0, D52 to D56): `NumberFormat { currency, style, hideDecimals }` and `NewBudgetOptions { retirement401k, hsa }`. Built aside and renamed into place. Answers `Invalid` ("not available in this build yet") until veritas-store's `create_budget` lands. |
| `Session.importActualZip(zip:intoDirectory:) async throws` | Copies an Actual export into a new folder and opens it. The zip is only read. A copy that SQLite finds damaged is refused with `AuditFailed`. |
| `state() throws -> SessionState` | Everything the shell's chrome shows in one call: `directory`, `readOnly` (the banner's reason), `canUndo`, `canRedo`, `checking` (background check still running), `snapshotProblem`, `skippedRows`, `auditFindings`, `backupFolder`, `lastBackup`. |
| `overview() throws -> Overview` | `budgetName`, `numberFormat` (`nil` until the core reads Actual's preferences, so use Actual's defaults), and the four sidebar groups (`cash`, `credit`, `investments`, `loans`, decided by docs/finance/statements.md ST-9), each with its accounts (`id`, `name`, `balance` in cents, `closed`, `deleted`, `placedByBalance`) and `total`. |
| `auditReport() throws -> [AuditFinding]?` | `nil` until the background check finishes. Findings name rows by id only. Only `damage` makes the budget read-only (D51). |
| `perform(command:) throws -> Change` | Each `Command` is one SQLite transaction (R-REL-1) and one undo step (R-REL-5). v1 has `renameAccount(accountId:name:)`; workstreams add commands through `contract` PRs. |
| `undo() throws -> Change`, `redo() throws -> Change` | Unlimited for the session. |
| `subscribe(listener:) throws -> UInt64`, `unsubscribe(subscription:)` | `ChangeListener` is a Swift protocol. `changed(change:)` is called after every commit, undo, redo, restore or reload, on the calling thread, after the session is released, so the listener may call back in. |
| `snapshots() throws -> [SnapshotInfo]` | Newest first: `id`, `takenAt`, `reason`. |
| `restoreSnapshot(id:) throws -> Change` | Snapshots the current state first, clears undo history, and returns `everything: true`. |
| `retention()`, `setRetention(retention:)` | `Retention { hourly, daily, monthly, operationDays }`, default 48, 30, 12 and 30. |
| `exportActualZip(to:) async throws -> [AuditFinding]` | R-IO-2. Snapshots a writable budget first, audits, writes beside `to` and renames over it. |
| `setBackupFolder(folder:)`, `backUpNow() throws -> String` | R-REL-4. Swift holds the folder's security-scoped access and sets the folder each time it opens the budget. `backUpNow` writes `YYYY-MM-DD-<budget name>.zip`. The app schedules it; the core never does. Failures land in `state().lastBackup.problem` and never block editing. |
| `maintain() throws -> Change` | The app calls this on a timer, every few minutes. It collects the background check and takes the hourly snapshot when one is due. **After it returns, the shell refetches `state()`**: the read-only reason, `checking`, `auditFindings` and `snapshotProblem` may have changed even when the returned change is empty. |
| `close() throws` | Releases the connection and the writer lock now. Later calls fail with `Invalid`. |

## Change events

`Change { entities: [EntityRef], everything: Bool }`. `EntityRef { kind, id, month }` names one row: `kind` is an `EntityKind`, `id` is absent only for a month's hold, and `month` is set for budget cells and holds. A change naming an account or transaction means the overview's balances changed. `everything` (after a restore, a reload, or the background check turning the budget read-only) means refetch every screen and `state()`.

## Planned next

Queries returning screen models: `budgetMonth(Month)`, `register(accountId, filter)`, `bills()`, `goals()`, `netProfit(range)`, `sankey(range)`, `forecast(horizon, scenarios)`, `wrapped(year)`, `statements(range)` and `taxSummary(year)`. Each arrives through a `contract` PR from the workstream that owns it.
