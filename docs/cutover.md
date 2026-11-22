# Cutover: from Actual to Veritas

This is the checklist for moving day-to-day budgeting from Actual to Veritas (PLAN §8, Phase 3). Gate 3 is Ryan's decision at the end of it.

## Before you start

- [ ] Every P0 criterion in PLAN §10 is proven in [acceptance.md](acceptance.md).
- [ ] The shadow run on your real export passed: `cargo xtask shadow <export.zip>` printed `all checks pass`. First run 2026-11-20: round trip identical, 37 months and 10 accounts with 0 differences, 14 of 14 dashboard cards and 1 of 1 custom report matching Actual.

## 1. Bring your budget over

1. Finish any edits in Actual and quit it, so nothing changes after you export.
2. In Actual, export the budget (Settings → Export). If Veritas says the file is from an older Actual, update Actual and export again (R-IO-7).
3. In Veritas, choose **Import from Actual…** and pick that `.zip`.
4. Run `cargo xtask shadow <that .zip>` again. It should print `all checks pass`.

From here on, **Veritas is the source of truth.** Don't enter anything in Actual, or the two will drift.

## 2. Turn on backups

Backups belong to each budget, so set them up with the imported budget open.

1. Open Settings → Backups.
2. Choose your OneDrive folder for the Actual `.zip` backup (R-REL-4).
3. Set the backup schedule to **Daily**. It starts as Off.
4. Click **Back up now** and check that the `.zip` appears in the folder.
5. Leave snapshot retention at the default (48 hourly, 30 daily, 12 monthly) unless you want more (R-REL-3).

## 3. Two weeks side by side

For two weeks, budget only in Veritas: every transaction, budget move, bill and goal. Once a week:

1. In Veritas, choose **Export to Actual…**.
2. In Actual, delete last week's imported copy, then import the new `.zip`.
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
| Bills on a weekday of the month (for example, the second Friday of each month) that end after a set number of payments and started long ago | Correct, but slow to list | v1 limit, docs/acceptance.md |

## 5. Gate 3: the daily-drive decision

After two weeks, decide whether Veritas becomes your daily budget. Confirm:

- [ ] No unexplained difference turned up in the weekly spot-checks.
- [ ] Backups kept arriving in the folder.
- [ ] A snapshot restore works. In Settings → Backups → Snapshots, restore an older snapshot and check it. Then come back to your current budget by restoring the snapshot Veritas took just before that restore.
- [ ] Nothing you rely on falls under section 4.

## Going back

You can return to Actual at any time:

1. In Veritas, choose **Export to Actual…**.
2. Import the `.zip` into Actual.

The round trip is lossless for Actual's data (§10, proven in [acceptance.md](acceptance.md)), and the Veritas-only extras ride along in the file. Snapshots also let you rewind Veritas itself to any snapshot it has kept.
