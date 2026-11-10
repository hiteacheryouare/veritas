# Net Profit, Sankey and statements (R-REP-2, R-REP-6, R-REP-7)

Status: draft for Gate 0. Owner: Fred. Builds on classification.md (cited as CLS-n). All amounts are integer cents. There is no division anywhere in this file, so there is no rounding.

## ST-1 Net Profit and Net cash flow (R-REP-2)

Definitions are CLS-6. Restated as the Net Profit report layout:

| Line | Value |
|---|---|
| Income | `income_total` |
| Expenses (interest, fees, escrow included) | `expense_total` |
| **Net Profit** | `income_total - expense_total` |
| Debt paydown (principal) | `debt_paydown` |
| Savings contributions | `savings_total` |
| **Net cash flow** | `net_profit - debt_paydown - savings_total` |
| Unreviewed transfers (only shown when non-zero, shown below Net cash flow, never inside it) | `unreviewed_total` (CLS-6), positive = money out |

Rules:

- **ST-1.1** Only rows with `leg != mirror` and kinds `income`, `expense`, `debt_principal`, `savings_contribution` contribute. `internal_transfer`, `starting_balance` and `adjustment` never appear in Net Profit.
- **ST-1.2** Refunds reduce expenses. A period's `expense_total` may be negative. It is shown as a negative number, never moved to income.
- **ST-1.3** Hidden and archived categories are included and labeled (R-REP-3).
- **ST-1.4** "Net cash flow" is the net change of the on-budget accounts from operations: see ST-6 for the proof that it equals the change in on-budget balances.
- **ST-1.5** Net cash flow has one definition everywhere (CLS-6). `unreviewed_total` is shown directly under it. ST-6 subtracts it in a separately named subtotal.
- **ST-1.6** Every number drills down to its rows, listing primary legs only, sorted by date then id.

## ST-2 Net worth

- **Balance of an account on date D** = sum of `amount` over its rows (CLS-1: alive, `is_parent = 0`) with `date <= D`. Children count, the split parent does not, so no double count.
- **Net worth on D** = sum of all account balances, every account, on-budget and off-budget, open and closed (edge case 18). Assets minus liabilities, because a liability balance is negative.
- **Asset or liability.** An account is a liability if `veritas_account_meta.kind = liability`. If there is no metadata, the account is a liability on date D when its balance on D is negative. This choice changes only which side of the balance sheet a line sits on. It never changes net worth.
- Closed accounts appear on a balance sheet whenever their balance on that date is non-zero. A zero-balance closed account is omitted.

## ST-3 The four statements (R-REP-7)

Header on every statement: configurable entity name, statement title, period or as-of date, currency. Footer on every statement: "Unaudited. Obviously." Amounts shown in dollars and cents from cents. Liabilities and expenses are displayed as positive numbers under their own heading. A loss is shown in parentheses.

| Statement | Date basis | Layout |
|---|---|---|
| Balance Sheet | As-of date D | Assets (accounts grouped by account group), Total assets. Liabilities (same), Total liabilities. **Net worth = total assets - total liabilities.** |
| Income Statement | Period | Revenue by income category (grouped), Total revenue. Expenses by category group then category, Total expenses. Net income = Net Profit (ST-1). Interest and fees are expense lines like any other. |
| Statement of Owner's Equity | Period | See ST-4. |
| Cash Flow Statement | Period | See ST-6. |

Checks that must hold on every fixture and in every test (each one failing is a defect, never a rounding gap):

1. Balance sheet: total assets - total liabilities = net worth.
2. Income statement net income = `net_profit` of ST-1 for the same period.
3. Equity statement closing equity = balance sheet net worth at the period end (ST-4).
4. Cash flow statement closing balance = sum of on-budget balances at the period end (ST-6).

## ST-4 Statement of Owner's Equity and the reconciliation

Opening equity = net worth on the day before the period starts. Closing equity = net worth on the last day of the period.

Every reconciling line is defined here. There are no others, and there is never a plug.

