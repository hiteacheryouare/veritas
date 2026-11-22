# AI answers (R-AI-1)

Status: draft, revised for the PR #93 review round. Owner: Fred. Builds on classification.md (CLS-n) and statements.md (ST-n). Applies to the on-device assistant's read-only Q&A. Amounts are integer minor units, summed in the core. Examples use made-up names and amounts, a 2-place currency written as `$` unless an example says otherwise, and cents in the data lines.

## AI-1 Who computes what

- **AI-1.1** The app resolves the period from the user's words against today's date, the core computes every figure, and the app formats the answer. The model never computes a date, a sum, a difference, a percentage or a conversion, and never states a figure that no tool returned.
- **AI-1.2** Each tool's first line is the answer. The model may add words around it but never changes a figure, a label or a date range.
- **AI-1.3** The model offers no figure for a range no tool returned. A question about a range the tools cannot resolve gets the tool's own refusal, not an estimate. The model's instructions carry this sentence exactly: "Never give a figure for a time range a tool did not return. If a tool refuses the range, say so."
- **AI-1.4** No tool writes anything. Hypothetical questions never touch real data.
- **AI-1.5** Every amount is formatted by the app with the currency symbol and places (CLS-12.3). The templates below write `<amount>` for that text, and write a negative with `-`, where the app prints its minus sign (U+2212). The model never reformats an amount. AI-2 to AI-5 and AI-9 are on the register scale (R). AI-7 is on the budget scale (B), as the Budget screen formats it (CLS-12.10).

## AI-2 Spending (CLS-6, ST-1)

- **AI-2.1** "Spending" is the Expenses line of Net Profit: `expense_total` (CLS-6), the same figure as ST-1 for the same period. Refunds reduce it and it can be negative (ST-1.2). Hidden and archived categories are included and labeled (ST-1.3).
- **AI-2.2** `spending(period)` answers with three parts, in this order:
  1. `Spending, <range>: <expense_total>`
  2. `This is the Expenses line of the Net Profit report. Debt paydown and savings are not included.`
  3. The heading `Spending by category group, <range>:`, then one line per category group, for example `Everyday: $512.00`, then the lines of AI-2.5.
- **AI-2.3** The assistant never calls transfers, card payments, net cash flow, debt paydown or savings "spending", and never calls a refund income (CLS-2, ST-1.1). When a question calls any of them spending, the answer contains this sentence exactly: "Transfers, card payments, net cash flow, debt paydown and savings are never spending."
- **AI-2.4** The ST-10.10 cards (including "Monthly spending") use raw cash movement and are not this figure. The assistant never quotes a dashboard card number as spending.
- **AI-2.5** The group lines reconcile to `expense_total` to the cent, like the ST-10.8 Total. A group line uses the ST-10.3 label of the group, for example `Everyday (hidden)`, and a group with no name reads `Unnamed group`. After the group lines come one line for each expense amount that belongs to no group:
  - rows with no category: `Uncategorized (needs review): <amount>` (CLS-4 row 15). It can be negative, because a positive uncategorized row reduces expenses.
  - a category whose group record no longer exists is in no group, and shows by its own ST-10.3 label, for example `(deleted) Old Gym: $12.00`.

## AI-3 Categories and groups

- **AI-3.1** **One figure per kind.** For a category or group and a period, each figure is the CLS-6 sum over rows of one row kind (`expense`, `income`, `debt_principal`, `savings_contribution`) whose resolved category (CLS-1) is that category, or any category currently in that group. A group's Spending counts only `expense` rows, never every row of the group's categories. The category's own kind is never a second classifier (CLS-3, CLS-4): a purchase booked to a debt or savings category with no transfer is an `expense` row (CLS-4 row 14, `misfiled`) and counts as Spending. The category's kind is read in one place only, AI-3.4.
- **AI-3.2** Totals are summed in integer cents in the core over the same classified rows as the Net Profit screen (CLS-1, CLS-6), so a figure equals its line in the drill-down (ST-1.6). Signs follow CLS-6: Expenses, Debt paydown and Savings are positive for money out, Income is positive for money in.
- **AI-3.3** **Lines.** The answer lists every kind that has at least one row, in the order Expenses, Income, Debt paydown, Savings, each with its own label:
  - `Spending on <label>, <range>: <amount>`
  - `Income from <label>, <range>: <amount>`
  - `Debt paydown for <label>, <range>: <amount>. It is not spending.`
  - `Savings for <label>, <range>: <amount>. It is not spending.`

  The first line is followed by `From <n> transactions, on the Expenses line of the Net Profit report.` (`on the Income line` for an Income first line). It reads `From 1 transaction` for one. n counts the classified rows of that line's kind: split children count, split parents and mirror legs do not (CLS-1). The Debt paydown and Savings lines carry their own sentence and no count. Naming a category or group counts as asking for it by name under AI-5.1, for that category or group only. `spending(period)` never does.
