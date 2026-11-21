# Acceptance

Each criterion from PLAN §10, with the evidence for it. Last checked on ws/qa at `4a11f6d`, with dev merged in (2026-11-20), plus open PR #85 where a row names it.

| Status | Meaning |
|---|---|
| **proven** | A test exercises the whole criterion and fails if it breaks. |
| **partial** | Some clauses are proven. The row says which aren't. |
| **gap** | Nothing proves it, or the app contradicts it today. |
| **needs Ryan** | Only Ryan's data or hands can prove it. The row says what to do. |

## Open items

Of the 25 criteria, 22 are proven (D3 and D4 once #84 and #85 land), 2 have a fix in progress (C1, D2), and 1 needs Ryan (A1: run the real-core import UI test). Perf is partly proven (see [Performance](#performance)). WS-QA's tests found six bugs, all fixed on this branch (see [Bugs found](#bugs-found)).

In progress:
- **C1:** the report drill-down needs a transfer flag on `ReportTransaction` (core); #87 has the Mac side.
- **D2:** the Paid and Post messages need core wording; #87 has the bill editor.

Not covered here: PLAN §8 Phase 3's accessibility pass, and the rest of the shadow run, two weeks of use alongside Actual and an import into the Actual app.

## What CI runs

- **Rust:** `cargo test --workspace` with `VERITAS_REQUIRE_ORACLE=1`, so an oracle test fails rather than skips. Two harnesses run small in CI and full size by hand (see [Full-size runs](#full-size-runs)).
- **Swift:** `swift test` for VeritasKit only. VeritasMacUI tests run in `cargo xtask ci` on a Mac, not on GitHub.
- **XCUITests:** they don't run on hosted runners. Ryan runs them. The table marks them "UI test".
- **Perf:** the `perf` job runs `cargo xtask perf`, which fails when a core-measurable R-PERF budget is exceeded. See [Performance](#performance).

Paths below are relative to the repo. `plan_journeys` is `crates/veritas-ffi/tests/plan_journeys.rs`.

## Interop and safety

| # | Criterion | Evidence | Status |
|---|---|---|---|
| A1 | First launch offers New budget and Import from Actual equally, and a UI test walks both to an open budget. | UI tests in `apps/macos/VeritasUITests/NewBudgetJourneyTests.swift`: `testFirstLaunchOffersBothWaysInEqually`, `RealCoreNewBudgetTests.testANewBudgetIsCreatedByTheCoreWithItsAccounts`, and `testImportingAnActualExportOpensItThroughTheCore`, which picks `fixtures/synthetic/small.zip` in the open panel and imports it through the real core. | **needs Ryan.** Run the three UI tests. "Equally" is checked as both buttons enabled with neither the default; size and order aren't measured. |
| A2 | A budget made in Veritas exports to a zip Actual imports with no errors, showing the same balances and budgets. | `plan_journeys::a_budget_started_in_veritas_and_used_for_a_month_reads_the_same_in_actual`: three accounts with starting balances (a card owing), the starter categories, budgets and a move, paychecks, card swipes, a split, a card payment and a savings transfer. Actual's `importBudget` reads the export, and every month's to-budget and every envelope's budgeted, spent and available, plus every balance, equal what Veritas showed. Also `crates/veritas-interop/tests/create.rs`. | **proven** |
| A3 | Ryan's export imports and re-exports with an identical canonical dump. | `cargo xtask shadow` (`tools/shadow`) on Ryan's export, 2026-11-20 (ws/qa 9b7998c): Veritas's dump 1,179 lines, 0 differ; Actual's dump 0 sections differ. It compares with both Veritas's dumper and Actual's, without the `veritas_*` tables that adopting adds. On synthetic files: `crates/veritas-interop/tests/round_trip.rs` and `oracle_parity.rs` (small, all-features, perf-100k), and `tools/shadow/tests/privacy.rs`. | **proven** |
| A4 | After a scripted session, Actual shows the same balances, budgets, schedules and report config. | `plan_journeys::a_scripted_session_of_every_kind_of_edit_reads_back_in_actual` on all-features, in one session: a budget set and moved, a transfer to a new account, a $400 loan payment split $320/$80 by the wizard, a new confirm-first bill, a goal funded, spent and closed, and a renamed custom report set to yearly. Actual computes every month and balance as Veritas did, schedules the bill on the same date with `posts_transaction` 0, reads the payment as a categorized split, and holds the report's new name and interval. | **proven** |
| A5 | An unknown table and column survive import, edit and export. | `crates/veritas-interop/tests/edited_export.rs::an_unknown_table_and_column_survive_an_edit_of_the_same_row_r_io_5`, `round_trip.rs::an_unknown_table_and_column_survive_the_round_trip_r_io_5`, `passthrough.rs`. | **proven** |
| A6 | A newer Actual file opens read-only with an explanation, and Veritas can't modify it. | `plan_journeys::a_newer_actual_file_opened_through_the_api_refuses_every_edit_and_is_never_written`: the session reports `NewerActualVersion`, each kind of edit is refused with it, and the file is byte-identical after close. `crates/veritas-interop/tests/import.rs::a_newer_actual_file_opens_read_only_and_is_never_written_r_io_7`. The banner's words: `VeritasKitTests/ErrorPresentationTests.swift`, and the UI test `ReadOnlyAndErrorTests.testEveryReadOnlyReasonShowsItsBanner` on stub data. | **proven** |
| A7 | 1,000 random SIGKILLs during edits: no integrity or audit failure, and the last commit is always there. | `crates/veritas-session/tests/crash.rs`. Full run at `28ee97e`: 1,000 kills while editing (the budget reached edit 3,869), 1,000 during budget creation (3,461 made, 640 killed mid-build), and 1,000 rounds of a second window through a writer crash. All passed. CI runs 25 of each. | **proven.** The edits are an account rename and a budget cell, with undo, redo and snapshots. |
| A8 | Every risky operation snapshots first, and restoring brings back the exact prior state. | Restore exactness: `crates/veritas-session/tests/snapshots.rs::restoring_after_a_bulk_edit_brings_back_every_row_and_schema_object`. Before import, export and rule apply: `crates/veritas-session/tests/session.rs`. Before bulk edits, each restored to the exact prior state: `plan_journeys::merging_payees_snapshots_the_budget_first`, `moving_a_categorys_history_snapshots_the_budget_first`, `applying_next_months_suggestions_snapshots_the_budget_first`. The last two found the gap that 39e4d3e fixed. | **proven.** `SnapshotKind::VersionChange` is never taken, because Veritas has no version-change path yet. |
| A9 | The scheduled backup is a valid Actual zip in the chosen folder; removing the folder alerts and loses nothing. | `plan_journeys::a_backup_is_a_file_actual_opens_and_a_removed_folder_never_blocks_editing`: Actual imports the backup and shows the same numbers, no temporary file is left, a removed folder gives a typed error, and an edit made afterwards survives a reopen. Schedule and banner: `VeritasKitTests/SettingsAndReportTests.swift` (`aDueScheduleBacksUpOnOpenAndAnUnduOneDoesnt`, `schedulesFollowTheirInterval`, `aFailedBackupShowsTheCoresProblemAndNeverAlerts`). `crates/veritas-session/tests/backup.rs`. | **proven.** The Architect ruled that the persistent banner a failed backup raises counts as §10's "alert". The hourly loop in `WorkspaceModel` is untested; the schedule's due-on-open check is. |

## Budget and goals

| # | Criterion | Evidence | Status |
|---|---|---|---|
| B1 | For every fixture and 10k random op sequences, to-budget and every category's budgeted, spent and available match Actual for every month. | Fixtures: `crates/veritas-engine/tests/oracle_fixtures.rs` (small, all-features, perf-100k). Random sequences: `oracle_sessions.rs`, with ops for budget actions, holds, carryover, averages, copies, transactions added, moved, deleted and recategorized, splits, and goal closes. Full run at `28ee97e`: 10,000 sequences on small and 10,000 on all-features, all matching Actual (seed 1, 778 s). CI runs 100 per fixture. On Ryan's budget, the shadow run (2026-11-20) compared 37 months and 1,887 cells against Actual with 0 differing, and 10 account balances with 0 differing. | **proven.** Random sequences don't run on perf-100k. |
| B2 | `100.00`, type `+12.34`, Return gives `112.34`, in budget cells and transaction amounts. | Engine: `crates/veritas-engine/tests/field_journeys.rs`, `amount_field.rs`. Budget cell on the real core: `VeritasKitTests/RealCoreBudgetJourneyTests.swift`. Transaction amount on the real core: `VeritasKitTests/RealCoreAccountsTests.swift::typingPlusAnAmountAfterATransactionsAmountAddsToIt` (100.00, then `+`, then `+12.34`, saves −112.34). Real text field: `VeritasMacUITests/AmountFieldTypingTests.swift`. UI test `BudgetJourneyTests.testAppendingToABudgetedAmountAndMovingToTheNextEnvelope`. | **proven** |
| B3 | A $1,500 goal in 6 months suggests $250; the purchase makes it reached/spent; closing releases by a budget move, changes no transaction and hides the category; it still shows in last year's Net Profit and Wrapped. | `plan_journeys::a_goal_closed_this_year_keeps_its_transactions_and_still_shows_in_last_years_reports`: suggestion 25000 over 6 months, reached, then reached/spent at 2000 left, close adds 2000 to To Budget, every register row unchanged, envelope hidden; the next January, last year's Net Profit and Wrapped list it as a closed goal with 1,480.00, Wrapped counts it completed, and Actual agrees. Also `VeritasKitTests/RealCoreGoalJourneyTests.swift`. | **proven** |
| B4 | Deleting a category with history defaults to Archive and changes no transaction. | `VeritasKitTests/RealCoreBudgetJourneyTests.swift::archivingAnEnvelopeWithHistoryHidesItAndChangesNoNumbers`, `onlyAnEnvelopeWithNoHistoryIsReallyDeleted`. UI test `GoalsAndEnvelopesJourneyTests.testDeletingAnEnvelopeWithHistoryArchivesItInstead`. | **proven.** No test checks that Archive is the default button. |

## Transfers, debt and reports

| # | Criterion | Evidence | Status |
|---|---|---|---|
| C1 | Every transfer row in every register shows `→ To X` or `← From X`. | `VeritasKitTests/AccountsTests.swift::transfersReadAsToAndFromNeverAsAPayee`. UI test `AccountsJourneyTests.testTransfersReadAsToAndFromInTheRegister`, on one account register with stub data. | **fix in progress.** #87 labels transfer payees in the bill and rule pickers as the register does. The report drill-down's arrow needs a transfer flag on the core's `ReportTransaction`. The All, On budget and Off budget registers aren't checked yet. |
| C2 | A $400 payment entered in the wizard as $320 + $80 is an $80 expense and a $320 paydown, and a valid categorized split in Actual. | `crates/veritas-reports/tests/net_profit.rs::plan10_a_400_loan_payment_is_80_expense_and_320_paydown`. `crates/veritas-ffi/tests/money_journeys.rs::a_recorded_payment_is_the_split_actual_reads_and_prefills_the_next_one`. The A4 journey: paydown 32000 in Net Profit, and a split Actual reads. UI test `testPayingTheLoanSplitsPrincipalAndInterest` on stub data. | **proven** |
| C3 | A $50 swipe on the on-budget card is $50 groceries in that month; the card payment never spends. | `net_profit.rs::plan10_a_50_card_swipe_is_groceries_in_the_swipe_month_and_the_payment_never_spends`. `classification.rs::cls8_master_february_card_payment_is_an_internal_transfer_on_both_legs`. On the Budget screen, the A2 journey checks the swipe counts in Groceries that month and the card payment spends nothing, and Actual agrees. | **proven** |
| C4 | The month's Sankey sums to its income, and every outflow drills to its transactions. | `crates/veritas-reports/tests/standard_reports.rs::the_sankey_conserves_money_in_every_month_the_year_and_the_whole_history`. `properties.rs::sankey_invariants_hold_on_every_range`. `VeritasKitTests/RealCoreReportTests.swift::everySankeyNodeYouCanClickOpensTheRowsItWasDrawnFrom`. | **proven.** In a month that spends more than it earns, ST-8 adds a Shortfall source, so outflows sum to income plus shortfall. |
| C5 | Ryan's custom reports and dashboard show Actual's numbers. | The shadow run on Ryan's export, 2026-11-20: widgets 14 of 14 matched, custom reports 1 of 1 matched. Each native card goes through Actual's own card code (the comparison `crates/veritas-ffi/tests/widget_journeys.rs` uses, shared in `tests/support/cards.rs`), and each saved report's rows for its range and filters go through Actual's query engine (`report-rows`). Synthetic equivalents: `crates/veritas-reports/tests/custom_reports.rs`, `widgets.rs` (`d73_*`), `widget_journeys.rs`. | **proven.** A saved report's rows are compared, not its drawn chart. |
| C6 | The Balance Sheet balances, and opening equity + net income ± adjustments = closing equity, on every fixture. | `crates/veritas-reports/tests/fixture_statements.rs` (small, all-features, perf-100k, every month). `properties.rs::statements_always_balance_and_reconcile`. | **proven** |
| C7 | Tax CSV totals per bucket equal the sum of the year's tagged transactions. | `properties.rs::tax_totals_equal_their_detail_rows`. `tax.rs::tax5_summary_and_detail_csv`. `VeritasKitTests/RealCoreTaxTests.swift`. | **proven** |

## Bills, forecast and insights

| # | Criterion | Evidence | Status |
|---|---|---|---|
| D1 | An auto-post bill posts on its date; a confirm-first bill waits in Upcoming and Paid posts an edited amount; both round-trip with the right `posts_transaction`. | `crates/veritas-ffi/tests/bills_rules_journeys.rs`: `a_month_of_bills_posts_the_same_rows_whether_veritas_actual_or_both_open_the_budget`, `paid_with_an_edited_amount_and_date_writes_what_actual_writes_when_you_post_and_edit`, `paying_an_auto_post_bill_by_hand_never_lets_either_app_post_it_again`. The A4 journey checks `posts_transaction` 0 on a confirm-first bill in Actual. | **proven** |
| D2 | Ryan never sees the word "rule" while creating or editing a bill. | None. | **fix in progress.** #87 changes the bill editor's badge to "Has settings only Actual shows". The Paid and Post messages ("N rules couldn't run here") and "This bill's rule can't be read" are core strings in `crates/veritas-rules` (posting.rs, bill.rs). No test reads the bill flow's words yet. |
| D3 | Forecasts at 3, 6, 10 and 12 months render with confidence bands; a scenario changes only `veritas_scenario*` tables. | Core: `crates/veritas-forecast/tests/projection.rs` (`r_fc_1_horizons_at_3_6_10_and_12_months`, `r_fc_1_band_is_net_worth_plus_scaled_offsets`), `crates/veritas-ffi/src/forecast.rs::r_fc_2_what_ifs_write_only_scenario_tables_one_undo_step_each`. Screens, on #84 (93583bd): the Forecast page with its horizon picker, band and compare, and the What-ifs page. `VeritasKitTests/RealCoreForecastTests` checks that the model shows the core's forecast unchanged and that a what-if changes only `veritas_scenario*`. UI tests `ForecastJourneyTests` (3 of 3, `notes/ui/ui12.log`). | **proven** once #84 lands. |
| D4 | Wrapped renders for any full year and exports a redacted image with no amounts. | Core: `crates/veritas-forecast/tests/wrapped.rs` (including `fc_8_redaction_covers_every_card_as_a_share`), `crates/veritas-ffi/tests/insights_journeys.rs`. Screen and image, on #85 (3ddcdbf): `VeritasKitTests/RealCoreWrappedTests.swift::eachYearInTheDataRendersAndHidesItsAmounts` renders every year in all-features from the real core; `WrappedStoryTests.redactedCardsCarryNoMoneyAmount` checks the cards and the text read back from the rendered redacted image; UI tests `WrappedJourneyTests`. `WrappedRedactionWorstCaseTests.aBudgetThatHidesDecimalsShowsNoAmountOnARedactedCard` (saved in `veritas.wt/notes/pr85-wrapped-redaction-worst-case-test.patch`) covers the hardest case to spot: a budget that hides decimals and names no currency, where 1,480.00 reads "1,480". | **proven** once #85 lands. |
| D5 | With AI off nothing AI-related loads; with it on, no network calls. | On #81 (3bdf12d): the model is asked about only once AI is turned on. `AssistantSettingsTests.withTheSettingOffTheModelIsNeverAsked` counts zero asks for a launch with AI off and for opening Settings, makes no model session, and counts one ask when AI is turned on. `AssistantReadOnlyTests`, and the read-only journey `AssistantLeavesTheBudgetAloneTests.everyToolAskedAnythingLeavesTheBudgetAsItWas` (saved in `veritas.wt/notes/pr81-assistant-read-only-test.patch`), which asks every tool for ordinary, odd and impossible things on the real core and then checks that every table, the undo history and the alerts are unchanged; a planted write fails it. The built Veritas.app is sandboxed with no `network.client` entitlement, so macOS refuses any outgoing connection, and the app's sources make none. | **proven.** The app still links FoundationModels, so dyld maps that system framework at launch. The Architect's reading of §10: a mapped system framework with no calls, model or session is not "AI-related loading". No test watches for network traffic while the model answers; the guarantee is the sandbox entitlement, read off the built app. |

## Performance

Measured on an M3 Pro at `b12a8c2`, release build, perf-100k (100k transactions), 2026-11-20. The fuzz harnesses shared the machine for part of the run. Log: `veritas.wt/notes/qa/perf-100k.log`.

| ID | Budget | Measured | Status |
|---|---|---|---|
| R-PERF-1 | Cold launch to an interactive budget ≤ 400 ms | GitHub runner: open until editable, 371 ms (`open_import_export` bench). M3 Pro, for reference: 156 ms. | **partial.** The core passes on the runner, but only 7% under the budget. The app's share of launch isn't measured; Ryan to time a cold launch, or add an `XCTApplicationLaunchMetric` test. |
| R-PERF-2 | Cell edit to updated screen ≤ 16 ms | GitHub runner: a budget cell edit until the month reads back 1.2 ms, the engine's recompute at most 1.0 ms, an envelope set to its 3-month average 10.4 ms. M3 Pro, for reference: 2.4 ms and 6.7 ms. | **partial.** The core passes, but the redraw isn't measured. Setting every envelope to its average is a bulk action, not a cell edit, so no budget covers it. |
| R-PERF-3 | Register scrolls at 120 Hz with no hitches | GitHub runner: the next page while scrolling (47,479 rows) takes 0.22 ms. M3 Pro, for reference: 0.10 ms, with the first page after a commit at 18.6 ms and the first page of a search at 63.7 ms. | **partial.** The core fetch fits in a frame. Scrolling smoothness in the table is unmeasured; Ryan to check with Instruments. |
| R-PERF-4 | Typical report ≤ 100 ms; year-long Sankey or forecast ≤ 500 ms | GitHub runner: the default dashboard 31–46 ms, each card's marginal cost at most 36 ms, the every-card page 131–175 ms. `reports_perf` hasn't run there yet (a failing test stopped it), so the reports' runner numbers come with the next perf run. M3 Pro, for reference, right after an edit: net profit for 12 months 85 ms, net worth 62, cash flow 69, balance sheet 75, owner's equity 67, tax summary 67, a year's Sankey 100, the default dashboard 22, the every-card page 73. | **proven** for the dashboard on the runner, under the Architect's reading of PLAN line 245 (each report, card and the default dashboard ≤ 100 ms; the every-card page ≤ 500 ms). The reports wait for the next perf run. |
| R-PERF-5 | Import or export of a 100k zip ≤ 3 s | GitHub runner (macos-26-arm64): import 1.75 s, export 3.15 s. M3 Pro, for reference: 0.71 s and 1.34 s. | **gap on the CI runner.** Export is 5% over the budget; it needs a core fix. |

**The CI gate.** CI's `perf` job runs `cargo xtask perf` on an Apple Silicon runner, in a release build. Every check runs, and then every budget that was exceeded is listed together.
- **R-PERF-4:** the job runs the release R-PERF-4 tests (`reports_perf.rs`, `dashboard_perf.rs`), which time the app's own path right after an edit makes the session classify again. They follow PLAN line 245 ("Typical report ≤ 100 ms; year-long Sankey or forecast ≤ 500 ms"), as the Architect read it on 2026-11-21:
  - each report, each card's marginal cost, and the default dashboard: ≤ 100 ms;
  - the page with every native card, year-long Sankey and forecast cards among them: ≤ 500 ms.
- **R-PERF-1, 2, 3 and 5:** the criterion benches run, and the gate reads each median from criterion's `estimates.json`. A bench takes 10 to 100 samples after a warm-up, so one slow sample on a shared runner doesn't move the median.
- **What's gated:**
  - open until editable ≤ 400 ms;
  - a budget cell edit, a recompute and an envelope set to its average ≤ 16 ms;
  - a register page while scrolling ≤ 8.33 ms (one 120 Hz frame);
  - import and export ≤ 3 s.

Reports measured with nothing cached are printed against 100 ms and 500 ms, but they don't gate.

| Measured | M3 Pro, local | GitHub runner (macos-26-arm64), 2026-11-21 |
|---|---|---|
| Open until editable (≤ 400 ms) | 156 ms | 371 ms |
| Cell edit / envelope average (≤ 16 ms) | 2.4 / 6.7 ms | 1.2 / 10.4 ms |
| Scrolling page (≤ 8.33 ms) | 0.10 ms | 0.22 ms |
| Import / export (≤ 3 s) | 0.71 / 1.34 s | 1.75 / **3.15 s, over** |
| Default dashboard, warm (≤ 100 ms) | 22 ms | 31–46 ms |
| Every-card page (≤ 500 ms) | 73 ms | 131–175 ms |
| Reports with nothing cached (shown, not gated) | 68–74 ms | 145–174 ms |

On the runner, export misses R-PERF-5 by 5%, and open until editable is 7% under its budget. A miss gets a core fix, not a new reading.

## Full-size runs

| What | Command | Last run |
|---|---|---|
| 1,000 SIGKILLs | `VERITAS_CRASH_KILLS=1000 cargo test -p veritas-session --test crash -- --test-threads=1` | `28ee97e`, 2026-11-20: passed, 576 s |
| 10k random sessions | `VERITAS_ORACLE_SESSIONS=10000 VERITAS_REQUIRE_ORACLE=1 cargo test -p veritas-engine --test oracle_sessions -- --test-threads=1` | `28ee97e`, 2026-11-20: passed, 778 s |
| Perf on perf-100k | `cargo xtask perf` (CI's `perf` job) | `b12a8c2`, 2026-11-20 |

## Fuzzing

Every parser that reads what an Actual file carries, fed hostile input from seeded generators that start from real fixture shapes. Each loop has a fixed cap, and each case runs under a watchdog.

| Harness | Cases | Checks |
|---|---|---|
| `crates/veritas-rules/tests/hostile_rules.rs` | 3,000 each for stored conditions, actions and recurrences; 1,000 hostile rule books; fixed edge tables | Each input parses or refuses with a gap. Parsed rules run on transactions, and recurrences give dates inside their window, increasing and within their count. A rule book built from hostile rows runs and plans bills. |
| `crates/veritas-reports/tests/hostile_settings.rs` | 2,000 JSON texts, 600 widget settings, 2,000 formulas, 600 custom report rows | JSON that parses round-trips. Every card builder answers or refuses. Formulas give a value, an error or a refusal. Report rows load and evaluate, or are refused. |
| `crates/veritas-reports/tests/hostile_import.rs` | 708 edited copies of small.zip (15 hostile SQL values across every column the loader reads, 17 hostile JSON texts in 9 JSON columns, 19 whole-file cases), 6 hostile metadata files | Each copy imports to a typed error, or to a budget that opens, audits, loads, runs its rules, schedules and every card and report. Nothing panics, hangs or returns Internal. A refused import leaves nothing behind, and a row the loader can't read is reported, never read as zero. |

CI runs all three.

## Bugs found

Each was first written as the smallest failing test. All six are fixed on this branch, and the tests now guard them.

| Test | What it found | Fixed in |
|---|---|---|
| `plan_journeys::moving_a_categorys_history_snapshots_the_budget_first` | Moving a category's history re-files every transaction in it with no snapshot first (R-REL-3). | 39e4d3e |
| `plan_journeys::applying_next_months_suggestions_snapshots_the_budget_first` | Applying suggestions rewrites a month of budget cells with no snapshot first (R-REL-3). | 39e4d3e |
| `hostile_settings.rs::settings_nested_past_the_stack_are_refused_without_crashing_the_app` | `Json::parse` (`crates/veritas-reports/src/json.rs`) recurses with no depth limit. A dashboard or custom report setting nested 100,000 deep overflows the stack and aborts the app, which the FFI guard can't catch (R-REL-7). | d894fb4 |
| `hostile_rules.rs::a_daily_bill_counted_from_long_ago_lists_five_years_of_dates_without_stalling` | A bill that ends after N occurrences re-walks its count from the start for every date it lists. A daily one started long ago takes minutes for the five years the bill detail asks for, and hits the 20 s watchdog in a debug build. | 7b246a6 |
| `hostile_import.rs::the_calendar_card_refuses_a_day_spent_at_the_smallest_storable_amount` | The calendar card negates a day's total. At i64::MIN that panics in debug and wraps to a wrong figure in release. Actual never writes past 2^51, so only a damaged file reaches it. | ca2a23f |
| `hostile_import.rs::the_age_of_money_card_refuses_a_day_spent_at_the_smallest_storable_amount` | The same overflow in the age-of-money card. | ca2a23f |
