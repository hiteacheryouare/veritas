# Classification (R-REP-1)

Status: draft for Gate 0. Owner: Fred. Every report, screen and export cites this file for what a transaction *means*.

Conventions used in all finance specs:

- Amounts are integers in minor units (cents). Examples write `-5000` for -$50.00.
- Sign is Actual's: positive = money into the account, negative = money out of it (`transactions.amount`).
- A "period" is a set of civil dates. A row belongs to the period containing its `date`. No time zones.
- Report magnitudes are positive numbers defined by the formulas in CLS-6. Raw row amounts keep Actual's sign.

## CLS-1 Definitions

| Term | Meaning |
|---|---|
| Row | One non-tombstoned `transactions` row that is not a split parent. A split parent (`isParent = 1`) is never classified; its children are. Children sum to the parent, so nothing is lost or doubled. |
| Transfer row | A row whose payee has `transfer_acct` set, paired by `transferred_id` to a counterpart row in the other account. |
| Pair | The two rows of one transfer. |
| On-budget / off-budget | The account's current `accounts.offbudget` flag, applied to all of history. Actual does the same (`reference/actual/packages/loot-core/src/server/budget/base.ts:81,142`). |
| Primary leg | For an on-budget <-> off-budget pair, the on-budget row. It carries the category and the kind. |
| Mirror leg | The other row of an on<->off pair. It gets the same kind as the primary leg, is marked `leg = mirror`, and is skipped by every flow total. It is still counted in balances and net worth. |
| Kind | Exactly one of the seven values in CLS-2. Decided once, in the classification function, never re-derived by a report. |
| Flags | Zero or more of `needs_review`, `misfiled`, `kind_conflict`. Flags never change a kind, they only surface a problem. |

## CLS-2 Transaction kinds

| Kind | Meaning | Counts in Net Profit? | Changes net worth? |
|---|---|---|---|
| `income` | Money earned (pay, interest, gifts received into an income category) | Yes, adds | Yes |
| `expense` | Money spent, including interest, fees, escrow, refunds (negative expense) | Yes, subtracts | Yes |
| `internal_transfer` | Money moved between your own accounts with no financial meaning (card payment, checking to checking) | No | No (pair nets to 0) |
| `debt_principal` | Principal portion of a loan payment | No (own line: Debt paydown) | No (cash down, liability down) |
| `savings_contribution` | Money moved to a savings or investment account | No (own line: Savings) | No |
| `starting_balance` | An account's opening balance | No | Yes (own reconciling line) |
| `adjustment` | A non-cash or unexplained balance change: reconciliation adjustment, off-budget revaluation | No | Yes (own reconciling lines) |

## CLS-3 Category kinds

A category has a kind. It is stored in `veritas_category_meta.kind`. Kinds describe categories, kinds in CLS-2 describe rows.

| Category kind | Used for | Default when no Veritas metadata exists |
|---|---|---|
| `income` | Actual income categories | `categories.is_income = 1` |
| `expense` | Everything you budget for, including goal categories | Every other category |
| `debt_principal` | The envelope that holds the principal part of loan payments | Never defaulted. Set by the loan wizard or by the user. |
| `savings` | The envelope that holds money moved to savings or investing | Never defaulted. Set by the user or the R-XFER-4 prompt. |

Rules:

- **CLS-3.1** Actual wins on the income/expense axis. If `is_income = 1`, the effective kind is `income` whatever the metadata says. If `is_income = 0` and metadata says `income`, the effective kind is `expense`. Both cases set `kind_conflict` (the envelope math in Actual does not change).
- **CLS-3.2** Hidden or archived categories keep their kind. Hiding never reclassifies history (R-REP-3).
- **CLS-3.3** A goal category is an `expense` category. Spending on a goal is spending. Contributions to a goal are budget moves, not transactions, so they never appear here.
- **CLS-3.4** No guessing from names. Existing Actual data with no metadata gets the defaults above and nothing else.

## CLS-4 Decision table

Evaluate top to bottom. The first matching row decides the kind. Inputs: split parent, starting-balance flag, reconciliation marker, transfer or not, the account(s) on/off budget, the category's effective kind.