- **AI-3.4** **Nothing to report.** A category or group with no rows of any kind answers one line with `0.00` in the currency's places. It reads `Income from <label>` for an Actual income category or group, and `Spending on <label>` for anything else, with the matching second line. This is the only place the category's own kind is read.
- **AI-3.5** `<label>` is the ST-10.3 label, the same function the reports use: `Groceries (hidden)`, `Old Gym (hidden, archived)`, `Old Gym (closed goal)`, `(deleted) Old Gym`. Groups carry the same labels.
- **AI-3.6** **Matching names.** `categorySpending(name, period)` matches a category or a category group by a key. Both the asked name and each candidate name become a key the same way: fold case and accents, read `&` as `and`, treat any run of characters that is not a letter or digit as a word break, drop a leading `the`, `my` or `our`, then make each word singular (`ies` to `y` for words over 4 letters, otherwise drop a trailing `s`, but not `ss`, for words over 3 letters). On the asked name only, a trailing `category`, `categories` or `envelope` narrows the match to categories, `group` narrows it to groups, and `<name> in <group>` narrows it to that group's category.
  - **Exactly one exact key match:** it answers.
  - **More than one exact match, of any mix:** no figure, and `More than one category or group is called "Food": the category Food in Everyday, the category group Food. Ask which one.`
  - **No exact match:** no figure, and the closest names (AI-3.7).
- **AI-3.7** **Closest names.** At most 3, from the candidates ranked in tiers: (1) the candidate key starts with the asked key, (2) it contains it, (3) the asked key contains the candidate key, (4) any other candidate, kept only if its edit distance (Levenshtein, on keys) from the asked key is at most 2. Within a tier, sort by edit distance, then by name ignoring case. A name appears once. The answer reads `No category or group is called "groc". The closest name is Groceries.` for one, `The closest names are Groceries, Gifts.` for several, and just `No category or group is called "zzz".` for none. The core never guesses which one was meant.

## AI-4 Income (CLS-6)

- **AI-4.1** `income(period)` first line: `Income, <range>: <income_total>`. Second line: `This is the Income line of the Net Profit report.` Then the lines by category.
- **AI-4.2** "Income" answers questions about earnings only. It is `income_total`, net of reversals (CLS-6). Refunds are never income. Transfers in, loan proceeds and savings withdrawals are never income.

## AI-5 Debt paydown and savings

- **AI-5.1** Debt paydown and savings are never quoted unless the user asks for them by name. Naming a category or group (AI-3.3) or asking for the Net Profit report or net cash flow (AI-9) is asking by name. `spending(period)` and `income(period)` never quote them.
- **AI-5.2** When quoted, the figure carries its own label (`Debt paydown, <range>`, `Savings, <range>`, and for one category or group `Debt paydown for <label>, <range>` or `Savings for <label>, <range>`), never "spending", and the line says `It is not spending.` Interest, fees and escrow are expenses and are not debt paydown (ST-1).
- **AI-5.3** A negative figure adds a sentence. Savings: `Negative means more was withdrawn than saved.` Debt paydown: `Negative means more was borrowed than paid down.`

## AI-6 Periods

- **AI-6.1** The label always states the exact range used. A whole calendar month or year reads `November 2026` or `2026`. A partial range reads `November 1 to 22, 2026` or `January 1 to November 22, 2026`, with the year stated once at the end when both ends share it. A partial range across years reads `December 15, 2025 to January 3, 2026`. A whole-period label is never used for a partial range. An explicit range that equals a whole month or year gets the whole label.
- **AI-6.2** Resolution, with today = 2026-11-22 unless the row says otherwise:

| User's words | Range | Label |
|---|---|---|
| nothing named, "this month" | 2026-11-01 to 2026-11-30 | November 2026 |
| "so far this month", "this month so far", "month to date" | 2026-11-01 to 2026-11-22 | November 1 to 22, 2026 |
| "last month" | 2026-10-01 to 2026-10-31 | October 2026 |
| "last month", today = 2026-01-15 | 2025-12-01 to 2025-12-31 | December 2025 |
| "this year" | 2026-01-01 to 2026-12-31 | 2026 |
| "so far this year", "year to date" | 2026-01-01 to 2026-11-22 | January 1 to November 22, 2026 |
| "last year" | 2025-01-01 to 2025-12-31 | 2025 |
| "this month last year", "same month last year" | 2025-11-01 to 2025-11-30 | November 2025 |
| "October" alone | the latest October not after today's month: 2026-10 | October 2026 |
| "October 2026", "2026-10" | as written, whole month | October 2026 |
| "February 2024" | 2024-02-01 to 2024-02-29 | February 2024 |
| "2026" | as written, whole year | 2026 |
| `2026-10-01 to 2026-10-31` | as written, equals a whole month | October 2026 |
| `2025-12-15 to 2026-01-03` | as written | December 15, 2025 to January 3, 2026 |
| `2026-11-10 to 2026-11-05` | start after end | refused with the period hint, no figure |

