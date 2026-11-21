# Cutover: from Actual to Veritas

This is the checklist for moving day-to-day budgeting from Actual to Veritas (PLAN §8, Phase 3). Gate 3 is Ryan's decision at the end of it.

## Before you start

- [ ] Every P0 criterion in PLAN §10 is proven in [acceptance.md](acceptance.md).
- [ ] The shadow run on your real export passed: `cargo xtask shadow <export.zip>` printed `all checks pass`. First run 2026-11-20: round trip identical, 37 months and 10 accounts with 0 differences, 14 of 14 dashboard cards and 1 of 1 custom report matching Actual.

## 1. Set up backups first

1. Open Veritas, then Settings → Backups.
2. Choose your OneDrive folder for the scheduled Actual `.zip` backup (R-REL-4).
3. Leave snapshot retention at the default (48 hourly, 30 daily, 12 monthly) unless you want more (R-REL-3).
4. Check that the first backup `.zip` appears in the folder.

## 2. Bring your budget over

1. Finish any edits in Actual and quit it, so nothing changes after you export.
2. In Actual, export the budget (Settings → Export).
3. In Veritas, choose **Import from Actual** and pick that `.zip`.
4. Run `cargo xtask shadow <that .zip>` again. It should print `all checks pass`.

From here on, **Veritas is the source of truth.** Don't enter anything in Actual, or the two will drift.

## 3. Two weeks side by side

For two weeks, budget only in Veritas: every transaction, budget move, bill and goal. Once a week:

1. In Veritas, choose **Export to Actual…**.
2. Import that `.zip` into the Actual desktop app.
3. Spot-check in Actual against Veritas:
   - [ ] account balances;
   - [ ] this month's and last month's budget (budgeted, spent, available, to budget);
   - [ ] upcoming bills (schedules) and whether each auto-posts;
   - [ ] your custom reports and dashboard.

Write down anything that differs, and report it before the next week starts.

## 4. Know what v1 doesn't do

| Area | In v1 | Where it's decided |
|---|---|---|
| Bank sync | None. Its columns are kept untouched. | D14 |
| Live sync with an Actual server | None. Interop is file import and export. | D5 |
| Actual templates and automations | Kept as they are, not run | D15 |
| Tracking ("reflect") budgets | Open read-only | R-BUD-2 |
| Rules Veritas can't run | Shown read-only, with "Edit in Actual" | R-RULE-3 |
| A file from a newer Actual | Opens read-only, with the reason | R-IO-7 |
| Weekday bills counted from long ago (for example, every second Friday since 1990) | Correct, but slow to list | Accepted v1 limit |

## 5. Gate 3: the daily-drive decision

After two weeks, decide whether Veritas becomes your daily budget. Confirm:

- [ ] No unexplained difference turned up in the weekly spot-checks.
- [ ] Backups kept arriving in the folder, and you restored one snapshot (Settings → Snapshots → Restore…) to check it.
- [ ] Nothing you rely on falls under section 4.

## Going back

You can return to Actual at any time:

1. In Veritas, choose **Export to Actual…**.
2. Import the `.zip` into Actual.

The round trip is lossless for Actual's data (§10, proven in [acceptance.md](acceptance.md)), and the Veritas-only extras ride along in the file. Snapshots also let you rewind Veritas itself to any point in the retention window.
