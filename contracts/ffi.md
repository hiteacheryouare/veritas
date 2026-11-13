# FFI surface (contract v3)

`veritas-ffi` is the only crate the apps link. It exports with UniFFI proc macros, and `cargo xtask bindings` generates the Swift module `VeritasCore`, with its C module `VeritasCoreFFI`, into `apps/macos/Packages/VeritasKit`. Swift never sees SQL, Actual column names or business rules (PLAN §6.1, Appendix C).

## Rules for every export

- Every export returns `Result<_, FfiError>` and goes through `guard`, which catches panics into `FfiError::Internal`. UniFFI turns an infallible export into `try!` in Swift, so a panic there would crash the app.
- Coarse calls only. The UI asks for whole screen models ("budget month 2027-03"), never one cell at a time (PLAN §6.5).
- Records are plain data. Money crosses as `i64` minor units, and VeritasKit formats it for display. Dates cross as `"YYYY-MM-DD"` strings, months as `Month`, and moments as UTC `"YYYY-MM-DDTHH:MM:SSZ"`.
- Long work (open, create, import, export, and later reports and forecasts) is `async` in Swift, runs on a core thread, and takes a `CancelToken`. UniFFI never cancels the Rust side of an `async` call, so cancelling the Swift task alone stops nothing. The caller makes a `CancelToken()` per call and calls `token.cancel()` from `withTaskCancellationHandler`. The work then stops before its one visible step: a cancelled create, import or export leaves nothing behind, and a cancelled open closes the budget again. All answer `FfiError.Cancelled`. A cancel after that step, or after the work finished, does nothing.
- A panic inside a session call reloads the budget from disk before the `Internal` error comes back, so memory always matches the file (R-REL-7).

## `CancelToken` (object)

| Call | Notes |
|---|---|
| `CancelToken()` | A fresh token for one call. |
| `cancel()` | Asks the work to stop. Safe to call any time, from any thread, more than once. |
| `isCancelled() -> Bool` | Whether `cancel()` was called. |

The pattern every wrapper uses:

```swift
let token = CancelToken()
return try await withTaskCancellationHandler {
	try await Session.importActualZip(zip: zip, intoDirectory: folder, cancel: token)
} onCancel: {
	token.cancel()
}
```

## Free functions

| Rust | Swift | Notes |
|---|---|---|
| `core_version()` | `coreVersion() throws -> String` | Proves the bridge links. |
| `parse_month(text)` | `parseMonth(text:) throws -> Month` | `"YYYY-MM"` → `Month { year, month }`. |
| `list_budgets(budgets_dir)` | `listBudgets(budgetsDir:) throws -> [BudgetListing]` | Every budget folder in the directory: `id`, `name`, `folder`, `lastOpened`, most recently opened first. Hidden folders, including budgets still being built, are skipped. |
| `currencies()` | `currencies() throws -> [Currency]` | Actual's currency table, in its order: `code` (empty for "None"), `name`, `symbol`, `decimalPlaces`, `numberFormat` (the style Actual suggests with it) and `symbolFirst`. |
| `evaluate_amount(text, style)` | `evaluateAmount(text:style:) throws -> Int64` | What was typed in an amount field, arithmetic included, the way Actual evaluates it (R-BUD-3): `"12.50+3"` is 1550. `style` decides the decimal separator. Text that isn't a storable amount is `Invalid` with a message to show beside the field, and the field keeps its old value (D37). Too large is `Overflow`. |

## `Session` (object)

One open budget folder. A second window or process on the same folder opens read-only (R-REL-6).