- **AI-6.3** "This month" and "this year" are the whole calendar period, matching the Net Profit screen, including days after today. "So far" and "to date" end today. Example: on 2026-11-22, "October" alone is October 2026. On 2026-09-05 it is October 2025. On 2026-10-05 it is October 2026, whole month.
- **AI-6.4** A row belongs to the period containing its own date (conventions, top of classification.md). A refund dated in a later period than its purchase reduces the later period only, as in CLS-8 C-1.

## AI-7 Budget figures

- **AI-7.1** `budget(name, month)` first line: `Budgeted for <name>, <month>: <amount>`. Second line: `Budget screen figures for <month>: activity <a>, available <v>.`
- **AI-7.2** Activity is the Spent figure of Veritas's own Budget screen, as printed there. That screen prints money out as positive (`-spent`), unlike Actual's budget cell, which binds the raw `catSumAmount` and prints spending as negative (`reference/actual/packages/desktop-client/src/components/budget/envelope/EnvelopeBudgetComponents.tsx:473-477`). The core keeps Actual's sign. Only the Mac display flips it (D78, Gate 1 mockup D43). Sources: `apps/macos/Packages/VeritasMacUI/Sources/VeritasMacUI/Budget/EnvelopeRowView.swift:47` (`screen.format.amount(-envelope.spent, on: .budget)`), `apps/macos/Packages/VeritasMacUI/Sources/VeritasMacUI/Budget/EnvelopeGroupView.swift:23` (`figure(-group.spent)`), `design/mockups/budget.html` (Dining out shows spent 214.75) and `design/interaction.md:93` (VoiceOver reads "spent 214 dollars 75"). The assistant prints exactly what the Veritas screen prints and flips nothing itself. An envelope where refunds exceed purchases prints a negative activity, as the screen does. Available is the screen's own figure, not derived by the assistant or the model. These are budget figures, not Net Profit figures, and may differ from AI-2 (CLS-9). The word stays "activity" (see AI-7.3).
- **AI-7.3** The words "spent" and "spending" are never used for budget figures. Reassigning budgeted money is a budget change, not a transaction.
- **AI-7.4** In a 0-place budget, the answer adds the CLS-12.10 fixed line after the two answer lines: "Spending is recorded at 2 places and budgets at 0, so these numbers mix both, as in Actual." The word "over" never appears.

## AI-8 Worked examples (tests)

Today is 2026-11-22. Scenario A (examples 1, 5, 8, 10, 12): one on-budget account. November rows: category Groceries -20000 and -21200, category Gas -10000 (both in group Everyday), category Dining out -8000 and a refund +3000 (group Fun), Car Loan Principal -30000 (a debt principal transfer, row 6), a savings transfer -20000 (row 7), Salary +500000, a card payment -50000 between own accounts. Other examples state their own rows.

1. **Spending, November so far.** Expenses = 41200 + (8000 - 3000) + 10000 = 56200. Debt paydown 30000, savings 20000 and the card payment contribute nothing. Answer:
   - `Spending, November 1 to 22, 2026: $562.00`
   - `This is the Expenses line of the Net Profit report. Debt paydown and savings are not included.`
   - `Spending by category group, November 1 to 22, 2026:`
   - `Everyday: $512.00` and `Fun: $50.00`

   Net Profit for the same range: 500000 - 56200 = 443800.