| # | Condition | Kind | Leg | Flags |
|---|---|---|---|---|
| 1 | Split parent | not classified | - | - |
| 2 | `starting_balance_flag = 1` | `starting_balance` | primary | - |
| 3 | Reconciliation marker (CLS-5), any account, any category | `adjustment` (source `reconciliation`) | primary | - |
| 4 | Transfer, both accounts on-budget | `internal_transfer` | both primary | `needs_review` if a category is present |
| 5 | Transfer, both accounts off-budget | `internal_transfer` | both primary | `needs_review` if a category is present |
| 6 | Transfer on<->off, category kind `debt_principal` | `debt_principal` | on-budget row primary, other mirror | - |
| 7 | Transfer on<->off, category kind `savings` | `savings_contribution` | on-budget row primary, other mirror | - |
| 8 | Transfer on<->off, category kind `expense` | `expense` | on-budget row primary, other mirror | - |
| 9 | Transfer on<->off, category kind `income` | `income` | on-budget row primary, other mirror | - |
| 10 | Transfer on<->off, no category | `internal_transfer` | on-budget row primary, other mirror | `needs_review` |
| 11 | Not a transfer, account off-budget | `adjustment` (source `offbudget_revaluation`) | primary | - |
| 12 | Not a transfer, on-budget, category kind `income` | `income` | primary | - |
| 13 | Not a transfer, on-budget, category kind `expense` | `expense` | primary | - |
| 14 | Not a transfer, on-budget, category kind `debt_principal` or `savings` | `expense` | primary | `misfiled` |
| 15 | Not a transfer, on-budget, no category | `expense` | primary | `needs_review` |

Why the unusual rows:

- **Row 8 and 9 (capitalized transfers).** Moving money on->off with an ordinary expense category (say, a $500 car purchase into an off-budget Car asset) is `expense`, the same as Actual's envelope treats it. The asset shows up on the balance sheet, so statements add one named reconciling line (ST-4, "capitalized transfers"). It is explicit, never a plug.
- **Row 14.** A principal or savings category only makes sense when money actually lands in another account. Without a transfer partner the balance sheet would drop with no matching asset or liability. We book it as `expense` (conservative) and flag it. Never silently treat it as paydown.
- **Row 15.** An uncategorized row is `expense` regardless of sign. A positive amount reduces expense. We never book unknown money as income. Net Profit is identical either way. Only the gross income and expense totals differ.
- **Row 10.** An uncategorized on<->off transfer stays out of Net Profit and flows to its own line in the cash flow statement (ST-6) and the Sankey (ST-8, "Unreviewed transfers"). It keeps every identity closed until the user decides what it was.
- **Row 11.** Off-budget accounts have no categories in Actual. A non-transfer row there (a house revalued, a brokerage balance updated) is a non-cash change, not income. Interest earned inside an off-budget account is therefore an `adjustment` unless the user records it in an on-budget account.
- **Row 4.** Actual clears the category on same-side transfers (`reference/actual/packages/loot-core/src/server/transactions/transfer.ts:25-45`), so a present category on rows 4 and 5 means odd imported data.

### CLS-4.1 Kind assignment for on<->off prompts (R-XFER-4)

| User answer | Category used | Resulting kind |
|---|---|---|
| Loan/debt payment | the one `debt_principal` category (created in a "Debt" group if none exists; if several, ask) | `debt_principal` |
| Saving/investing | a `savings` category (same rule) | `savings_contribution` |
| Other | a category the user picks | kind of that category (row 8 or 9) |

VERIFY (P0.4): `transfer.ts:25-45` shows Actual keeps the category on mixed pairs and clears it on same-side pairs. I found no server-side check that a category is *required*. If it is only a UI nudge, existing data can contain row 10.

### CLS-4.2 Existing Actual data with no metadata

On import, every on<->off transfer whose category has no Veritas kind lands on row 8 (`expense`), because that is what Actual's own reports do. Loan payments and savings moves therefore overstate expenses until the user answers once per category. Veritas must show a review count ("12 transfers to off-budget accounts are counted as spending") and a one-time wizard that assigns kinds to categories, never to individual rows. Net Profit stays reproducible and equal to Actual-style numbers until the user acts. Decision D-1 in the hand-off report covers whether to change this default.

