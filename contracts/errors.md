# Error taxonomy (contract v0)

Every failure is a typed error. The core never panics across the FFI (R-REL-7). `veritas_domain::CoreError` is the core's type. `veritas_ffi::FfiError` mirrors it one to one, and Swift receives it as a thrown `FfiError`. A panic caught at the boundary becomes `Internal`.

| CoreError / FfiError | When | What the app does |
|---|---|---|
| `NotFound { entity, id }` | A command names a row that doesn't exist or is tombstoned | Refreshes the screen. The row was likely changed elsewhere. |
| `Invalid(message)` | Input fails validation: a bad date, an amount out of range, an impossible schedule | Shows `message` inline next to the field. Messages are written for Ryan, not developers. |
| `ReadOnly(reason)` | A write to a budget opened read-only | Shows the banner for that reason: `NewerActualVersion` (R-IO-7), `TrackingBudget` (R-BUD-2), `OpenElsewhere` (R-REL-6) or `AuditFailed` (R-REL-2). |
| `UpgradeInActualFirst { required }` | Import of a file missing migration ids Veritas expects (R-IO-7) | Explains: "Open this budget in Actual {required} or newer, then export it again." |
| `AuditFailed { violations }` | The invariant audit found problems on open, import or export | Opens the audit report. Nothing is auto-fixed. |
| `NotAnActualBudget(message)` | A zip without `db.sqlite` and `metadata.json` laid out the way Actual's importer accepts (R-IO-1) | Explains what was found instead. |
| `Storage(message)` | SQLite failed; the command rolled back and memory was restored (R-REL-1) | "Couldn't save that change. Nothing was lost." Retry is offered. |
| `Io(message)` | A file or folder problem: disk full, permissions, a missing backup folder (R-REL-4) | Names the path and what to check. Backup failures never block editing. |
| `Overflow` | Money arithmetic left the i64 range | "That amount is too large." |
| `Cancelled` | A report or forecast was cancelled because the user navigated away | Nothing. |
| `Internal(message)` | A bug, including a caught panic | "Veritas hit a bug." Offers to copy the details. Data is untouched. |

## Rules

- New variants need a `contract` PR that updates both enums, the `From` mapping in `veritas-ffi`, and this table.
- Messages never contain amounts, payee names or notes from the budget. Errors can end up in logs.