| # | Line | Formula over the period (primary legs unless stated) | Why it exists |
|---|---|---|---|
| E1 | Net income | `net_profit` | Earned minus spent |
| E2 | Capitalized transfers | minus the sum of `amount` for rows of kind `expense` or `income` that are on<->off transfers (CLS-4 rows 8 and 9) | Money left the budget as an "expense" but is still owned as an off-budget asset. Net worth did not fall, so equity adds it back. A row with amount -50000 adds +50000 |
| E3 | Starting balances | sum of `amount` for kind `starting_balance` (all accounts) | New accounts bring opening wealth |
| E4 | Reconciliation adjustments | sum of `amount` for kind `adjustment`, source `reconciliation` | Unexplained cash difference found at reconcile |
| E5 | Off-budget revaluations | sum of `amount` for kind `adjustment`, source `offbudget_revaluation` | Asset value changes (house, brokerage). Non-cash |
| E6 | Unpaired transfer legs | sum of `amount` of rows flagged `unpaired` whose kind is `internal_transfer`, `debt_principal` or `savings_contribution` (all accounts, both rows of a one-way link) | One side of a transfer is missing or the link is not reciprocal (edge case 4), so net worth moved with no matching partner. Zero on a clean file. A flagged problem, not a plug |

**Closing equity = opening equity + E1 + E2 + E3 + E4 + E5 + E6**, to the cent.

Why it closes: classify every row's effect on net worth (the sum of balances). A transfer pair nets to zero, so `debt_principal`, `savings_contribution`, `internal_transfer` change nothing. Non-transfer `income` and `expense` rows change net worth by their amount, which E1 captures. `income` and `expense` transfer pairs are in E1 but net to zero in net worth, which E2 cancels (E2 covers paired rows only. An unpaired `income` or `expense` row moves net worth by its amount, which E1 already has). Unpaired `internal_transfer`, `debt_principal` and `savings_contribution` rows move net worth with no income effect, which E6 captures. `starting_balance` and `adjustment` rows move net worth by their amount (E3, E4, E5). Every row falls in exactly one of these cases, so the sum is exact.

If the identity does not close, a row is misclassified or a row lacks a pair. The audit (R-REL-2) reports it. Veritas never adds a balancing line.

Presentation: E2 is labeled "Off-budget assets acquired (expensed above)". Lines E3 to E6 are hidden when zero. E1 is always shown.

## ST-5 Equity worked example (master scenario, classification.md CLS-8)

Opening balances 2026-12-31: CHK 200000, VISA 0, SAV 500000, LOAN -980000, BRK 1000000, CAR 0. Opening net worth = **720000**.

January closing balances:

| Account | Closing | Computation |
|---|---|---|
| CHK | 388800 | 200000 + 300000 - 40000 - 20000 - 50000 - 1200 |
| VISA | -7500 | -5000 - 4000 + 1500 |
| SAV | 520000 | 500000 + 20000 |
| LOAN | -948000 | -980000 + 32000 |
| BRK | 1015000 | 1000000 + 15000 |
| CAR | 50000 | 0 + 50000 |
| CD | 100000 | starting balance |

Assets = 388800 + 520000 + 1015000 + 50000 + 100000 = **2073800**. Liabilities = 7500 + 948000 = **955500**. Net worth = **1118300**.

| Equity line | Amount |
|---|---|
| Opening equity | 720000 |
| E1 Net income (CLS-8: 300000 - 65500) | 234500 |
| E2 Capitalized transfers (T8: -(-50000)) | 50000 |
| E3 Starting balances (T7) | 100000 |
| E4 Reconciliation adjustments (T10) | -1200 |
| E5 Off-budget revaluations (T9) | 15000 |
| **Closing equity** | **1118300** |

720000 + 234500 + 50000 + 100000 - 1200 + 15000 = 1118300 and matches the balance sheet.

February (chained): opening equity 1118300. E1 = 297500 (T11, T12). E2 to E5 = 0 (T13 is an internal transfer). Closing = **1415800**. Check by balances: CHK 678800, VISA 0, SAV 520000, LOAN -948000, BRK 1015000, CAR 50000, CD 100000, sum 1415800.

## ST-6 Cash Flow Statement

"Cash" means the on-budget accounts net of on-budget card debt: the money the envelopes govern. Card swipes lower it at swipe, which is how the budget sees them.

| Section | Line | Value |
|---|---|---|
| Opening | Budget accounts balance | sum of on-budget balances on the day before the period |
| Operating | Net Profit | `net_profit` |
| Financing | Debt paydown | minus `debt_paydown` |
| Saving and investing | Savings contributions | minus `savings_total` |
| | **Net cash flow** | operating + financing + saving. Same as CLS-6 |
| Review | Unreviewed transfers | minus `unreviewed_total` (negative when money left the budget) |
| | **Change in budget cash from operations** | net cash flow + unreviewed line |
| Other | Starting balances in budget accounts | sum of `amount` of kind `starting_balance` on on-budget accounts |
| Other | Reconciliation adjustments in budget accounts | sum of `amount` of kind `adjustment` on on-budget accounts |
| Closing | Budget accounts balance | sum of on-budget balances at the period end |

