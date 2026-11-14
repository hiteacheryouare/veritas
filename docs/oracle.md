# The Actual oracle

`tools/actual-oracle/` drives Actual's own engine (`@actual-app/api`, pinned to exactly `26.11.0`) so Veritas can be checked against it. It is a dev tool and never ships. Its commands:

| Command | Does |
|---|---|
| `dump <zip> [--out <file>] [--raw-only]` | Writes the canonical dump of an Actual export: every table as stored in the zip, plus the budget values Actual computes for it. |
| `apply <zip> <ops.json> --out <zip>` | Imports the zip, runs an op list through Actual and exports the result. |
| `generate <small\|all-features\|perf-100k> [--out <zip>]` | Builds a synthetic fixture into `fixtures/synthetic/<spec>.zip`. |
| `replay <zip> <sessions.json> --out <results.json>` | Runs many op lists in one Actual process, each on a fresh import of the zip, and writes one result per list: `error` (the first failing op's message, or `null`), the budget `months` as in `computed`, and the raw `zeroBudgets`, `zeroBudgetMonths` and `budgetNotes` rows. After each op it waits one timer turn, because some of Actual's work outlives the call that started it: the 6- and 12-month averages don't await their writes (`budget/actions.ts:311-357`) and the sheet recomputes on a later tick. A person's next click always comes later, and without the wait the next op reads stale cells. `apply` and `generate` don't wait, so the committed fixtures stay byte-identical. The engine's differential tests use `replay` for thousands of random sessions. |
| `eval <cases.json> [--out <file>]` | Runs Actual's `evalArithmetic` on a JSON array of `{"format", "expression"}` cases, each under its own number format. Results are JavaScript's number text (`"Infinity"`, `"-0"`) or `null`. |
| `arithmetic-vectors [--out <file>]` | Rewrites `crates/veritas-engine/tests/fixtures/actual-arithmetic-vectors.json`, the vectors the engine's arithmetic tests compare against bit for bit. |

`eval` and `arithmetic-vectors` can't use the bundle: `@actual-app/api` tree-shakes `evalArithmetic` and `currencyToAmount` away. They import the pinned, unmodified `packages/loot-core/src/shared/arithmetic.ts` and `util.ts` from `reference/actual` (or `ACTUAL_REFERENCE_DIR`) through a module hook that adds the `.ts` extension and resolves `date-fns` from the oracle's own `node_modules` (`src/reference.ts`). They refuse to run, naming `cargo xtask reference`, when that checkout is missing or isn't at the manifest's commit. `npm test` checks that `arithmetic-vectors` reproduces the committed file and skips those tests when the checkout is missing.

`widgets <zip> <requests.json>` runs Actual's own dashboard card code from the same checkout: each request is `{id, type, meta, firstDayOfWeekIdx}`, and the oracle resolves the card's time range with `calculateTimeRange` and runs the card's spreadsheet from `packages/desktop-client/src/components/reports`. The hook resolves `@actual-app/core/*` and the `#` aliases through the checkout's own `package.json` files, every other package from the oracle's `node_modules`, and swaps the client connection for `src/shims/connection.ts`, which forwards each `send` to the open Actual session. Output is canonical JSON with dates as `yyyy-MM-dd` and non-integer numbers as their JS text, so readers that round a float's last digit can't make a false mismatch. A card that throws comes back as `{error}`.

`dashboards <zip>` imports the zip and returns what Actual's dashboard and report pages read: the live `dashboard_pages` and `dashboard` rows through AQL (with `meta` parsed) and the saved reports through `report/get`. The dashboard journey test compares it with what Veritas shows after its edits.

All commands also take `--today <YYYY-MM-DD>` (default `2026-11-09`) and `--seed <n>` (default `1`).

## Running it

```sh
cd tools/actual-oracle
npm ci                      # installs only @actual-app/api 26.11.0 and its tree, no install scripts
node src/main.ts dump ../../fixtures/synthetic/small.zip --out /tmp/small.json
npm test
```

Node 24 or newer runs the TypeScript directly (type stripping) and provides `node:sqlite`, so there is no build step and no other package. `.npmrc` sets `ignore-scripts=true`: the only native module, `better-sqlite3` 13, ships N-API prebuilds inside its tarball and has no install script, and nothing else in the tree has one either.

## Determinism and isolation

Every command runs Actual inside `withActual` (`src/environment.ts`), which before Actual loads:

- **Pins today.** `Date` is replaced by a proxy whose "now" starts at local noon of `--today` and advances 1 ms per thousand reads. Actual reads the clock for today's date, budget bounds, schedule next dates, `sort_order`, movement notes and the zip's file times. The slow tick keeps CRDT timestamps increasing without crossing midnight.
- **Sets `TZ=UTC`**, so the same command gives the same bytes on any machine.
- **Seeds randomness.** `crypto.randomUUID`, `crypto.getRandomValues` and `Math.random` come from one seeded generator. Actual generates ids, the budget's directory id and the CRDT node id with them, and `Math.random` decides the `kvcache_key` value. Export empties that table, but the deleted row's bytes stay in a free page of the db file, so it has to be seeded for the zip to be byte-identical.
- **Blocks the network.** `fetch` is replaced by a tripwire that records the URL and throws, and a command fails if anything was recorded. The bundled API only reaches the network through `fetch` (its other built-in requires are `fs`, `path`, `os`, `crypto`, `events`, `stream` and `timers`). `init` is called without `serverURL`, which makes Actual call `setServer(null)`.
- **Uses a throwaway data dir.** Actual's `dataDir` is a fresh `mkdtemp` under `os.tmpdir()`, removed when the command ends. Raw dumps read the zip's db from a second temp dir. The tests run with `TMPDIR`, `HOME` and the working directory pointed at empty dirs and check that all three are still empty afterwards.
- **Keeps stdout clean.** Actual logs a lot, so `console.log`, `info` and `warn` go to stderr. A dump written to stdout is the only thing on stdout.

`npm test` also runs `generate` and `dump` under `sandbox-exec` with `(deny network*)` (macOS only) and checks the output is byte-identical to the committed fixture.

## Canonical dump, version 1

A dump is one JSON document with this shape:

```jsonc
{
	"computed": { ... },               // left out with --raw-only
	"format": "actual-canonical-dump",
	"metadata": { ... },
	"schema": [ ... ],
	"tables": { "<table>": { "primaryKey": [ ... ], "rows": [ ... ] } },
	"version": 1
}
```

The raw part (`metadata`, `schema`, `tables`) comes only from the bytes in the zip. It opens the zip's `db.sqlite` read-only with `node:sqlite` and never lets Actual touch it, so migrations, view rebuilds and clock resets don't show up in it. The Rust `canonical_dump` in `veritas-interop` mirrors exactly this part. Only the oracle produces `computed`.

### Locating the files

`db.sqlite` and `metadata.json` are taken from the zip root, or else from the one folder that holds both, the same rule as Actual's `importBuffer` (`reference/actual/packages/loot-core/src/server/cloud-storage.ts:223`). Zip entries may be stored or deflated.

### `metadata`

The parsed `metadata.json` with these keys removed, because they describe one local copy rather than the budget: `cloudFileId`, `encryptKeyId`, `groupId`, `id`, `lastSyncedTimestamp`, `lastUploaded`, `userId`. Every other key, including unknown ones, is kept.

The file goes through `JSON.parse`, so its values are re-encoded rather than copied. Strings, booleans and `null` survive exactly. Numbers become IEEE-754 doubles and are printed with ECMAScript `Number.prototype.toString`: `1.0` prints as `1`, `1e3` as `1000`, and an integer above 2^53 loses precision. A metadata number never gets the `.0` suffix that marks a SQLite REAL. Duplicate keys keep the last value.

### `schema`

One entry per `sqlite_master` row whose name doesn't start with `sqlite_`: `{"name", "sql", "tableName", "type"}`. Entries are sorted by `type`, then `name`. It covers tables, indexes, views and triggers, including the `kvcache` tables.

### `tables`

Every table in `sqlite_master` except `kvcache`, `kvcache_key` and names starting with `sqlite_` (`sqlite_sequence`, `sqlite_stat1`, `sqlite_stat4`). Each entry has:

- `primaryKey`: the primary key columns in key order, from `PRAGMA table_info` (`pk > 0`, sorted by `pk`). It is empty for a table without a declared key.
- `rows`: every row, tombstoned ones included, from `SELECT * FROM t ORDER BY <pk columns> COLLATE BINARY`. A table without a primary key orders by all its columns in declaration order. Rowids that aren't a declared column are not dumped.

A row is an object with one key per column. Its values keep the SQLite storage class:

| Storage class | Encoding | Example |
|---|---|---|
| NULL | `null` | `null` |
| INTEGER | JSON integer, every digit kept (64-bit) | `20261030` |
| REAL | JSON number that always has a `.` or an exponent | `16384.0`, `0.5`, `1e+21` |
| REAL ±∞ | `{"$real":"Infinity"}` or `{"$real":"-Infinity"}` | |
| TEXT | JSON string, decoded from UTF-8 (see below) | `"Rent"` |
| BLOB | `{"$blob": "<lowercase hex>"}` | `{"$blob":"00ff"}` |

TEXT bytes are decoded as UTF-8, and each invalid sequence becomes one U+FFFD per maximal subpart. That is the WHATWG decoder's rule and the one Rust's `String::from_utf8_lossy` follows. Examples: `F0 80 80` gives three U+FFFD, `E2 82 41` gives U+FFFD then `A`, `ED A0 80` (an encoded surrogate) gives three U+FFFD.

A REAL is printed with ECMAScript `Number.prototype.toString` (the shortest digits that round-trip, exponent form below 1e-6 and from 1e21 up, written `1e+21`/`1e-7`), with `.0` appended when that text has neither `.` nor `e`. `-0.0` prints as `0.0`. JSON text columns such as `rules.conditions` stay strings; they are never parsed.

### `computed`

Values come from Actual after `api.importBudget` loads the zip into the scratch dir. All amounts are integer cents.

```jsonc
"computed": {
	"accounts": { "<account id>": { "balance": 622330 } },
	"budgetType": "envelope",
	"months": {
		"2026-10": {
			"categories": {
				"<expense category id>": { "balance": 1731, "budgeted": 9000, "carryover": false, "spent": -8423 },
				"<income category id>": { "received": 400000 }
			},
			"forNextMonth": 0, "fromLastMonth": 447343, "incomeAvailable": 847343, "lastMonthOverspent": -657,
			"toBudget": 670686, "totalBalance": 1644, "totalBudgeted": -176000, "totalIncome": 400000, "totalSpent": -175890,
			"groups": { "<group id>": { "balance": 1731, "budgeted": 9000, "spent": -8423 } }
		}
	},
	"schedules": { "<schedule id>": { "completed": false, "nextDate": "2026-12-03", "upcoming": ["2026-12-03", "..."] } },
	"today": "2026-11-09"
}
```

- `months` has one entry per month in `api.getBudgetMonths()`. The first month is three months before the earlier of the current month and the earliest transaction date. The earliest date counts tombstoned transactions but not split children. The last month is eleven months after the current month, because the API's list leaves out the final month of the budget bounds (`reference/actual/packages/loot-core/src/server/budget/base.ts:423`, `reference/actual/packages/loot-core/src/server/api.ts:451`). Each month is `api.getBudgetMonth(month)` with names and flags dropped and categories pulled out of their groups. The fields are whatever Actual returns for the budget type. In envelope budgets that means `budgeted`, `spent`, `balance` and `carryover` for expense categories and `received` for income ones.
- `accounts` covers every account that isn't tombstoned, closed ones included. `balance` is `api.getAccountBalance(id)`, which counts transactions dated up to today.
- `schedules` has `nextDate` (Actual's `next_date`), and `upcoming` is the next 12 dates from today via `api.getScheduleDates`. `upcoming` is `[]` for a completed schedule, and `null` when Actual can't expand the schedule's date (for example an impossible start date).
- `computed` reflects what Actual shows right after opening the file. It does not auto-post schedules. Use the `runSchedules` op for that.

### Text layout

The output is meant to be compared byte for byte, so the layout is fixed:

- Object keys are sorted by Unicode code point (UTF-8 byte order).
- Objects are printed one key per line, indented with tabs, as `"key": value`.
- An array is printed one element per line, and each element is written compactly on that line (`{"a":1,"b":[2,3]}`, no spaces, keys sorted). This puts each table row on its own line.
- Empty objects and arrays are `{}` and `[]`. The file ends with one newline.
- Numbers outside SQLite cells (in `metadata` and `computed`) are printed with ECMAScript `Number.prototype.toString`.
- Strings, keys included, are wrapped in `"` and escape exactly these characters: `"` → `\"`, `\` → `\\`, U+0008 → `\b`, U+0009 → `\t`, U+000A → `\n`, U+000C → `\f`, U+000D → `\r`. The rest of U+0000 to U+001F become `\u00xx` with lowercase hex, and a lone surrogate (which can only come from a `\ud800`-style escape in `metadata.json`) becomes `\udxxx` with lowercase hex. Everything else is written as raw UTF-8, including `/`, U+007F, U+2028, U+2029 and all non-ASCII.

`test/canonicalJson.test.ts` pins these rules with small examples the Rust side can copy.

## Op files

```json
{ "version": 1, "ops": [ { "op": "setBudgetAmount", "month": "2026-10", "category": "<id>", "amount": 10000 } ] }
```

Ops run in order. The first failure stops the run and names the op's index. Amounts are integer cents, dates are `YYYY-MM-DD`, months are `YYYY-MM`.

**Ids.** Every op that creates something takes the new entity's `id`, so Veritas and the oracle can replay the same file and compare by id. Most of Actual's create handlers generate their own id, always as their first `uuid` call. The oracle first checks that no row in the target table already has the id. Actual's insert is a series of column writes, and a write to an existing row becomes an `UPDATE` (`reference/actual/packages/loot-core/src/server/sync/index.ts:142`), so a reused id would overwrite that row without any error. It then hands the id to Actual's first uuid call and checks afterwards that a row with that id exists in the expected table. If either check fails, the op fails. Ids that Actual creates as side effects (transfer payees, the counterpart leg of a transfer, a schedule's rule, mapping rows) come from the seeded generator. Refer to a transfer's counterpart leg as `{"transferOf": "<id of the leg you created>"}`. Refer to an account's transfer payee as `{"transferTo": "<account id>"}` wherever a transaction takes a `payee`.

| Op | Fields | Runs |
|---|---|---|
| `createAccount` | `id`, `name`, `offBudget?`, `closed?`, `initialBalance?` | `api.createAccount`. Its starting balance is dated today. |
| `updateAccount` | `id`, `fields` (API names: `name`, `offbudget`, `account_group_id`, …) | `api.updateAccount` |
| `closeAccount` | `id`, `transferAccount?`, `transferCategory?` | `api.closeAccount` |
| `reopenAccount` | `id` | `api.reopenAccount` |
| `createAccountGroup` | `id`, `name` | `api.createAccountGroup` |
| `createCategoryGroup` | `id`, `name`, `hidden?` | `api.createCategoryGroup` |
| `updateCategoryGroup` | `id`, `fields` | `api.updateCategoryGroup` |
| `createCategory` | `id`, `name`, `group`, `isIncome?`, `hidden?` | `api.createCategory` |
| `updateCategory` | `id`, `fields` (`name`, `hidden`, `group_id`) | `api.updateCategory` |
| `deleteCategory` | `id`, `transferCategory?` | `api.deleteCategory` |
| `createPayee` | `id`, `name` | `api.createPayee` |
| `mergePayees` | `target`, `merge` (ids) | `api.mergePayees` |
| `createPayeeLocation` | `id`, `payee`, `latitude`, `longitude` | `internal.send('api/payee-location-create')`. The API has no exported method for it. |
| `addTransactions` | `account`, `transactions`, `runTransfers?` (default true), `learnCategories?` (default false) | `api.addTransactions`. Transactions use the API's field names (`id`, `date`, `amount`, `payee`, `payee_name`, `category`, `notes`, `cleared`, `subtransactions` with their own `id`s). Actual runs the rules on each one. |
| `updateTransaction` | `id` (or `{transferOf}`), `fields` | `api.updateTransaction` |
| `deleteTransaction` | `id` (or `{transferOf}`) | `api.deleteTransaction`. It tombstones both legs of a transfer. |
| `setBudgetAmount` | `month`, `category`, `amount` | `api.setBudgetAmount` |
| `setCarryover` | `month`, `category`, `flag` | `api.setBudgetCarryover`. It applies from that month onward. |
| `holdForNextMonth` | `month`, `amount` | `api.holdBudgetForNextMonth` |
| `resetHold` | `month` | `api.resetBudgetHold` |
| `transferAvailable` | `month`, `amount`, `category` | `internal.send('budget/transfer-available')`, which moves To Budget into a category |
| `moveBudget` | `month`, `from`, `to` (category id or `"to-budget"`), `amount`, `currencyCode?` | `internal.send('budget/transfer-category')`. It also appends a movement line to the `budget-<month>` note. |
| `coverOverspending` | `month`, `from`, `to`, `amount?`, `currencyCode?` | `internal.send('budget/cover-overspending')` |
| `budgetAction` | `action`, `args` | `internal.send('budget/<action>', args)` for one of Actual's envelope budget handlers, with the arguments its client sends (`reference/actual/packages/desktop-client/src/budget/mutations.ts:780-900`): `budget-amount`, `copy-previous-month`, `copy-single-month`, `set-zero`, `set-3month-avg`, `set-6month-avg`, `set-12month-avg`, `set-n-month-avg`, `hold-for-next-month`, `reset-hold`, `cover-overspending`, `transfer-available`, `cover-overbudgeted`, `transfer-category`, `copy-until-year-end`, `set-carryover`, `reset-income-carryover`. It skips the API's month and amount checks. |
| `applyTemplates` | `month` | `internal.send('budget/apply-goal-template')`, which stores `#template` notes as `goal_def` and fills unbudgeted categories |
| `storeNoteCleanups` | | `internal.send('budget/store-note-cleanups')`, which stores `#cleanup` notes as `cleanup_def` and `cleanup_groups` |
| `createRule` | `id`, `stage?`, `conditionsOp?`, `conditions`, `actions` | `api.createRule`, which keeps the supplied id |
| `createSchedule` | `id`, `name`, `date` (`YYYY-MM-DD` or a RecurConfig), `postsTransaction?`, `payee?`, `account?`, `amount?`, `amountOp?` | `api.createSchedule` |
| `runSchedules` | | `internal.send('schedule/force-run-service')`, which auto-posts due schedules that post transactions |
| `skipSchedule` | `id` | `internal.send('schedule/skip-next-date')`, which moves the next date past one occurrence |
| `postSchedule` | `id`, `today?` | `internal.send('schedule/post-transaction')`, dated `next_date`, or the current day when `today` is true |
| `updateSchedule` | `id`, `fields?` (`schedules` columns such as `completed` or `posts_transaction`), `conditions?`, `resetNextDate?` | `internal.send('schedule/update')`, which merges the conditions with `updateConditions` and may reset the next date |
| `setNote` | `id` (an entity id, `account-<id>`, `budget-<month>`, …), `note` | `api.updateNote` |
| `createTag` | `id`, `tag`, `color?`, `description?` | `api.createTag` |
| `createCustomReport` | `id`, `report` or `reportText` (a `CustomReportEntity` without `id`; as text, nested JSON such as `conditions` keeps its key order) | `internal.send('report/create')` |
| `updateCustomReport` | `id`, `setText`, `appendConditionsText?` | `internal.send('report/update')` with the stored entity from `report/get`, the conditions appended and the fields in `setText` over it, as the report editor saves |
| `createDashboardPage` | `id`, `name` | `internal.send('dashboard-create')` |
| `renameDashboardPage` | `id`, `name` | `internal.send('dashboard-rename')` |
| `deleteDashboardPage` | `id` | `internal.send('dashboard-delete')`, which tombstones the page and its widgets and refuses the last page |
| `addDashboardWidget` | `id`, `page`, `type`, `width`, `height`, `x?`, `y?`, `meta?` or `metaText?` | `internal.send('dashboard-add-widget')`, which keeps the supplied id |
| `updateWidgetMeta` | `id`, `setText`, `appendConditionsText?` | `internal.send('dashboard-update-widget')` with the stored meta, the conditions appended and the keys in `setText` over it, as a card's settings save |
| `moveWidget` | `id`, `x`, `y`, `width`, `height` | `internal.send('dashboard-update-widget')` with the frame only |
| `removeWidget` | `id` | `internal.send('dashboard-remove-widget')` |
| `createFilter` | `id`, `name`, `conditions`, `conditionsOp?` | `internal.send('filter-create')` |
| `setPreference` | `id`, `value` | `api.setPreference`. Setting `budgetType` switches between envelope and tracking. Actual rebuilds the sheet for the new type without awaiting it (`reference/actual/packages/loot-core/src/server/sync/index.ts:552`), so the oracle then recreates the budgets and reopens the file. |

`apply` loads the zip normally, so Actual's month checks apply: a budget month must lie inside the budget's bounds, which start three months before the earliest transaction. Add transactions before budgeting their months.

## Fixtures

Each spec in `src/specs/` is an op list built from fixed dates and a fixed seed. Its ids are name-based UUIDs (`sha256("<spec>:<kind>:<name>")` shaped as v4). `generate` creates an empty budget with `api.runImport`, applies the ops with the same code path as `apply`, and exports. Regenerating gives byte-identical zips, which `npm test` checks for `small` and `all-features`.

| Fixture | Size | Contents |
|---|---|---|
| `small.zip` | 15.7 KB | 2 accounts, 4 categories, 3 months of paychecks, rent, groceries and a transfer, with budgets |
| `all-features.zip` | 77 KB | See below |
| `perf-100k.zip` | 20.4 MB (57.8 MB db) | 105,186 transactions (3% split) over 2011-11 to 2026-10, 502 categories in 25 groups (one hidden group, a hidden category per group), 409 payees, 14,400 budget cells. Generating it takes about 70 s and 3.4 GB of RAM. A full dump takes about 5 s and is 75 MB. |

`all-features` covers twelve months (2025-11 to 2026-10) and populates:

- **Accounts:** account groups; Checking, Savings, a Wallet with a starting balance (`starting_balance_flag`), an on-budget Visa card that carries debt, an off-budget Brokerage, an off-budget Car Loan, and a closed account.
- **Transactions:** splits; split loan payments whose principal child is a transfer to the loan; transfers on↔on, on→off and off→on; refunds; uncleared rows; tombstoned rows (a deleted transfer removes both legs); an edit to a transfer's counterpart; merged payees.
- **Categories:** a hidden group, a hidden category with activity, a deleted category merged into another (`category_mapping`), and an income category of our own.
- **Budget:** carryover flags, overspending with and without carryover, budget moves, transfer-from-available, cover-overspending, hold and reset hold (`zero_budget_months`), `#template` goals (`goal_def`, `zero_budgets.goal`) and `#cleanup` groups. It also makes a detour into a tracking budget, so `reflect_budgets` has rows, and the file ends as an envelope budget.
- **Rules:** a pre-stage rename, an `or` rule, a templated `set` (Handlebars) plus `append-notes`, and a formula `set`. Veritas won't execute the last three, and they are tied to a payee with no transactions so they never fire.
- **Schedules:** auto-post monthly rent; confirm-first internet that moves weekends earlier; a gym schedule on the 31st with an approximate amount that ends after 24 occurrences; a Feb 29 yearly schedule with an end date; a twice-monthly paycheck; a one-off.
- **Other:** notes (category, account, month movement notes, an orphan note), tags, a payee location, a saved filter, synced preferences, two custom reports, and a second dashboard page with net-worth, cash-flow, spending, custom-report, markdown, summary, calendar, sankey, formula and one unknown widget type.

Tables still empty in `all-features`, and why:

- `banks`: written only by bank-sync linking.
- `created_budgets`, `pending_transactions`: legacy tables nothing in 26.11.0 writes.
- `messages_crdt`, `messages_pending`: written only while syncing with a server.
- `kvcache`, `kvcache_key`: emptied by every export and excluded from dumps.

## What the oracle found about Actual 26.11.0

These go to P0.4 (`docs/actual-format.md`) and the plan's Appendix A.

- **Offline and headless works**, with one trap. Without `serverURL`, `api.shutdown()` (and `api.sync()`) runs a full sync that does nothing, but it reports `success`. That makes Actual auto-post due schedules and save `lastScheduleRun` (`reference/actual/packages/loot-core/src/server/schedules/app.ts:860`), and the API doesn't await either one, so a caller that deletes the data dir right after shutdown races a metadata write. The oracle closes the budget before shutdown. The same path is why schedules auto-post in the desktop app without a server.
- **`runImport` logs `FileUploadError: unauthorized`.** It ends with `cloudStorage.upload()`, which gives up on the missing user token before any request is made.
- **The create API doesn't take ids.** `createAccount`, `createCategory`, `createCategoryGroup`, `createPayee`, `createSchedule`, `createTag` and `createAccountGroup` ignore a supplied id. `createRule`, `addTransactions` (including `subtransactions`) and `dashboard-add-widget` keep one.
- **`api.addTransactions` runs rules** on every transaction it adds (`reference/actual/packages/loot-core/src/server/accounts/sync.ts:1057`). The desktop register runs `rules-run` itself before `transactions-batch-update`.
- **`payee_name` also fills `imported_description`** when `addTransactions` creates the payee.
- **Actual's own round trip isn't exact.** On import, `resetClock` gives the file a new CRDT node id, so `messages_clock` differs after Actual imports and re-exports a file (`reference/actual/packages/loot-core/src/server/budgetfiles/app.ts:667`). Every other table is identical. Veritas has to keep `messages_clock` untouched. Comparisons made after Actual imports a Veritas export have to ignore the node id.
- **Deleted bytes ride along in exports.** Export deletes the `kvcache` rows (`reference/actual/packages/loot-core/src/server/cloud-storage.ts:170`) but doesn't vacuum, so the db file still holds their old bytes.
- **Monthly schedules skip short months.** A monthly schedule starting on the 31st has no date in months without a 31st: from Jan 31 it goes Mar 31, May 31, Jul 31, Aug 31 and so on. It never moves to the 30th or the 28th. This matters for edge case 13.
- **`createSchedule` doesn't validate the date** (it stored `2026-02-29`), but `getScheduleDates` rejects it later.
- **Appendix A** says "about 61 migrations". A 26.11.0 file records 60 `__migrations__` ids.
