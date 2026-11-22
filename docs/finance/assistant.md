# AI answers (R-AI-1)

Status: draft. Owner: Fred. Builds on classification.md (CLS-n) and statements.md (ST-n). Applies to the on-device assistant's read-only Q&A. Amounts are integer minor units (cents), summed in the core. Examples use two places (CLS-12) and made-up names and amounts.

## AI-1 Who computes what

- **AI-1.1** The app resolves the period from the user's words against today's date, the core computes every figure, and the app formats the answer. The model never computes a date, a sum, a difference, a percentage or a conversion, and never states a figure that no tool returned.
- **AI-1.2** Each tool's first line is the answer. The model may add words around it but never changes a figure, a label or a date range.
- **AI-1.3** The model offers no figure for a range no tool returned. A question about a range the tools cannot resolve gets the tool's own refusal, not an estimate.
- **AI-1.4** No tool writes anything. Hypothetical questions never touch real data.

## AI-2 Spending (CLS-6, ST-1)

- **AI-2.1** "Spending" is the Expenses line of Net Profit: `expense_total` (CLS-6), the same figure as ST-1 for the same period. Refunds reduce it and it can be negative (ST-1.2). Hidden and archived categories are included and labeled (ST-1.3).
- **AI-2.2** `spending(period)` answers with three parts, in this order:
  1. `Spending, <range>: <expense_total>`
  2. `This is the Expenses line of the Net Profit report. Debt paydown and savings are not included.`
  3. The Expenses line by category group, one line each, `Everyday: 812.00`. Group lines sum to `expense_total` to the cent.
- **AI-2.3** The assistant never calls transfers, card payments, net cash flow, debt paydown or savings "spending", and never calls a refund income (CLS-2, ST-1.1).
- **AI-2.4** The ST-10.10 cards (including "Monthly spending") use raw cash movement and are not this figure. The assistant never quotes a dashboard card number as spending.

## AI-3 Categories and groups

- **AI-3.1** `categorySpending(name, period)` matches a category or a category group by name, ignoring case and accents. A group's figure is the sum of its categories.
- **AI-3.2** Totals are summed in integer cents in the core over the same classified rows as the Net Profit screen (CLS-1, CLS-6), so a category's figure equals its line in the drill-down (ST-1.6). A category with no rows reads `0.00`.
- **AI-3.3** First line: `Spending on <name>, <range>: <amount>`. A hidden or archived category reads `Groceries (hidden)` or `Groceries (archived)`. Second line: `From <n> transactions, on the Expenses line of the Net Profit report.` A split row counts each child once, never the parent (CLS-1).
- **AI-3.4** An income category (CLS-3) answers `Income from <name>, <range>: <amount>`, net of reversals, and says `on the Income line` instead.
- **AI-3.5** A category whose kind is debt paydown or savings is never labeled "Spending". See AI-5.
- **AI-3.6** No match, or several matches: `No category or group is called "groc". The closest names are Groceries, Gifts.` and no figure. The core never guesses which one was meant.

## AI-4 Income (CLS-6)

- **AI-4.1** `income(period)` first line: `Income, <range>: <income_total>`. Second line: `This is the Income line of the Net Profit report.` Then the lines by category.
- **AI-4.2** "Income" labels earn questions only. It is `income_total`, net of reversals (CLS-6). Refunds are never income. Transfers in, loan proceeds and savings withdrawals are never income.

## AI-5 Debt paydown and savings

- **AI-5.1** Debt paydown and savings are never quoted unless the user asks for them by name.
- **AI-5.2** When asked by name, the figure carries its own label (`Debt paydown, <range>` or `Savings, <range>`, and for one category `Debt paydown for <name>, <range>` or `Savings for <name>, <range>`), never "spending", and the answer says it is not spending. Interest, fees and escrow are expenses and are not debt paydown (ST-1).

## AI-6 Periods

- **AI-6.1** The label always states the exact range used. A whole calendar month or year reads `November 2026` or `2026`. A partial range reads `November 1 to 22, 2026` or `January 1 to November 22, 2026`. A whole-period label is never used for a partial range.
- **AI-6.2** Resolution, with today = 2026-11-22:

| User's words | Range | Label |
|---|---|---|
| nothing named, "this month" | 2026-11-01 to 2026-11-30 | November 2026 |
| "so far this month", "this month so far", "month to date" | 2026-11-01 to 2026-11-22 | November 1 to 22, 2026 |
| "last month" | 2026-10-01 to 2026-10-31 | October 2026 |
| "this year" | 2026-01-01 to 2026-12-31 | 2026 |
| "so far this year", "year to date" | 2026-01-01 to 2026-11-22 | January 1 to November 22, 2026 |
| "last year" | 2025-01-01 to 2025-12-31 | 2025 |
| "October" alone | the latest October not after today's month: 2026-10 | October 2026 |
| "October 2026", "2026-10" | as written, whole month | October 2026 |
| "2026" | as written, whole year | 2026 |
| `YYYY-MM-DD to YYYY-MM-DD` | as written | as written, partial form |

- **AI-6.3** "This month" and "this year" are the whole calendar period, matching the Net Profit screen, including days after today. "So far" and "to date" end today. Example: on 2026-11-22, "October" alone is October 2026. On 2026-09-05 it is October 2025. On 2026-10-05 it is October 2026, whole month.
- **AI-6.4** A refund dated in a later period than its purchase reduces the later period only (CLS-1, period of its own date).

## AI-7 Budget figures

- **AI-7.1** `budget(name, month)` first line: `Budgeted for <name>, <month>: <amount>`. Second line: `Budget screen figures for <month>: activity <a>, available <v>.`
- **AI-7.2** Activity and available are the Budget screen's own figures, money out positive for activity, passed through unchanged. The core does not recompute them and the model never derives one from the other. They are budget figures, not Net Profit figures, and may differ from AI-2 (CLS-9).
- **AI-7.3** The words "spent" and "spending" are never used for budget figures. Reassigning budgeted money is a budget change, not a transaction.

## AI-8 Worked examples (tests)

Today is 2026-11-22. Made-up data, one on-budget account. November rows: Groceries -20000 and -21200 (total -41200), Dining out -8000 with a refund +3000, Everyday group also holds Gas -10000. Debt payment: Car Loan Principal -30000. Savings contribution -20000. Salary +500000. A card payment -50000 between own accounts.

1. **Spending, November so far.** Expenses = 41200 + (8000 - 3000) + 10000 = 56200. Debt paydown 30000, savings 20000 and the card payment contribute nothing. Answer first line `Spending, November 1 to 22, 2026: 562.00`. Second line is the AI-2.2 sentence. Net Profit for the same range: 500000 - 56200 = 443800.
2. **Refund crosses the boundary.** A -8000 Dining out purchase on October 30 and its +8000 refund on November 2. October Dining out 80.00, November Dining out -80.00. `Spending on Dining out, November 2026: -80.00`. Never shown as income.
3. **Category with no rows.** `Spending on Gifts, November 2026: 0.00`, `From 0 transactions, ...`.
4. **Hidden category.** Groceries hidden, same rows: `Spending on Groceries (hidden), November 2026: 412.00`.
5. **Group.** Everyday = Groceries 41200 + Gas 10000 = 51200: `Spending on Everyday, November 2026: 512.00`.
6. **Income.** Salary +500000 and a -5000 reversal in Salary: income_total = 495000. A user who said "so far this year" gets `Income, January 1 to November 22, 2026: 4,950.00`, never the whole-year label.
7. **Ambiguous name.** "groc" matches nothing exactly: `No category or group is called "groc". The closest names are Groceries, Gifts.` and no figure.
8. **Budget.** Budgeted 45000, activity 41200: `Budgeted for Groceries, November 2026: 450.00` then `Budget screen figures for November 2026: activity 412.00, available 38.00.` (no carryover in this example). The word "spent" does not appear.
9. **Debt paydown, asked by name.** `Debt paydown, November 1 to 22, 2026: 300.00`, not part of the Expenses line.
10. **Card payment.** "How much did I spend on my card bill?" is answered with spending by category. The -50000 payment is not spending (CLS-2).