**Closing = opening + change in budget cash from operations + other lines**, to the cent.

Why it closes: on-budget rows are exactly the primary legs (the mirror leg of an on<->off pair is in the off-budget account) plus on<->on pairs (net zero). Primary legs are classified into the lines above. On-budget rows that are not primary legs of income, expense, debt or savings rows are internal transfers: paired on<->on rows net to zero, and uncategorized or unpaired ones are the review line. Off-budget `adjustment` and `starting_balance` rows are not in on-budget accounts.

January example: opening on-budget balance = CHK 200000 + VISA 0 = 200000. Net Profit 234500. Debt paydown -32000. Savings -20000. Net cash flow **182500**, review 0, change in budget cash 182500. Starting balances on on-budget accounts 0. Reconciliation adjustments -1200 (T10). Closing = 200000 + 182500 - 1200 = 381300 = CHK 388800 + VISA -7500. Starting balance T7 and revaluation T9 are off-budget, so they are not in this statement.

February example: opening 381300, net cash flow 297500, review 0, closing 678800 + 0 = 678800 (CHK 678800, VISA 0). The Visa payment does not appear anywhere: it is an on<->on pair.

## ST-7 Statement edge cases (tests)

| Case | Setup | Expected |
|---|---|---|
| S-1 Closed account with a balance | SAV closed with 20000 left | Still in Total assets and net worth, labeled closed |
| S-2 Off-budget revaluation, nothing else | BRK +15000 only | E5 = 15000, E1 = 0, closing = opening + 15000 |
| S-3 Starting balance never income | CD starting 100000 | E3 only. `income_total` unchanged |
| S-4 Capitalized transfer reversed | CAR sold: CAR -50000 -> CHK +50000, category Car purchase | Rows are kind `expense` (positive amount, expense_total -50000), E2 = -50000, net worth unchanged |
| S-5 Debt payment | 40000 split 32000/8000 | Net worth change 0 from principal; E1 reflects only -8000; LOAN balance moves 32000 toward zero |
| S-6 Zero-history period | Period with no rows | All lines 0, closing = opening |
| S-7 Unreviewed transfer | Month with Income 100000, Groceries 20000, and CHK -10000 -> BRK, no category | Net worth unchanged. `net_cash_flow` = 80000 on the Net Profit report and in ST-6. `unreviewed_total` 10000, review line -10000, change in budget cash 70000. On-budget balance rises 70000 |
| S-8 Unpaired transfer | CHK -10000 transfer to SAV, `transferred_id` null, category Savings | Net worth falls 10000. E6 = -10000. Closing equity still equals the balance sheet |

## ST-8 Sankey (R-REP-6)

### Nodes

| Column | Node | Value |
|---|---|---|
| Sources | One node per income category with positive net in the range | net `income` rows of that category |
| Sources | "Refunds and withdrawals" (only if non-zero) | sum of the magnitudes of negative-net expense, savings and debt-principal categories (ST-8 rule 3) |
| Sources | "Unreviewed transfers in" (only if non-zero) | `-unreviewed_total` if `unreviewed_total` is negative (net money into the budget) |
| Sources | "Shortfall" (only if non-zero) | `max(0, total outflows - all other sources)` |
| Middle | "Income" | sum of all source nodes |
| Outflows | One node per category group, children per category | net `expense` of positive-net categories |
| Outflows | "Savings" | `savings_total` if positive |
| Outflows | "Debt paydown" | `debt_paydown` if positive |
| Outflows | "Income reversals" (only if non-zero) | sum of the magnitudes of negative-net income categories (rule 3) |
| Outflows | "Unreviewed transfers" (only if non-zero) | `unreviewed_total` if positive |
| Outflows | "Unallocated" | the remainder, see below |

Flows are integer cents. Zero-value nodes and flows are omitted.

### Rules