2. **Refund crosses the boundary.** A -8000 Dining out purchase on October 30 and its +8000 refund on November 2. October Dining out $80.00, November `Spending on Dining out, November 2026: -$80.00`. Never shown as income.
3. **Category with no rows.** `Spending on Gifts, November 2026: $0.00`, then `From 0 transactions, on the Expenses line of the Net Profit report.` An income category with no rows: `Income from Side work, November 2026: $0.00`, then `From 0 transactions, on the Income line of the Net Profit report.`
4. **Labels.** Groceries hidden: `Spending on Groceries (hidden), November 2026: $412.00`. A hidden, archived category Old Gym with one -1200 row: `Spending on Old Gym (hidden, archived), November 2026: $12.00`, then `From 1 transaction, on the Expenses line of the Net Profit report.`
5. **Group.** Everyday = Groceries 41200 + Gas 10000 = 51200: `Spending on Everyday, November 2026: $512.00`.
6. **Income.** Salary +500000 and a -5000 reversal in Salary: income_total = 495000. A user who said "so far this year" gets `Income, January 1 to November 22, 2026: $4,950.00`, never the whole-year label.
7. **No match.** "groc" has no exact key. Keys: `groc` against `grocery` (starts with it, tier 1), `gas` (edit distance 3) and `gift` (edit distance 3). Answer: `No category or group is called "groc". The closest name is Groceries.` and no figure.
8. **Budget.** Budgeted 45000, Actual's `spent` -41200: `Budgeted for Groceries, November 2026: $450.00` then `Budget screen figures for November 2026: activity $412.00, available $38.00.` (no carryover). The word "spent" does not appear. Refunds win: budgeted 10000, `spent` +3000: activity `-$30.00`, available `$130.00`.
9. **Ambiguous name.** A group Food and a category Food in group Everyday: "food" answers `More than one category or group is called "Food": the category Food in Everyday, the category group Food. Ask which one.` and no figure. "food group" narrows to the group and answers. "Gift" and "Gifts" both exist as categories: the key is `gift` for both, so asking for either is ambiguous.
10. **Card payment.** "How much did I spend on my card bill?" No tool knows a card bill. The model calls `spending` for the range and answers with its first line, `Spending, November 1 to 22, 2026: $562.00`, the AI-2.2 second line, and the sentence "Transfers, card payments, net cash flow, debt paydown and savings are never spending." The -50000 payment is not in any figure (CLS-2).
11. **Group with mixed kinds.** Separate scenario: group Savings and Debt holds a -30000 principal transfer (row 6) in Car Loan Principal and a -1000 bank fee (row 13) in Fees. Answer:
    - `Spending on Savings and Debt, November 2026: $10.00`
    - `From 1 transaction, on the Expenses line of the Net Profit report.`
    - `Debt paydown for Savings and Debt, November 2026: $300.00. It is not spending.`

    Never $310.00 as spending. A -2000 purchase booked to the debt-kind category Car Loan Principal with no transfer is an `expense` row (CLS-4 row 14): `Spending on Car Loan Principal, November 2026: $20.00` and no Debt paydown line.
12. **Reconciliation.** Scenario A plus an uncategorized +4000 deposit (CLS-4 row 15, contributes -4000) and a -1200 row in a deleted category Old Gym whose group is gone. Expenses = 56200 - 4000 + 1200 = 53400. Lines: `Everyday: $512.00`, `Fun: $50.00`, `Uncategorized (needs review): -$40.00`, `(deleted) Old Gym: $12.00`. 51200 + 5000 - 4000 + 1200 = 53400.
13. **Debt paydown, asked by name.** `Debt paydown, November 1 to 22, 2026: $300.00. It is not spending.` A withdrawal-driven negative Savings line of -$50.00 adds `Negative means more was withdrawn than saved.`
14. **0-place currency.** Yen, made-up. A 500 yen purchase is stored -50000 (R), `expense_total` 50000: `Spending, November 2026: ¥500`. An envelope budgeted 600 (B): `Budgeted for Rice cooker, November 2026: ¥600`, then `Budget screen figures for November 2026: activity <a>, available <v>.` with a and v printed exactly as the Budget screen prints them (CLS-12.10), then the CLS-12.10 fixed line. The tests assert the line, the budgeted text and that "over" is absent. The activity and available digits come from the Budget screen's own formatter, not from this file.
15. **Periods.** The AI-6.2 table is the test table, one row per case. Expected labels are as written there.

## AI-9 Net Profit summary

`netProfit(period)` answers when the user asks for Net Profit, net cash flow, or the whole report. Asking for it is asking for debt paydown and savings by name (AI-5.1). Each line carries the range, in this order:

- `Net Profit, <range>: <amount>`
- `Income, <range>: <amount>`
- `Expenses, <range>: <amount>`
- `Debt paydown, <range>: <amount>. It is not spending.`
- `Savings, <range>: <amount>. It is not spending.`
- `Net cash flow, <range>: <amount>`
- `Unreviewed transfers, <range>: <amount>. Not in Net cash flow.` Shown only when not zero (CLS-6 `unreviewed_total`).

The negative sentences of AI-5.3 follow a negative Debt paydown or Savings line. The figures are the ST-1 lines for the range. Example, Scenario A: Net Profit `$4,438.00`, Income `$5,000.00`, Expenses `$562.00`, Debt paydown `$300.00`, Savings `$200.00`, Net cash flow `$3,938.00` (443800 - 30000 - 20000 = 393800). No Unreviewed transfers line.
