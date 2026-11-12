# Classification (R-REP-1)

Status: draft for Gate 0. Owner: Fred. Every report, screen and export cites this file for what a transaction *means*.

Conventions used in all finance specs:

- Amounts are integers in minor units (cents). Examples write `-5000` for -$50.00. The minor unit and its decimal places come from the budget's currency (CLS-12). Examples use two places.
- Sign is Actual's: positive = money into the account, negative = money out of it (`transactions.amount`).
- A "period" is a set of civil dates. A row belongs to the period containing its `date`. No time zones.
- Report magnitudes are positive numbers defined by the formulas in CLS-6. Raw row amounts keep Actual's sign.

## CLS-1 Definitions

| Term | Meaning |
|---|---|
| Row | One row of Actual's `v_transactions_internal_alive` with `is_parent = 0`. That view drops tombstoned rows and children whose parent is tombstoned or missing (`docs/actual-format.md` §4, `reference/actual/packages/loot-core/src/server/aql/schema/index.ts:404-412`). A split parent is never classified; its children are. Children sum to the parent, so nothing is lost or doubled. Balances use the same rows. |
| Category of a row | The row's `category` resolved through `category_mapping`, as `v_transactions_internal` does. A merged or deleted category's history therefore classifies under its target's kind, as Actual's budget sums do (`budget/base.ts:138-143`). No resolvable category means uncategorized. A category deleted with no transfer target maps to itself and is tombstoned: the row still counts as categorized, classified by the tombstoned row's kind, and is labeled `(deleted) <name>` (edge case 3). This follows Actual's budget sums, and differs from `v_transactions`, which shows such rows as uncategorized. |
| Transfer row | A row whose payee (resolved through `payee_mapping`) has `transfer_acct` set. The counterpart account is `payees.transfer_acct`, never inferred from the counterpart row. |
| Pair | The two rows of one transfer, linked by `transferred_id`. Two rows are **paired** only if the link is reciprocal: A's `transferred_id` is B, B's `transferred_id` is A, both are live rows (see Row), each is in the other's `transfer_acct` account, and the amounts net to zero (`a + b = 0`). A pair whose amounts do not net to zero is not a transfer: both rows are unpaired. Dates need not match: Actual does not sync transfer dates by default, so legs on different dates are normal and are paired. Each leg belongs to the period of its own date (ST-4 line E7, ST-6). A split parent never counts as a partner. If the link is one-way, **both** rows are unpaired, so E6 (ST-4) adds the two amounts together and nets to zero like a pair does. The audit (R-REL-2) reports every unpaired row. Any other transfer row is **unpaired** (edge case 4): partner missing, link not reciprocal, or amounts not netting to zero. An unpaired row is still classified by the table below using `transfer_acct`, and carries flag `unpaired`. |
| On-budget / off-budget | The account's current `accounts.offbudget` flag, applied to all of history. Actual does the same (`reference/actual/packages/loot-core/src/server/budget/base.ts:81,142`). |
| Orphaned row | A row whose own account has no `accounts` row at all. A tombstoned (deleted) account that still has live rows is not orphaned: its rows count everywhere and the account is labeled `deleted` (history is sacred, R-REP-3), with `needs_review` on the account in the audit. No account holds it, so it is **excluded from every total**: balances, net worth, flow totals and every equity and cash flow line. It is not classified (like a split parent), carries `needs_review`, and is listed by the audit. If only the row's `transfer_acct` account is missing, the row is not orphaned: its own account holds it and balances include it. It is an unpaired transfer, and the missing counterpart account is treated as off-budget for rows 4-10. |
| Hidden and archived | Hidden means the category or its group is hidden. Archived means any `archived_on` value, regardless of the report period. Both are labels on historical reports, never filters. |
| Primary leg | For an on-budget <-> off-budget pair, the on-budget row. It carries the category and the kind. |
| Mirror leg | The other row of an on<->off pair. **A pair is classified once, from the on-budget row's category.** The mirror never has a category (Actual never gives the off-budget side one, `docs/actual-format.md` §4.2), so it is never evaluated against rows 6-10. It copies the primary leg's kind, is marked `leg = mirror`, and is skipped by every flow total. It still counts in balances and net worth. An off-budget row whose on-budget partner is missing, not reciprocal, or whose amount does not net to zero against it (an unpaired row, see Pair) has no primary to copy from: it is classified as `internal_transfer`, `leg = primary`, flags `unpaired` and `needs_review`. |
| Kind | Exactly one of the seven values in CLS-2. Decided once, in the classification function, never re-derived by a report. |
| Flags | Zero or more of `needs_review`, `misfiled`, `kind_conflict`, `unpaired`. Flags never change a kind, they only surface a problem. Flags live on the primary leg. A mirror carries none of its own (a row 10 primary's `needs_review` is not repeated on its mirror). `kind_conflict` is set only when the category's kind decided the row (rows 6-9 and 12-14). |

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
| 2 | `starting_balance_flag = 1`, and the row is not a paired transfer | `starting_balance` | primary | `needs_review` if the row is an unpaired transfer |
| 3 | Reconciliation marker (CLS-5), any account, any category, and the row is not a paired transfer | `adjustment` (source `reconciliation`) | primary | `needs_review` if the row is an unpaired transfer |
| 4 | Transfer, both accounts on-budget | `internal_transfer` | both primary | `needs_review` if a category is present; also `needs_review` on both rows if the starting-balance flag or a reconciliation marker was set |
| 5 | Transfer, both accounts off-budget | `internal_transfer` | both primary | `needs_review` if a category is present; also `needs_review` on both rows if the starting-balance flag or a reconciliation marker was set |
| 6 | Transfer on<->off, category kind `debt_principal` | `debt_principal` | on-budget row primary, other mirror | `needs_review` on the primary leg if the starting-balance flag or a reconciliation marker was set |
| 7 | Transfer on<->off, category kind `savings` | `savings_contribution` | on-budget row primary, other mirror | `needs_review` on the primary leg if the starting-balance flag or a reconciliation marker was set |
| 8 | Transfer on<->off, category kind `expense` | `expense` | on-budget row primary, other mirror | `needs_review` on the primary leg if the starting-balance flag or a reconciliation marker was set |
| 9 | Transfer on<->off, category kind `income` | `income` | on-budget row primary, other mirror | `needs_review` on the primary leg if the starting-balance flag or a reconciliation marker was set |
| 10 | Transfer on<->off, no category | `internal_transfer` | on-budget row primary, other mirror | `needs_review` on the primary leg |
| 11 | Not a transfer, account off-budget | `adjustment` (source `offbudget_revaluation`) | primary | - |
| 12 | Not a transfer, on-budget, category kind `income` | `income` | primary | - |
| 13 | Not a transfer, on-budget, category kind `expense` | `expense` | primary | - |
| 14 | Not a transfer, on-budget, category kind `debt_principal` or `savings` | `expense` | primary | `misfiled` |
| 15 | Not a transfer, on-budget, no category | `expense` | primary | `needs_review` |

Why the unusual rows:

- **Rows 2 and 3 yield to pairs.** A paired transfer is net-worth neutral, so it can never be an opening balance or an adjustment. If a flagged row is a paired transfer, rows 4-10 decide it and it gets `needs_review`. A flagged row with no partner (unpaired) moves net worth, so rows 2 and 3 apply to it.

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

Resolved in P0.4: Actual keeps the category on mixed pairs and clears it on same-side pairs (`transfer.ts:24-45`). No server check requires a category on on->off transfers. Only the Close Account modal does (`desktop-client/src/components/modals/CloseAccountModal.tsx:33-40`, `docs/actual-format.md` §4.2). Uncategorized on->off transfers exist in real files, which is why row 10 exists. PLAN R-XFER-4's "which Actual requires to carry a category" is wrong and needs correcting.

### CLS-4.2 Existing Actual data with no metadata

On import, every on<->off transfer whose category has no Veritas kind lands on row 8 (`expense`), because that is what Actual's own reports do. Loan payments and savings moves therefore overstate expenses until the user answers once per category. Veritas must show a review count ("12 transfers to off-budget accounts are counted as spending") and a one-time wizard that assigns kinds to categories, never to individual rows. Net Profit stays reproducible and equal to Actual-style numbers until the user acts. Decided by Ryan at Gate 0 (FIN-1 in CLS-11): keep this default.

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
| `unreviewed_total` | minus the sum of `amount` of on-budget rows of kind `internal_transfer` that carry flag `needs_review` from row 10, or flag `unpaired` (an uncategorized on<->off transfer, or a transfer whose partner is missing). Positive = money out. Not part of `net_cash_flow` |

`income_total` is net of reversals: a negative amount in an income category reduces it. Withdrawals from savings (positive amount on kind `savings_contribution`) make `savings_total` negative and raise net cash flow. `net_cash_flow` is always exactly the formula above, on every screen and statement. `unreviewed_total` is reported next to it, never inside it. Statement and Sankey rules build on these definitions (statements.md).

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
| C-6 On->off transfer, no category | CHK -10000 to BRK, no category | Row 10: `internal_transfer`, `needs_review`. Does not touch `net_profit` or `net_cash_flow`. `unreviewed_total` = 10000 (ST-6) |
| C-12 Mirror leg has no category | T5m (LOAN +32000, no category) paired with T5a | Classified once from T5a (row 6). T5m copies kind `debt_principal`, `leg = mirror`, no flag. It is never sent to row 10 |
| C-13 Unpaired transfer | CHK -10000, payee = BRK transfer payee, `transferred_id` null, category Savings | Row 7 using `transfer_acct`: `savings_contribution`, flag `unpaired`. Net worth falls 10000 with no matching asset, so equity line E6 (ST-4) = -10000 |
| C-14 Merged category | Rows filed under deleted category "Dining Old", `category_mapping` points to "Dining" (expense) | Classified by "Dining". Same as Actual's budget sums |
| C-15 Child of a tombstoned parent | Parent tombstoned, child live | Not a row. Excluded from balances and every report, as in Actual |
| C-7 Unmarked category on an on->off transfer | Existing Actual file, CHK -32000 to LOAN, category "Car Loan" with no metadata | Row 8: `expense` until the user assigns kind `debt_principal` (CLS-4.2) |
| C-8 Same-side transfer | CHK to Visa | Row 4 on both rows. No flow totals change |
| C-9 Hidden category with activity | Category "Old Gym" hidden, 2027-01-04 CHK -3000 | Row 13: `expense`, included, labeled hidden (R-REP-3) |
| C-10 Kind conflict | `is_income = 0`, metadata kind `income`, row +5000 | effective kind `expense`, `kind_conflict`, row is row 13 |
| C-11 Split with mixed kinds | Parent CHK -10000: child -7000 Groceries, child -3000 Dining | Two `expense` rows. Parent not classified. Sum of kinds' amounts = -10000 |

## CLS-9 Reports vs the budget sheet (intentional divergence)

Facts about Actual's envelope sheet (`reference/actual/packages/loot-core/src/server/budget/base.ts:133-149`, `docs/actual-format.md` §5):

- A categorized starting balance is budget income. It feeds To Budget.
- Hidden categories count fully in spent.
- Spent sums live split children (not parents) in on-budget accounts only, by mapped category.
- Only the first income group feeds To Budget.
- Any on-budget row with a category is activity in that envelope, including the principal child of a loan payment (`transfer.ts` keeps the category on on->off children).

Veritas reports follow this file, not the sheet. Where they differ, the budget screen keeps Actual's number (R-BUD-1) and reports keep ours, on purpose.

| Case | Budget screen (Actual's math) | Reports (this spec) |
|---|---|---|
| New on-budget account, starting balance 50000, categorized to "Starting Balances" | To Budget +50000 | `income_total` 0, row 2 `starting_balance` |
| Loan payment, principal child -32000 in "Loan principal" | "Loan principal" envelope spent 32000. It must be funded or it shows overspent | Debt paydown 32000, not an expense |
| Hidden category with -3000 | spent 3000 | expense 3000 (labeled hidden) |

The loan wizard and the budget suggestions (FC-5) therefore treat the debt-principal envelope as something to fund each month.

## CLS-10 Paychecks recorded gross, with splits

Decided by Ryan at Gate 0 (FIN-4): paychecks are recorded as gross pay with withholding children, so the deposit is the split parent.

| Child | Category kind | Row | Kind | Tax bucket (tax-buckets.md) |
|---|---|---|---|---|
| Gross wages (positive) | `income` | 12 | `income` | `w2_income` |
| Income tax and payroll tax withheld (negative), e.g. federal, state, Social Security and Medicare | `expense` | 13 | `expense` | `taxes_withheld` |
| Pre-tax retirement contribution (negative), as a transfer to an off-budget retirement account | `savings` | 7 | `savings_contribution` | `retirement_contribution` |
| Pre-tax HSA contribution (negative), as a transfer to an off-budget HSA account | `savings` | 7 | `savings_contribution` | `hsa` |
| Insurance premium or other deduction (negative), no account | `expense` | 13 | `expense` | none |

Rules:

- Withholding is an expense, so Net Profit is gross pay minus taxes and other deductions. This is how Ryan's tax-adjusted income reads on the Income Statement.
- Retirement and HSA deductions are real money moved to an account the user owns, so they are savings contributions, not expenses. That needs an off-budget account to receive them. Without one, the child is on-budget, non-transfer, in a `savings` category: row 14 (`expense`, `misfiled`). The wizard tells the user to add the account.
- The parent amount is the net deposit and equals the sum of the children.
- Employer contributions are not cash on the paycheck and are not recorded unless the user records them as an off-budget revaluation (row 11).

Worked example C-16 (made-up numbers, checking CHK, 401k and HSA are off-budget accounts):

| Row | Amount | Kind |
|---|---|---|
| Parent, net deposit | +234400 | not classified |
| Gross wages | +400000 | `income` |
| Federal income tax withheld | -50000 | `expense` |
| State income tax withheld | -20000 | `expense` |
| Social Security and Medicare withheld | -30600 | `expense` |
| 401k, transfer to 401k account | -40000 | `savings_contribution` (mirror +40000 in the 401k account) |
| HSA, transfer to HSA account | -10000 | `savings_contribution` (mirror +10000) |
| Health premium | -15000 | `expense` |

Check: 400000 - 50000 - 20000 - 30600 - 40000 - 10000 - 15000 = 234400. `income_total` 400000. `expense_total` 50000 + 20000 + 30600 + 15000 = 115600. `net_profit` 284400. `savings_total` 50000. `net_cash_flow` 284400 - 50000 = **234400** = the net deposit.

## CLS-11 Decision log for the finance specs

| Id | Question | Status |
|---|---|---|
| FIN-1 | Unmarked on->off transfers in existing data: count as expense with a review count and a one-time per-category wizard | Resolved by Ryan (Gate 0, 2026-11-09): yes (CLS-4.2) |
| FIN-2 | Off-budget purchases booked with an ordinary expense category (rows 8 and 9) are expense, with equity line E2. Alternative: a new excluded kind | Resolved by Ryan (Gate 0, 2026-11-09): default accepted |
| FIN-3 | Escrow is an expense when paid to the servicer. Alternative: an off-budget escrow asset account | Resolved by Ryan (Gate 0, 2026-11-09): default accepted |
| FIN-4 | Paychecks recorded gross with splits (CLS-10) | Resolved by Ryan (Gate 0, 2026-11-09) |
| FIN-5 | No-spend days ignore bill-linked rows and loan-payment parts (FC-8) | Resolved by Ryan (Gate 0, 2026-11-09): default accepted |

## CLS-12 Currency and decimal places

Actual stores amounts as integers in the smallest unit it shows for the budget's `defaultCurrencyCode`: its currency table gives `decimalPlaces` (`reference/actual/packages/loot-core/src/shared/currencies.ts`), and its conversions divide by `10^decimalPlaces` (`shared/util.ts`, `integerToAmount`). Most currencies have 2 places. JPY, KRW and IRR have 0, so a stored `5000` is 5000 yen, not 50.00. The "None" currency has 2.

- **CLS-12.1** Every place a spec says "cents", "two places" or "whole dollars" means the budget currency's minor unit, its decimal places and its major unit. Code never hard-codes 2. The stored integer is never rescaled.
- **CLS-12.2** Amounts shown or exported as decimal text use the currency's decimal places: 2 places for USD (`-5000` is `-50.00`), 0 for JPY (`-5000` is `-5000`). This covers the tax summary `total` (TAX-5), the `#goal` note line (DEBT-8.2) and every report.
- **CLS-12.3** Rounding to whole major units (FC-5 suggestions and notices, FC-3 notes) rounds to a multiple of `10^decimalPlaces` minor units: 100 for USD, 1 for JPY. With 0 decimal places a rounding to whole units changes nothing.
- **CLS-12.4** Integer arithmetic, rounding rules and identities are unchanged by the number of places. Nothing is converted through a float.
- **CLS-12.5** Example, with made-up amounts: a USD budget's `12345` shows `123.45` and rounds to `12300` to the nearest whole dollar. A JPY budget's `12345` shows `12345` and stays `12345`. An FC-5 suggestion of 7089 rounds to 7100 in USD and stays 7089 in JPY.
- **CLS-12.6** Open question for Ryan: a budget whose `defaultCurrencyCode` is not in Actual's table. Option A: treat it as 2 places and list it in the audit (R-REL-2). Option B: refuse to open it read-write. Until decided, code must not guess a different number of places.