## CLS-5 Reconciliation marker

A row is a reconciliation adjustment if either:

1. Veritas created it through its reconcile flow. Recorded in `veritas_transaction_meta.adjustment = 1`. This is authoritative.
2. Actual created it: row is cleared, has no payee, no category, and `notes` equals exactly `Reconciliation balance adjustment` (`reference/actual/packages/desktop-client/src/accounts/reconciliation.ts:108`). Best effort only. The text is localized in Actual and rules may rewrite it. A miss falls to row 15 (`expense`, `needs_review`) and the user can mark it as an adjustment.

## CLS-6 Report magnitudes

For a period, summing over rows with `leg != mirror`:

| Name | Formula |
|---|---|
| `income_total` | sum of `amount` for kind `income` |
| `expense_total` | minus the sum of `amount` for kind `expense` (refunds reduce it; it can be negative) |
| `net_profit` | `income_total - expense_total` |
| `debt_paydown` | minus the sum of `amount` for kind `debt_principal` |
| `savings_total` | minus the sum of `amount` for kind `savings_contribution` |
| `net_cash_flow` | `net_profit - debt_paydown - savings_total` (R-REP-2) |

`income_total` is net of reversals: a negative amount in an income category reduces it. Withdrawals from savings (positive amount on kind `savings_contribution`) make `savings_total` negative and raise net cash flow. Statement and Sankey rules build on these definitions (statements.md).

## CLS-7 Where it lives

One function in `veritas-reports`: `classify(row, context) -> { kind, leg, source?, flags }`. `context` carries account budget flags, category effective kinds, the pair lookup and the reconciliation markers. Reports, the Sankey, statements, tax buckets, forecasts and Wrapped call it and never read `is_income` or `offbudget` themselves. A report that decides for itself what counts as spending is a blocker in review.

## CLS-8 Worked examples

Master scenario (used again in statements.md). Accounts: CHK Checking (on), VISA Visa (on, liability), SAV Savings (off), LOAN Car Loan (off, liability), BRK Brokerage (off), CAR Car asset (off), CD Emergency CD (off, opened Jan 15). Opening balances at 2026-12-31: CHK 200000, VISA 0, SAV 500000, LOAN -980000, BRK 1000000, CAR 0, CD does not exist yet.

January 2027 rows (loan payment stored as in debt.md DEBT-6):

| Id | Date | Account | Amount | Detail | Row | Kind | Leg |
|---|---|---|---|---|---|---|---|
| T1 | 01-01 | CHK | 300000 | Paycheck, category Paycheck (income) | 12 | `income` | primary |
| T2 | 01-03 | VISA | -5000 | Groceries swipe | 13 | `expense` | primary |
| T3 | 01-05 | VISA | -4000 | Clothing swipe | 13 | `expense` | primary |
| T4 | 01-09 | VISA | 1500 | Clothing refund, category Clothing | 13 | `expense` | primary |
| T5 | 01-10 | CHK | -40000 | Loan payment, split parent | 1 | not classified | - |
| T5a | 01-10 | CHK | -32000 | Child, transfer to LOAN, category Loan principal | 6 | `debt_principal` | primary |
| T5b | 01-10 | CHK | -8000 | Child, category Loan interest | 13 | `expense` | primary |
| T5m | 01-10 | LOAN | 32000 | Counterpart of T5a | 6 | `debt_principal` | mirror |
| T6 | 01-12 | CHK | -20000 | Transfer to SAV, category Savings | 7 | `savings_contribution` | primary |
| T6m | 01-12 | SAV | 20000 | Counterpart of T6 | 7 | `savings_contribution` | mirror |
| T7 | 01-15 | CD | 100000 | Starting balance flag | 2 | `starting_balance` | primary |
| T8 | 01-20 | CHK | -50000 | Transfer to CAR, category Car purchase (expense) | 8 | `expense` | primary |
| T8m | 01-20 | CAR | 50000 | Counterpart of T8 | 8 | `expense` | mirror |
| T9 | 01-25 | BRK | 15000 | Balance updated, no category | 11 | `adjustment` | primary |
| T10 | 01-31 | CHK | -1200 | Reconciliation adjustment | 3 | `adjustment` | primary |