| Call | Notes |
|---|---|
| `Session.open(directory:cancel:) async throws` | Usable as soon as it returns. The full integrity check, the invariant audit and the open snapshot run in the background. |
| `Session.createBudget(name:directory:format:options:cancel:) async throws` | A brand-new budget (R-IO-0, D52 to D56). `NumberFormat { currency, style, hideDecimals }`, where `style` is one of `commaDot`, `dotComma`, `spaceComma`, `apostropheDot` and `commaDotIn`. `NewBudgetOptions { retirement401k, hsa, accounts }`, where each `NewAccount { name, kind, startingBalance }` is in cents. For a `creditCard`, `lineOfCredit`, `loan` or `mortgage`, the balance is what is owed, as a positive amount, and the core stores it negative. The core decides each kind's budget flag and side (D8, docs/finance/statements.md ST-9.2): `checking`, `savings`, `cash`, `creditCard` and `lineOfCredit` are on budget; `investment`, `retirement`, `property`, `loan` and `mortgage` are off budget. A blank account name is `Invalid`, and a starting balance beyond Actual's limit of 2^51 − 1 cents, either sign, is `Overflow`. veritas-store builds the budget from its pinned template with the starter categories and setup's accounts, in a folder beside the target, which is renamed into place once every file is flushed. |
| `Session.importActualZip(zip:intoDirectory:cancel:) async throws` | Copies an Actual export into a new folder, built aside and renamed into place, and opens it. The zip is only read. A copy that SQLite finds damaged is refused with `AuditFailed`. |
| `state() throws -> SessionState` | Everything the shell's chrome shows in one call: `directory`, `readOnly` (the banner's reason), `canUndo`, `canRedo`, `checking` (background check still running), `snapshotProblem`, `skippedRows`, `auditFindings`, `backupFolder`, `lastBackup`. |
| `overview() throws -> Overview` | `budgetName`, `numberFormat` (`nil` until the core reads Actual's preferences, so use Actual's defaults), and the four sidebar groups (`cash`, `credit`, `investments`, `loans`, decided by docs/finance/statements.md ST-9), each with its accounts (`id`, `name`, `balance` in cents, `closed`, `deleted`, `placedByBalance`) and `total`. |
| `auditReport() throws -> [AuditFinding]?` | `nil` until the background check finishes. Findings name rows by id only. Only `damage` makes the budget read-only (D51). |
| `perform(command:) throws -> Change` | Each `Command` is one SQLite transaction (R-REL-1) and one undo step (R-REL-5). v1 has `renameAccount(accountId:name:)`; workstreams add commands through `contract` PRs. |
| `undo() throws -> Change`, `redo() throws -> Change` | Unlimited for the session. |
| `subscribe(listener:) throws -> UInt64`, `unsubscribe(subscription:)` | `ChangeListener` is a Swift protocol. `changed(change:)` is called after every commit, undo, redo, restore or reload, on the calling thread, after the session is released, so the listener may call back in. |
| `snapshots() throws -> [SnapshotInfo]` | Newest first: `id`, `takenAt`, `reason`. |
| `restoreSnapshot(id:) throws -> Change` | Snapshots the current state first, clears undo history, and returns `everything: true`. |
| `retention()`, `setRetention(retention:)` | `Retention { hourly, daily, monthly, operationDays }`, default 48, 30, 12 and 30. |
| `exportActualZip(to:cancel:) async throws -> [AuditFinding]` | R-IO-2. Snapshots a writable budget first, audits, writes beside `to` and renames over it. **In the sandbox, `to` is a file in the app's temporary directory, and Swift then moves it to where the user chose**, through the save panel's URL and the security-scoped access Swift holds. The core writes only in the budgets directory (each budget's folder and, while creating or importing, a hidden folder beside it), the backup folder and the app's temporary directory. |
| `setBackupFolder(folder:)`, `backUpNow() throws -> String` | R-REL-4. Swift holds the folder's security-scoped access and sets the folder each time it opens the budget. `backUpNow` writes `YYYY-MM-DD-<budget name>.zip`. The app schedules it; the core never does. Failures land in `state().lastBackup.problem` and never block editing. |
| `maintain() throws -> Change` | The app calls this on a timer, every few minutes. It collects the background check and takes the hourly snapshot when one is due. **After it returns, the shell refetches `state()`**: the read-only reason, `checking`, `auditFindings` and `snapshotProblem` may have changed even when the returned change is empty. |
| `close() throws` | Releases the connection and the writer lock now. Later calls fail with `Invalid`. |

## Change events

`Change { entities: [EntityRef], everything: Bool }`. `EntityRef { kind, id, month }` names one row: `kind` is an `EntityKind`, `id` is absent only for a month's hold, and `month` is set for budget cells and holds. A change naming an account or transaction means the overview's balances changed. `everything` (after a restore, a reload, or the background check turning the budget read-only) means refetch every screen and `state()`.

## Planned next

Queries returning screen models: `budgetMonth(Month)`, `register(accountId, filter)`, `bills()`, `goals()`, `netProfit(range)`, `sankey(range)`, `forecast(horizon, scenarios)`, `wrapped(year)`, `statements(range)` and `taxSummary(year)`. Each arrives through a `contract` PR from the workstream that owns it.
