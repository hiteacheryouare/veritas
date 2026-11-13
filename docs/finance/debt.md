# Debt, loans and goals (R-DEBT-1..3, R-XFER-4, R-GOAL-1/4, edge case 8)

Status: draft for Gate 0. Owner: Fred. Builds on classification.md (CLS-n) and statements.md (ST-n). Amounts are integer cents.

Envelope budget math (To Budget, carryover, overspending, hold) is Actual's and is not redefined here. Where this file moves budget money it calls Actual's own move (`reference/actual/packages/loot-core/src/server/budget/actions.ts:643-677`, `transferCategory`).

## DEBT-1 Definitions

| Term | Meaning |
|---|---|
| Loan account | An off-budget account that is a liability. Its balance is negative (-980000 means $9,800.00 owed). `veritas_account_meta.kind = liability`. |
| Loan metadata (R-DEBT-1, all optional) | `rate_bps` (annual rate in basis points, integer, 980 = 9.80%), `payment_cents`, `payment_day` (1-31), `lender` (payee). |
| Outstanding principal | `-balance` of the loan account as of the day before the payment, in cents, never below 0. |
| Loan payment | One payment from an on-budget account to the lender, made of parts: principal, interest, escrow, fees. |
| Payment total | Money that left the paying account. Equals the sum of the parts. |

## DEBT-2 Meaning of each part

| Part | Kind (CLS-2) | Category kind | Where the money goes | Net worth effect |
|---|---|---|---|---|
| Principal | `debt_principal` | `debt_principal` | Transfer to the loan account, so the liability shrinks | 0 (cash down, debt down) |
| Interest | `expense` | `expense` (e.g. "Loan interest") | Lender, leaves the books | negative |
| Escrow | `expense` | `expense` (e.g. "Property tax and insurance") | Servicer holds it for bills it pays for you | negative |
| Fees (late, origination) | `expense` | `expense` (e.g. "Loan fees") | Lender | negative |

Rules:

- **DEBT-2.1** Principal, interest, escrow and fees are integers >= 0. The sum must equal the payment total exactly. The wizard computes principal as the remainder, so a leftover cent can never go missing: `principal = total - interest - escrow - fees`.
- **DEBT-2.1a** When a payment total is allocated to parts, as in a forecast (forecasting.md FC-2), it covers interest first, then escrow, then fees, then principal. Principal is what is left, floored at 0.
- **DEBT-2.2** Principal must not be negative. If a payment is smaller than the interest owed, the wizard blocks and says so. Unpaid interest added to the loan balance is recorded by the user as an `adjustment` on the loan account (a non-cash change), never as a negative principal.
- **DEBT-2.3** Principal larger than outstanding principal is a warning, not an error (a refund or escrow credit can explain it). The loan balance would turn positive and the account is listed as an asset that day by ST-2 (the side follows the sign of the balance, whatever the metadata says). The wizard shows "This pays off the loan and leaves a credit of $X".
- **DEBT-2.4** Net Profit sees interest, escrow and fees as expenses and principal as Debt paydown (ST-1). Escrow is its own category and never lands in principal or interest.
- **DEBT-2.5** Escrow is booked as an expense when paid to the servicer (simple, matches how a household budgets). It is not modeled as an off-budget escrow asset in v1. Resolved by Ryan at Gate 0 (FIN-3 in classification.md CLS-11). The alternative is an off-budget escrow asset account.
- **DEBT-2.6** Interest is booked when paid. Interest accrued but unpaid is not recorded. The loan balance the user sees moves only by principal, so it can differ from the lender's statement by accrued interest. This is a stated simplification of D7.

## DEBT-3 Pre-fill from the last payment (R-DEBT-2)

Order of sources for the wizard's defaults:

1. The most recent loan payment to this loan account (the last row of kind `debt_principal` in this loan account, grouped with its sibling parts): `total`, `interest`, `escrow`. `fees` is never pre-filled (0).
2. If no earlier payment exists, `payment_cents` from the loan metadata as the total, with interest as the estimate in DEBT-4 if `rate_bps` is set, escrow 0.
3. Otherwise nothing is pre-filled.

Then `principal = total - interest - escrow - fees` (DEBT-2.1). If principal exceeds outstanding principal, principal is capped at outstanding principal and the total is reduced by the same amount (a final, smaller payment). The user can overwrite every field.

The date defaults to today. The category fields default to the categories used by the last payment.

## DEBT-4 Interest estimate (a hint, never a default)

When `rate_bps` is set the wizard shows "About $X of this is probably interest". It is never written without the user accepting it.

