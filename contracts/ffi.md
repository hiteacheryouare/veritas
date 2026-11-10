# FFI surface (contract v0)

`veritas-ffi` is the only crate the apps link. It exports with UniFFI proc macros, and `cargo xtask bindings` generates the Swift module `VeritasCore`, with its C module `VeritasCoreFFI`, into `apps/macos/Packages/VeritasKit`. Swift never sees SQL, Actual column names or business rules (PLAN §6.1, Appendix C).

## Exported today

| Rust | Swift | Notes |
|---|---|---|
| `core_version() -> Result<String, FfiError>` | `coreVersion() throws -> String` | Proves the bridge links. |
| `parse_month(text) -> Result<Month, FfiError>` | `parseMonth(text:) throws -> Month` | `"YYYY-MM"` → `Month { year, month }`. Proves typed errors cross the bridge. |
| `FfiError` | `enum FfiError: Error` | See contracts/errors.md. |

## Rules for every export

- Every export returns `Result<_, FfiError>` and goes through `guard`, which catches panics into `FfiError::Internal`. UniFFI turns an infallible export into `try!` in Swift, so a panic there would crash the app.
- Coarse calls only. The UI asks for whole screen models ("budget month 2027-03"), never one cell at a time (PLAN §6.5).
- Records are plain data. Money crosses as `i64` minor units, and VeritasKit formats it for display. Dates cross as `"YYYY-MM-DD"` strings and months as `Month`.
- Long work (reports, forecasts, import, export) runs on the core's background queue and can be cancelled (`FfiError::Cancelled`).

## Shape planned for v1 (frozen at Gate 1)

WS-SESSION owns the implementation. Workstreams add their models through `contract` PRs.

- `Session` (object): `open(path)`, `importActualZip(zip, intoDirectory)`, `exportActualZip(to)`, `close()`, and `isReadOnly` / `readOnlyReason`.
- `session.perform(command: Command) -> CommandOutcome`. Each `Command` is one SQLite transaction (R-REL-1) and one undo step (R-REL-5). `undo()` and `redo()` sit alongside it.
- `session.subscribe(listener: ChangeListener)`. `ChangeListener` is a foreign trait. It receives `Change { entities: [EntityRef] }` after each commit, and view models refetch only what changed.
- Queries returning screen models: `budgetMonth(Month)`, `register(accountId, filter)`, `bills()`, `goals()`, `netProfit(range)`, `sankey(range)`, `forecast(horizon, scenarios)`, `wrapped(year)`, `statements(range)` and `taxSummary(year)`.