1. **Range.** Month, quarter, year or any custom inclusive date range. Categories are netted across the whole range, not month by month.
2. **Only classified flows.** Rows of kind `internal_transfer`, `starting_balance` and `adjustment` never appear. Capitalized transfers (CLS-4 rows 8, 9) appear as ordinary expense categories.
3. **Negative nets.** A flow cannot be negative. Any expense category, savings or debt paydown whose net over the range is negative (refund bigger than spending) is dropped from the outflow side and its magnitude is added to the source node "Refunds and withdrawals". A negative-net income category (clawback) is dropped from sources and shown as an outflow node "Income reversals".
4. **Unallocated.** Unallocated is income that was neither spent, saved nor used to pay down debt in the range: Unallocated = Income node - (sum of every other outflow node). It is never negative. Whenever Shortfall is 0: Unallocated = `net_cash_flow - unreviewed_total` (invariant 3). If the sum of outflows exceeds the Income node, the excess is a "Shortfall" source (money drawn from earlier balances), the Income node grows by that amount, and Unallocated is 0.
5. **Hidden and archived categories** appear, labeled.
6. **Drilldown.** Clicking a node lists its primary-leg rows. "Unallocated" is derived, not a set of rows: it opens the income rows and the outflow totals it was computed from, captioned "Income not yet spent, saved or paid toward debt".

### Invariant (tests must assert all three)

1. Sum of income-category source flows minus the "Income reversals" node = `income_total` for the range, exactly. With no reversals, the income-category sources sum to `income_total`.
2. The Income (middle) node equals the sum of all source nodes, and the sum of all outflow nodes equals the Income node.
3. If "Shortfall" is 0, Unallocated = `net_cash_flow - unreviewed_total`. With no unreviewed transfers that is `net_cash_flow`. If "Shortfall" is greater than 0, Unallocated = 0 and Shortfall = `unreviewed_total - net_cash_flow`.

### Worked examples

| Case | Inputs | Expected |
|---|---|---|
| X-1 January master | Income Paycheck 300000. Expenses: Groceries 5000, Clothing 2500 (4000 - 1500), Loan interest 8000, Car purchase 50000. Debt paydown 32000. Savings 20000 | Sources: Paycheck 300000. Income node 300000. Outflows: Food 5000, Shopping 2500, Debt costs 8000, Big purchases 50000, Savings 20000, Debt paydown 32000, Unallocated **182500** (= net cash flow). Outflows total 300000 |
| X-2 February | Income 300000, Groceries 2500, card payment 10000 | Outflows: Food 2500, Unallocated 297500. The card payment is absent |
| X-3 Shortfall | Income 100000, Rent 90000, Dining 40000 | Sources: Paycheck 100000, Shortfall 30000. Income node 130000. Outflows: Housing 90000, Dining 40000. Unallocated 0. Invariant 1 holds at 100000, invariant 2 holds at 130000 |
| X-4 Refund exceeds spending | Income 100000, Groceries 20000, Clothing net -8000 (purchase 2000, refund 10000) | Sources: Paycheck 100000, Refunds and withdrawals 8000. Income node 108000. Outflows: Groceries 20000, Unallocated **88000**. Net cash flow = 100000 - (20000 - 8000) = 88000 |
| X-5 Unreviewed transfer | Income 100000, Groceries 20000, uncategorized transfer to BRK 10000 | Outflows: Groceries 20000, Unreviewed transfers 10000, Unallocated 70000. Invariant 3: `net_cash_flow` 80000 - `unreviewed_total` 10000 = 70000 |
| X-8 Income reversal | Paycheck +100000, income category "Interest" net -500 | `income_total` 99500. Sources: Paycheck 100000. Outflows: Income reversals 500, Unallocated 99500. Invariant 1: 100000 - 500 = 99500 |
| X-9 Unreviewed inflow | Income 100000, Groceries 20000, uncategorized transfer BRK -> CHK +10000 | `unreviewed_total` -10000. Sources: Paycheck 100000, Unreviewed transfers in 10000. Income node 110000. Outflows: Groceries 20000, Unallocated 90000 = 80000 + 10000 |
| X-10 Paycheck with splits | CLS-10 example C-16 | Sources: Paycheck 400000. Outflows: Taxes and deductions 115600 (by category group), Savings 50000, Unallocated 234400 = `net_cash_flow` |
| X-6 Empty range | No income, no spending | No nodes, no flows, no error |
| X-7 Split row | Split parent -10000 with children Groceries -7000, Dining -3000 | Groceries 7000, Dining 3000 |