`estimate = round_half_up(outstanding_principal * rate_bps / 120000)`

- `120000 = 10000 (basis points per 1.0) * 12 months`.
- Rounding: half up to the nearest cent. The estimate is only a hint, so there is no leftover to allocate. The stored split always comes from the user's entered interest, with principal as the remainder (DEBT-2.1).
- Real lenders use day counts and rate changes. The user's number wins and is stored as entered.

## DEBT-5 Worked examples

Standard payment: $400 = $320 principal + $80 interest. Loan balance -980000, `rate_bps` 980, payment from CHK.

| Case | Inputs | Expected |
|---|---|---|
| L-1 Standard split | total 40000, interest 8000 | principal 32000. Loan balance -980000 -> -948000. CHK -40000. Net Profit: expense 8000. Debt paydown 32000. Net worth change from the payment: -8000 |
| L-2 Estimate | outstanding 980000, rate 980 bps | 980000 * 980 = 960400000. 960400000 / 120000 = 8003.33. Estimate **8003**. Stored split uses the user's 8000 |
| L-3 Pre-fill from last payment | last: total 40000, interest 8000, escrow 0 | wizard shows total 40000, interest 8000, principal 32000. For the next payment the hint on balance -948000 is 948000 * 980 / 120000 = 7742 exactly (hint only) |
| L-4 Escrow | mortgage payment total 200000 = interest 100000 + escrow 40000, remainder principal 60000 | expense 140000. Debt paydown 60000. Loan balance moves by 60000. Net cash flow effect -200000 |
| L-5 Fee | total 40000 + late fee 2500 = 42500, interest 8000 | principal = 42500 - 8000 - 2500 = 32000. Expense 10500 |
| L-6 Extra principal | total 50000, interest 8000 | principal 42000. All of it is debt paydown. Expense 8000 |
| L-7 Final payment | outstanding 20000, pre-fill total 40000, interest 100 | principal capped at 20000, total becomes 20100, loan balance 0 |
| L-8 Overpay | principal 25000 on outstanding 20000 | warning. Loan balance +5000, moves to assets. Net worth unchanged by the payment |
| L-9 Payment below interest | total 5000, interest 8000 | blocked (principal would be -3000) |
| L-10 Zero-history loan | no loan metadata, no earlier payment | nothing pre-filled, wizard asks for total and interest |

## DEBT-6 Payment storage (verified in P0.4)

The wizard produces one `LoanPayment { account, date, payee, parts[] }`, stored as a split parent with a transfer child. Classification is CLS-4 rows 6 and 13.

Rows for L-1 (CHK paying 40000, parts principal 32000, interest 8000):

| Row | Account | Amount | Detail |
|---|---|---|---|
| Parent | CHK | -40000 | `isParent = 1`, payee = lender, no category |
| Child 1 | CHK | -32000 | payee = the loan account's transfer payee, category Loan principal (kind `debt_principal`), `parent_id` = Parent |
| Child 2 | CHK | -8000 | payee = lender, category Loan interest, `parent_id` = Parent |
| Counterpart | LOAN | +32000 | created by Actual's transfer logic for Child 1, `transferred_id` links to Child 1, no category |

With escrow (L-4) Child 3 is the escrow expense. Escrow and fees are children with expense categories, never transfers.

Rules:

- **A.1** Children sum to the parent: 32000 + 8000 = 40000 (audit rule, R-REL-2).
- **A.2** Editing the payment replaces the children and the counterpart in one SQLite transaction (R-REL-1, R-XFER-3). Deleting the parent tombstones the children and the counterpart.
- **A.3** Register shows one payment row "Loan payment -$400.00" expandable to its parts. The principal child renders `-> To Car Loan` (R-XFER-2).
- **A.4** Confirmed in Actual: `addTransfer` refuses only split parents (`reference/actual/packages/loot-core/src/server/transactions/transfer.ts:48-54`); a child with `transferred_id` and a standalone counterpart is tested at `transfer.test.ts:172-219`.
- **A.6** Budget activity (Actual's math, kept as is): the principal child is on-budget with a category, so it is spending in that envelope (`reference/actual/packages/loot-core/src/server/budget/base.ts:133-149`). The "Loan principal" envelope must be funded each month or it shows overspent, while reports show Debt paydown, not an expense (classification.md CLS-9). The wizard offers to budget the payment's principal into that envelope, and the pre-fill principal (DEBT-3) is the suggestion for that envelope (forecasting.md FC-5).
- **A.5** Deleting the *loan-side* counterpart does not delete the child. Actual unlinks it (payee and `transferred_id` set to null, `transfer.ts:103-111`), leaving an ordinary categorized child. R-XFER-3 "deleting one side deletes both" therefore holds from the checking side only. Veritas, deleting the loan-side row, must either delete the whole payment (parent, children) after confirmation or accept the unlink. Recommendation: confirm, then delete the whole payment in one SQLite transaction.

## DEBT-7 Credit cards (R-DEBT-3)

Cards are on-budget liability accounts. Spending counts at swipe date.

| Event | Rows | Kind | Effect |
|---|---|---|---|
| $50 grocery swipe | VISA -5000, category Groceries | `expense` | Groceries expense 5000 in the month of the swipe |
| Pay the card | CHK -5000 and VISA +5000, transfer | `internal_transfer` both | No Net Profit effect. CHK down, VISA up |
| One payment covering two statements | CHK -10000 / VISA +10000 covers Jan statement 7500 + Feb swipe 2500 | `internal_transfer` | No spending. The two swipes counted once each, in their own months |
| Refund to the card | VISA +1500, category Clothing | `expense` | Reduces expenses (CLS-8 C-1) |
| Card interest or annual fee | VISA -2000, category "Card interest" | `expense` | Expense (interest is a cost) |
| Cash advance | VISA -10000 and CHK +10000, transfer | `internal_transfer` | Not income and not spending. Debt up |
| Pay card from off-budget savings | SAV -5000 / VISA +5000, transfer, category kind `savings` on the VISA row | `savings_contribution` (a withdrawal) | Savings contributions -5000, Net cash flow +5000, VISA balance up 5000. Never income, never spending |

## DEBT-8 Goals

These are the finance rules for goals (R-GOAL-1, R-GOAL-4, edge case 8). Envelope math is Actual's.

### Definitions

| Term | Meaning |
|---|---|
| Goal | A category in a Goals group with `veritas_goal(target_cents, target_date?, status)`. Category kind `expense` (CLS-3.3). Actual cannot express target dates. It can hold the target amount as a long-term goal (`goal_def` with `template_settings.source = "ui"`, or `zero_budgets.goal` with `long_goal = 1`), and shows progress only when its experimental goal-templates flag is on. Veritas never depends on that display. |
| Available | The goal category's available balance for a month from Actual's envelope sheet (`leftover-<category>`). Not recomputed by Veritas. |
| Status | Stored in `veritas_goal`: `active`, `reached` or `closed`. It only moves forward (see DEBT-8.2). `reached_month` is stored with it. "Spent" is the sum of the category's net spending (CLS-6 `expense_total`), shown on the card as "Reached → Spent" once status is `reached` or `closed` and spending exists. |
| Carry | `available - budgeted(M)` for the goal in month `M`: money already in the envelope before this month's contribution. Because `available` already includes this month's spending in the goal, carry is net of it. |
| Months left | Number of budget months from the current month `M` through the target month, counting both, minimum 1. "Due in N months" means the target date falls in month `M + N - 1`, so the current month is month 1. The UI stores the target date, tests use any date in that month. No target date: no suggestion. |

### DEBT-8.1 Monthly suggestion (R-GOAL-1)

`remaining = max(0, target - carry)`. `monthly = ceil(remaining / months_left)`.

Carry excludes this month's own budgeted amount, so following the suggestion does not change it: after budgeting 25000 in January, carry is still 0 and the suggestion is still 25000.

- Rounding: ceiling, so the target is reached on time. The earlier months are rounded up, so the last month contributes less: `min(monthly, remaining after the earlier months)`. For month k, contribution = `min(monthly, remaining - (k-1) * monthly)` floored at 0. Goal suggestions stay in cents and are not rounded to whole dollars.
- Example G-1: target 150000, carry 0, current month 2027-01, target date 2027-06-15 ("due in 6 months": June is month 1 + 6 - 1). Months left 6. `monthly` **25000**. Total over six months 150000. The same goal with target date 2027-07-15 has 7 months left and suggests 21429.
- Example G-1b: January budgeted 25000, available 25000, so carry 0, still **25000**. In February, with 25000 carried and 25000 budgeted again, carry is 25000, `remaining` 125000, months left 5, monthly 25000.
- Example G-2: target 100000, months left 3, available 0: monthly **33334**, schedule 33334, 33334, 33332. Sum 100000.
- Example G-3: remaining 1, months left 3: monthly 1, schedule 1, 0, 0.
- Example G-4: carry >= target: remaining 0, suggestion 0.
- Example G-5: target date already passed, remaining 50000: months left 1, monthly 50000.

### DEBT-8.2 Lifecycle (R-GOAL-3)

`active` -> `reached` -> `closed`. The state is stored and only moves forward.

- **active to reached.** After every command that can change the goal's `available` (budget edit, move, transaction in or out of the category), Veritas checks `available >= target` for the current month and, if true, sets `reached` and `reached_month = M` in the same SQLite transaction (R-REL-1). Imported goals with no stored state are backfilled once: `reached_month` is the first month-end where Actual's `available >= target`.
- **Spending never reverts it.** Buying the laptop for 148000 drops `available` to 2000, but status stays `reached` and the card reads "Reached → Spent" (PLAN section 10). **Spent** means status `reached` with `available < target`. A spent goal gets no refill suggestion and no ETA (D66, GOAL-10). With `available >= target` the card reads "Reached".
- **closed.** Set by Close goal (DEBT-8.3), from `active` or `reached`.
- **Reopen.** Sets status to `reached` if `reached_month` is set, else `active`, and unhides the category. Nothing else. A reopened goal that was reached and is still spent keeps reading "Reached → Spent" and gets no suggestion, because status never moves back.
- **Re-target.** Status only moves forward, so a goal that was reached is not set back to `active`. Re-targeting means the user gives the category a new goal with a new target (and, optionally, a date): a new `veritas_goal` row in `active`, from the current month. The old goal keeps its `reached_month` and still counts once in Wrapped for the year it was reached. The new goal gets suggestions and an ETA like any active goal (DEBT-8.1, FC-6).

Actual representation (Ryan's choice at Gate 0): Veritas writes a note line `#goal <amount>` on the category, in major units with at most two decimals and no thousands separator (for 150000 cents: `#goal 1500.00`; Actual's grammar `goal-template.pegjs:29,90,96` per `docs/actual-format.md`), and writes `zero_budgets.goal` = target in cents with `long_goal` = 1 for the current month and each later month Veritas creates. Rules: only one `#goal` line per category; Actual's template apply nulls `goal_def` for categories whose notes lack the line, so Veritas keeps the line when it edits notes; Actual shows progress only behind its experimental goal-templates flag; target dates exist only in `veritas_goal`. Closing a goal leaves the note line and the existing rows untouched.

### DEBT-8.3 Close goal and release leftover (R-GOAL-4, edge case 8)

Let `M` be the current budget month and `L` the goal's **available** at the end of `M` as Actual's envelope math reports it (so carryover and spending are already included).

| Leftover | Action in month M | Result |
|---|---|---|
| `L > 0` | Move `L` from the goal to the envelope the user picks (or To Budget), using Actual's `transferCategory` semantics: goal budgeted amount `-= L`, destination budgeted amount `+= L` | Goal available 0 |
| `L = 0` | No move | Nothing changes |
| `L < 0` (overspent goal) | Move `|L|` from the user's chosen envelope to the goal: chosen budgeted amount `-= |L|`, goal budgeted amount `+= |L|` | Goal available 0. Chosen envelope's available falls by `|L|` |

Rules:

- **GOAL-1** Closing never writes, edits, deletes, re-categorizes or re-files a transaction. The only writes are budget amounts of month M (the two budget cells), the goal status and the category's `hidden` flag, plus the later months' budget cells only when the user checks GOAL-10's "Also move those". A diff of `transactions` before and after is empty.
- **GOAL-2** Budget amounts may go negative as a result (e.g. goal budgeted 0 in M and `L = 2000` makes it -2000). Actual's `setBudget` does not clamp (`actions.ts:126-153`). VERIFY in P0.4 against the oracle that no later step rejects negative budgeted amounts.
- **GOAL-3** A negative release can overspend the chosen envelope. Veritas does not block it. The sheet shows "Dining will be $X over" before confirmation and offers to pick a different envelope.
- **GOAL-4** Months before `M` keep their budget amounts. Months after `M` start from available 0 for the goal, so nothing carries.
- **GOAL-5** Past reports keep the goal's spending in its own category (R-GOAL-5, R-REP-3). Hiding changes nothing historical.
- **GOAL-6** If a transaction is categorized to a closed goal later, no release happens. The goal shows "Closed goal has new activity" and the user may reopen or move the budget by hand.
- **GOAL-7** The move writes Actual's usual movement note, as `transferCategory` does (`addMovementNotes`, `actions.ts:669-675`), so Actual shows the same history. WS-ENGINE owns parity.
- **GOAL-8** Releasing an overspent goal against To Budget (`L < 0`, source To Budget) is allowed and unclamped: goal budgeted amount `+= |L|`, To Budget falls by `|L|` and may go negative (overbudgeted, which Actual carries forward). Actual's own helpers clamp to To Budget or write a junk row, so Veritas writes the budget cell directly and the movement note (GOAL-7). The sheet warns "To Budget will be $X over" before confirmation, as in GOAL-3. The goal always ends at available 0.
- **GOAL-9** The `reached_month` backfill for imported goals scans months up to and including the current month only: the first month whose available is at least the target (the current month counts as soon as it is). Budget amounts planned for future months never mark a goal reached.
- **GOAL-10** (D66) **A spent goal is not nagged, and closing shows later budget.**
  - A goal with status `reached` and `available < target` reads "Reached → Spent". It has no refill suggestion (FC-5 reads "{Goal}: reached, keeping ${current budgeted}") and no ETA (FC-6). It gets them again only as a new goal after a re-target (DEBT-8.2).
  - Before the user confirms Close goal, the sheet lists the goal category's budgeted amounts in months after `M`: "{Goal} has {total} budgeted in {n} later months. It stays in the hidden envelope unless you move it." Months whose budgeted amount is 0 are not counted.
  - An unchecked option, "Also move those to {destination}", makes one budget move per later month with a non-zero amount, in the same command and SQLite transaction as the close, using the same destination and Actual's `transferCategory` semantics (GOAL-7 movement note per move): for each later month `m` with budgeted amount `b`, goal budgeted `-= b` and destination budgeted `+= b`. To Budget as destination only lowers the goal's cell. A negative `b` moves the other way. No transaction is touched (GOAL-1 still holds: these are budget cells).
  - Left unchecked, only month `M` is written (GOAL-1), and each later month with a non-zero amount in the closed goal's envelope shows the flag "Closed goal has budget". The flag goes away when the amount is 0. Nothing else changes.

Worked examples (all in month M, one goal "New laptop", target 150000):

| Case | Setup | Expected |
|---|---|---|
| R-1 Positive leftover | Budgeted 150000 over the months, laptop bought 148000, available 2000. Destination "General Savings" budgeted 30000 in M | Goal budgeted in M -2000, General Savings 32000, goal available 0, category hidden, no transaction touched |
| R-2 Negative leftover | Budgeted 150000, bought 152000, available -2000. Chosen "Dining" budgeted 10000 | Dining 8000, goal +2000, goal available 0. Dining available falls 2000 |
| R-3 Zero leftover | available 0 | No budget move. Status `closed`, category hidden |
| R-4 Nothing budgeted in M | Goal budgeted 0 in M, available 2000 | Goal budgeted -2000, destination +2000 |
| R-5 Release to To Budget | `L = 2000`, destination "To Budget" | Goal budgeted -2000, nothing else. To Budget rises 2000 by Actual's math |
| R-6 Last year's report | Goal spending 148000 in 2027, closed 2027-03 | 2027 Net Profit still shows the 148000 expense in the goal's category, labeled closed |
| R-7 Over-saved | Budgeted 160000, bought 148000, available 12000 | Release 12000 |
| R-8 Late activity | After closing, -1000 categorized to the goal | No new move. Goal shows the notice (GOAL-6) |
| R-9 Later months, left unchecked | Close in March 2027. Vacation (target 150000) has leftover 2000, released to "General Savings" (30000 in March). April, May and June each have 25000 budgeted in Vacation | March: Vacation -2000, General Savings 32000. April to June stay at 25000 each, 75000 in three later months, and each shows "Closed goal has budget". To Budget in those months is 25000 lower each than if Vacation were empty |
| R-10 Later months, "Also move those" checked | Same as R-9, checked | April, May and June: Vacation 25000 -> 0, General Savings +25000 each (75000 in total across the three months), one movement note per move, goal flags gone. March is as in R-9. No transaction is touched |
| R-11 Later months to To Budget | Same as R-9, destination To Budget, checked | April to June: Vacation 25000 -> 0. To Budget in each month rises 25000 by Actual's math. No other envelope changes |
| R-12 Reached then spent | Target 150000. Reached in February, then 148000 is spent, `available` 2000 | The card reads "Reached → Spent". No suggestion, no ETA. The user sets a new target of 80000: a new active goal, suggestion `ceil((80000 - carry) / months_left)` as usual. The old goal still counts once in that year's Wrapped |
