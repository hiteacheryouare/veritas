# Tax buckets (R-REP-8, D13)

Status: draft for Gate 0. Owner: Fred. Builds on classification.md (CLS-n).

Scope (D13): a summary export that helps hand clean numbers to tax software or an accountant. Veritas does not map to form lines, does not estimate liability, and does not say that anything is deductible or taxable. This file contains no dollar limits, rates or thresholds on purpose. If one is ever added, it must name its tax year and cite a primary source (IRS or equivalent) that was actually read.

## TAX-1 Definitions

| Term | Meaning |
|---|---|
| Bucket | A fixed label for a group of amounts the user wants totaled (TAX-2). |
| Tax year | The calendar year of the row's `date` (civil date). Year-end is Dec 31, not a fiscal year. |
| Category bucket | `veritas_category_meta.tax_bucket`, set on a category. Applies to every row in it. |
| Transaction tag | A tag in the row's own `notes` (`#medical`, whitespace-delimited, case-sensitive: `reference/actual/packages/loot-core/src/shared/tags.ts:1-25`). |
| Tag mapping | `veritas_tax_tag(tag, bucket)`: which tags mean which bucket. A tag with no mapping is ignored. |
| Counted amount | A row's signed contribution to a bucket total (TAX-4). |

## TAX-2 Bucket list

| Bucket id | Label | Direction |
|---|---|---|
| `w2_income` | W-2 income | inflow |
| `interest_income` | Interest income | inflow |
| `business_income` | Business income | inflow |
| `business_expense` | Business expenses | outflow |
| `charitable` | Charitable giving | outflow |
| `medical` | Medical | outflow |
| `education` | Education | outflow |
| `hsa` | HSA contributions | outflow |
| `none` | No tax bucket (override only) | - |

"Business" is split into income and expense so each total has one natural direction. Labels say what the user tagged, never what is deductible. `none` is an override: a tag mapped to `none` removes a row from its category's bucket.

## TAX-3 Which rows count, and precedence

1. Only rows with `leg != mirror` and kind `income`, `expense`, `debt_principal` or `savings_contribution` are eligible. Rows of kind `internal_transfer`, `starting_balance` and `adjustment` are never counted, even if tagged. (An HSA contribution is a transfer to an off-budget account with a `savings` category, so it is eligible through its on-budget leg. Its mirror leg is skipped, so nothing is double counted.)
2. A split parent is never counted. Each child is evaluated on its own category and its own notes. Tags on the parent's notes do not flow to the children.
3. **Precedence for one row:**
   1. Transaction tags: collect the mapped buckets of every tag in the row's notes. If exactly one distinct bucket, that is the row's bucket (`source = tag`). `none` wins as a bucket meaning "excluded".
   2. Otherwise, if two or more distinct buckets, the row is a **conflict**: it is excluded from totals and listed under "Needs a decision" in the PDF and as rows in the detail CSV with `bucket = conflict`.
   3. Otherwise the category's bucket (`source = category`).
   4. Otherwise not counted.
4. The same bucket reached through two tags counts once.
5. Tag over category, always: a pharmacy purchase in Groceries tagged `#medical` counts as medical, and the category has no bearing.

## TAX-4 Sign convention

`amount` is Actual's signed value. `counted_cents`:

| Bucket direction | counted_cents |
|---|---|
| inflow | `amount` |
| outflow | `-amount` |

Positive counted means "adds to this bucket". A refund in an outflow bucket is negative counted and reduces the total. Bucket total = sum of `counted_cents` for the tax year. A total may be negative. It is shown as negative with a note, never clamped to 0.

## TAX-5 CSV (UTF-8, no BOM, LF line endings, RFC 4180 quoting, header row)

**Summary file** `veritas-tax-summary-<year>.csv`, one row per bucket that has at least one counted row, sorted by bucket id:

| Column | Content |
|---|---|
| `tax_year` | `2027` |
| `bucket` | bucket id |
| `direction` | `inflow` or `outflow` |
| `total_cents` | integer, signed |
| `total` | same value as a decimal string with two places, formatted from the integer (never from a float), `-` prefix if negative |
| `transaction_count` | number of counted rows |

**Detail file** `veritas-tax-detail-<year>.csv`, one row per counted or conflicting row (split children, not parents), sorted by bucket, then date, then id:

| Column | Content |
|---|---|
| `tax_year` | year |
| `bucket` | bucket id, or `conflict` |
| `source` | `tag` or `category` |
| `date` | `YYYY-MM-DD` |
| `account` | account name |
| `payee` | payee name after payee mapping |
| `category` | category name (archived ones shown as-is) |
| `amount_cents` | Actual-signed integer |
| `counted_cents` | per TAX-4 (empty for conflicts) |
| `notes` | the row's notes |
| `transaction_id` | the row's id |
| `parent_id` | split parent id, or empty |

For text columns only, a cell that begins with `=`, `+`, `-` or `@` is written with a leading single quote so spreadsheets do not run it as a formula. Numeric columns are never altered.

PDF copy: the title is "Tax summary", the footer reads "Organized from your own categories and tags. Not tax advice." No line uses "deductible", "taxable" or "owed".

## TAX-6 Invariants

1. For each bucket, `total_cents` equals the sum of `counted_cents` of its detail rows. This is the §10 acceptance test.
2. Detail rows with a non-conflict bucket appear exactly once. A mirror leg never appears.
3. Reclassifying a category's kind (CLS-3) never changes a tax total unless it changes which rows are eligible under TAX-3.1.

## TAX-7 Worked examples (tax year 2027)

| Case | Rows | Expected |
|---|---|---|
| T-1 Category bucket and refund | Category "Donations" bucket `charitable`: -10000, -5000, +2000 (refund) | `charitable` total 13000, count 3 |
| T-2 Tag over category | Groceries (no bucket) row -3000 with notes `#medical`, mapping `medical` | `medical` total 3000, `source = tag` |
| T-3 Tag removes a category's bucket | "Donations" row -4000 with notes `#notax`, mapping `none` | Row not counted. `charitable` unchanged |
| T-4 Conflict | Row -2500, notes `#medical #education` | Not in any total. Detail row `bucket = conflict`, `counted_cents` empty |
| T-5 Same bucket twice | Notes `#medical #rx`, both map to `medical`, amount -1000 | `medical` +1000, counted once |
| T-6 HSA transfer | CHK -20000 transfer to off-budget HSA, `savings` category, notes `#hsa` | `hsa` total 20000. The HSA account's +20000 mirror row is absent from the detail |
| T-7 Interest income | CHK +1234 in income category "Interest", bucket `interest_income` | `interest_income` total 1234 |
| T-8 Year boundary | Donations -5000 on 2027-12-31, -7000 on 2028-01-01 | 2027 `charitable` 5000. 2028 `charitable` 7000 |
| T-9 Split | Parent -10000: child -6000 category "Donations", child -4000 Groceries, parent notes `#medical` | `charitable` 6000. Parent tag ignored. Nothing in `medical` |
| T-10 Negative total | Donations only +2000 (refund) in the year | `charitable` -2000, shown negative |
| T-11 Starting balance and adjustment | Tagged rows of those kinds | Not counted (TAX-3.1) |
| T-12 Business | Income category "Freelance" `business_income` +80000. Category "Supplies" `business_expense` -15000, +1000 refund | `business_income` 80000. `business_expense` 14000. No netting between them |

## TAX-8 Known limits, shown to the user

- Totals are only what was recorded. If pay is recorded as the net deposit, `w2_income` is net pay, not the gross shown on a wage statement. See decision D-4 in the hand-off report.
- Interest earned inside an off-budget account is an `adjustment` (CLS-4 row 11) and is not counted unless it is recorded in an on-budget income category.
- A bucket is a label the user chose. Whether an amount affects a tax return is for the user's tax software or accountant.