Expected January magnitudes (CLS-6): `income_total` 300000. `expense_total` = 5000 + (4000 - 1500) + 8000 + 50000 = **65500**. `net_profit` = **234500**. `debt_paydown` 32000. `savings_total` 20000. `net_cash_flow` = 234500 - 32000 - 20000 = **182500**.

February 2027 rows:

| Id | Date | Account | Amount | Detail | Row | Kind |
|---|---|---|---|---|---|---|
| T11 | 02-01 | CHK | 300000 | Paycheck | 12 | `income` |
| T12 | 02-01 | VISA | -2500 | Groceries swipe | 13 | `expense` |
| T13 | 02-03 | CHK | -10000 | Pay Visa (covers Jan statement 7500 + Feb swipe 2500) | 4 | `internal_transfer` |
| T13m | 02-03 | VISA | 10000 | Counterpart | 4 | `internal_transfer` |

Expected February: `expense_total` **2500** (the swipe counts once, in the month swiped, the card payment never counts), `net_profit` 297500, `net_cash_flow` 297500.

Nasty cases, each a test:

| Case | Rows | Expected |
|---|---|---|
| C-1 Refund crosses a period boundary | 2027-03-28 VISA -10000 Clothing. 2027-04-05 VISA +10000 Clothing refund. No other April spending. | March `expense_total` 10000. April `expense_total` **-10000** (negative expense). April `income_total` 0 (refund is never income). |
| C-2 Refund larger than the month's spending | April as above plus 2027-04-10 VISA -2000 Clothing | April `expense_total` -8000 |
| C-3 Starting balance never report income (intentional divergence from the budget sheet) | New on-budget account, balance 50000. Actual files it under an income category named "Starting Balances", or else the first income category (`reference/actual/packages/loot-core/src/server/accounts/payees.ts:19-29`, `app.ts:580-590`) | Row 2 wins: `starting_balance`, `income_total` unchanged. Actual's envelope sheet still counts a categorized starting balance as budget income, so To Budget rises by 50000 in the budget screen while report income does not. Reports differ from the budget sheet on purpose (PLAN edge case 16). Veritas does not change the budget math (R-BUD-1) |
| C-4 Principal on a non-transfer row | CHK -32000, category Loan principal, no transfer | Row 14: `expense`, flag `misfiled` |
| C-5 Uncategorized positive | CHK +4000, no category, no transfer | Row 15: `expense` (-4000 contribution to `expense_total`), flag `needs_review` |
| C-6 On->off transfer, no category | CHK -10000 to BRK, no category | Row 10: `internal_transfer`, `needs_review`. Does not touch `net_profit`. Appears in cash flow line ST-6 |
| C-7 Unmarked category on an on->off transfer | Existing Actual file, CHK -32000 to LOAN, category "Car Loan" with no metadata | Row 8: `expense` until the user assigns kind `debt_principal` (CLS-4.2) |
| C-8 Same-side transfer | CHK to Visa | Row 4 on both rows. No flow totals change |
| C-9 Hidden category with activity | Category "Old Gym" hidden, 2027-01-04 CHK -3000 | Row 13: `expense`, included, labeled archived (R-REP-3) |
| C-10 Kind conflict | `is_income = 0`, metadata kind `income`, row +5000 | effective kind `expense`, `kind_conflict`, row is row 13 |
| C-11 Split with mixed kinds | Parent CHK -10000: child -7000 Groceries, child -3000 Dining | Two `expense` rows. Parent not classified. Sum of kinds' amounts = -10000 |

## CLS-9 Reports vs the budget sheet (intentional divergence)

Actual's envelope sheet counts categorized starting balances as income and counts hidden categories, spent split children in on-budget accounts only, and feeds To Budget from the first income group only (`reference/actual/` budget math notes, P0.4 C-budget-math). Veritas reports follow this file, not the sheet. Where they differ, the budget screen keeps Actual's number and reports keep ours. Example: a 50000 starting balance in a new on-budget account raises To Budget by 50000 and leaves `income_total` at 0. Hidden categories count fully in both.
