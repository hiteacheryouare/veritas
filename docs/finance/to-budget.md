# To Budget quick actions (extends R-BUD-4; the Architect assigns the ID)

Status: draft for the independent Fred's D49 review. Owner: Fred. Builds on debt.md (DEBT-8, GOAL-n, DEBT-6 A.6), classification.md (CLS-3, CLS-12) and statements.md. Applies to the Budget screen's To Budget figure and the actions that spend it. Envelope math is Actual's and is not redefined here (R-BUD-1). Amounts are integer minor units on the budget scale (CLS-12.9), summed with checked arithmetic. Nothing here divides except DEBT-8.1's own ceiling. Examples use made-up names and amounts, a 2-place currency written as `$` unless an example says otherwise, and cents in the data lines.

## TB-1 The To Budget card and its popover

- **TB-1.1** The Budget screen shows the month's To Budget figure as a centred hero card. Its label follows Actual's (`ToBudgetAmount.tsx:59`): `To Budget:` when the figure is 0 or more and `Overbudgeted:` when it is negative. The figure is Actual's `to-budget` for the displayed month, unchanged (R-BUD-1).
- **TB-1.2** Clicking the card opens a popover with these rows, each with a subtitle that says what it would do before it is clicked:
  - **Assign to underfunded** (TB-3)
  - **Fund goals' monthly** (TB-4)
  - **Hold for next month…** (TB-5)
  - and Actual's own rows, as the Budget screen already offers them, with Actual's conditions (`ToBudgetMenu.tsx:30-80`): `Move to a category` when To Budget is above 0, `Cover from a category` when it is below 0, and `Reset next month's buffer` or `Disable current auto hold` when something is held.
- **TB-1.3** A row that cannot act is disabled and says why in its subtitle: "To Budget is $0.00" when it is 0, "Overbudgeted by $50.00. Cover it from a category first." when it is negative (the card says Overbudgeted:, and Actual offers Cover from a category), "Nothing is underfunded", "No active goal needs funding". Opening the popover writes nothing.
- **TB-1.4** Assign to underfunded and Hold apply to the **displayed month `M`**, which may be any month Actual has created. Fund goals' monthly applies to the **current month only** (TB-4.1), and is disabled for any other month with the subtitle "Goals are funded for the current month only.", because DEBT-8.1's months left and carry are defined for the current month. **In a currency without 2 places all three actions are disabled** (TB-2.6).

## TB-2 What the three actions share

- **TB-2.1** They only **add** budget to envelopes, or hold To Budget back. None takes budget from an envelope, none touches a transaction, and none moves money between accounts (a budget change is not a transaction, AI-7.3).
- **TB-2.2** **The cap.** Every amount is clamped to the To Budget that is left at that point. Each write reduces To Budget by what it writes, so the next write is clamped against what remains, as `fundPrincipal` does (debt.md DEBT-6 A.6). To Budget therefore never goes below 0 through these actions, and the sum written is at most the To Budget the action started with. A write that cannot be filled in full is filled in part with what is left (`min(need, To Budget left)`), and the rows after it get nothing.
- **TB-2.3** **Order.** Envelopes are taken in the Budget screen's order: category group order, then category order, then category id. Hidden groups and hidden categories are left out of the automatic actions, so hiding an envelope is never undone by a click. The popover's subtitle says "n hidden envelopes aren't included" when any hidden envelope would otherwise qualify.
- **TB-2.4** **One undo step.** Each action is one command: one SQLite transaction (R-REL-1) and one undo step (R-REL-5), named after the row ("Undo Assign to underfunded"). The plan is computed inside the command from the sheet as it stands, so a To Budget that changed since the popover opened gives the result of the rules on the current sheet. The result line reads "Assigned $412.00 to 3 envelopes" with Undo. Undo restores changed budget cells and notes exactly. A month-note row that an action created is retired by the session's undo (its text becomes NULL), as for any created Actual row.
- **TB-2.5** **Which writes.** Each write uses one of Actual's own budget actions, so Actual reads the file as if the user had made the same move there (parity, TB-7).
- **TB-2.6** **A currency without 2 places (D74, CLS-12.10, CLS-12.13).** To Budget is a mixed figure there: income and spending on the register scale minus budgeted on the budget scale, so in a yen budget it can be many times its true size: with income stored as 5000000 and ¥49,500 budgeted it reads ¥4,950,500 where the true amount left is ¥500. Every cap in these actions would be measured against that figure, so a click could assign far more than the user has. In such a budget Assign to underfunded, Fund goals' monthly and Hold for next month… are therefore disabled, each with the fixed line "Spending is recorded at 2 places and budgets at {places}, so To Budget can't be trusted here.", where `{places}` is the currency's decimal places. This is consistent with CLS-12.13, which holds the screens of such budgets until the oracle reproduces them. Actual's own rows in the popover (Move to a category, Cover from a category, Reset next month's buffer) are not changed by this spec. Converting To Budget to a true figure (CLS-12.11's conversion, with its rounding rule) would let these actions run there. It is a later decision (TB-9, question 5).

## TB-3 Assign to underfunded

- **TB-3.1** **Candidates.** Live, non-income, non-hidden envelopes (TB-2.3). A category of kind `debt_principal` or `savings` is a candidate like any other (CLS-3), since an unfunded loan-principal envelope shows overspent in Actual (debt.md A.6).
- **TB-3.2** **Pass 1: overspent envelopes.** For each candidate with `available < 0`, in order, cover the whole overspending from To Budget: Actual's `coverOverspending` with `from = To Budget` (`actions.ts:518-577`), which adds `min(-available, To Budget left)` to the envelope's budgeted amount and, like Actual, appends its movement note "Reassigned {amount} from To Budget → {category} on {Month dd}" to the month's notes (`addMovementNotes`, `actions.ts:727-777`).
- **TB-3.3** **Pass 2: monthly goal shortfalls.** After pass 1, for each candidate with a **monthly goal** in Actual's own figure, `zero_budgets.goal` for `M` with `long_goal` not 1, and `budgeted < goal`, in order, add `min(goal - budgeted, To Budget left)` to the budgeted amount, as Actual's "Fund goal from To Budget" does (`BalanceMenu.tsx:41-48`, `transferAvailable`, `actions.ts:578-593`). This is the shortfall Actual's goal status calls "Underfunded" (`GoalStatus.tsx:36-60`). Veritas reads the stored figure and never computes templates (R-BUD-6). A long-term goal (`long_goal = 1`) is a target and not a monthly amount, so it is not filled here. A Veritas goal's own monthly suggestion is filled by TB-4, not here. No movement note, as `transferAvailable` writes none.
- **TB-3.4** **Pass 1 before pass 2.** Overspending is covered before any goal is topped up, and pass 2 reads each budgeted amount after pass 1.
- **TB-3.5** **The subtitle** reads "{n} envelopes, ${total}" from the same rules, with the list a tap away, and "Nothing is underfunded" when `n` is 0. Disabled when To Budget is 0 or below.
- **TB-3.6** **A currency without 2 places:** the action is disabled (TB-2.6). Neither pass runs, because `available` and To Budget both mix the two scales.

## TB-4 Fund goals' monthly

- **TB-4.1** **Candidates, and the month.** The action is offered only when the displayed month is the **current** month, because DEBT-8.1 defines months left and carry for the current month. For a past or a future month it is disabled (TB-1.4). Each Veritas goal (DEBT-8) with status `active`, whose category is live and not hidden, in order. Goals that are `reached`, `closed` or spent (DEBT-8.2, GOAL-10) get nothing, because a reached goal has no refill suggestion.
- **TB-4.2** **The amount.** The goal's monthly suggestion for `M` as the Goals screen shows it (DEBT-8.1: `remaining = max(0, target - carry)`, `monthly = ceil(remaining / months_left)`), and for each goal the need is `max(0, monthly - budgeted(M))`. Its budgeted amount is raised by `min(need, To Budget left)`. Following the suggestion leaves the carry unchanged (DEBT-8.1, G-1b), so running the action twice in a month writes nothing the second time. It never lowers a budgeted amount that is already above the suggestion.
- **TB-4.3** **Skipped, and said so.** A goal with no target date has no suggestion (DEBT-8.1), and is skipped and listed in the subtitle: "1 goal skipped: no target date". In a budget whose currency does not have 2 places the whole action is disabled (TB-2.6), which also covers D77's goals with recorded spending.
- **TB-4.4** **Writes.** `transferAvailable` (`actions.ts:578-593`): the budgeted cell only, no movement note, no transaction. The goal's status is re-tested by DEBT-8.2's rule after the command (a goal whose available now reaches the target becomes `reached`).
- **TB-4.5** **The subtitle** reads "{n} goals, ${total}", and "No active goal needs funding" when `n` is 0.

## TB-5 Hold for next month…

- **TB-5.1** This is Actual's hold (`budget/hold-for-next-month`, `actions.ts:486-512`, and `calcBufferedAmount`, `actions.ts:34-41`), which Veritas already implements (`holdForNextMonth(month:amount:)`, `resetHold(month:)`). The row is offered when To Budget is above 0 and nothing is auto-held, as Actual's menu offers it (`ToBudgetMenu.tsx:46-54`). It opens a field "Hold this amount:" that defaults to the whole To Budget and accepts arithmetic like every amount field (R-BUD-3). The button is "Hold".
- **TB-5.2** **The clamp.** `change = min(max(amount, -held), max(To Budget, 0))`, and the new hold is `held + change`. Holds add up, a negative amount releases part of a hold, nothing above To Budget is held, and nothing is held or released when To Budget is 0 or below (Actual returns `false`), and Reset is the way to release then.
- **TB-5.3** **What it writes.** The month's `buffered` figure only. Held money leaves To Budget for the month and reappears in next month's To Budget. Reset (`resetHold`, `actions.ts:514-516`) sets `buffered` to 0.
- **TB-5.4** In a budget without 2 places the row is disabled (TB-2.6): Actual clamps the hold against the mixed To Budget, which is not a true figure. Reset stays available, since it only sets the hold to 0.

## TB-6 Naming

- **TB-6.1** **Decision (Ryan, 2026-11-26), on Fred's ruling: both names stay. The Budget screen's action is "Move budget" and is not renamed "Move money".** "Move money" is already the register's action for a transfer between accounts (R-XFER-1, `MoveMoneySheet`, the command palette and ⌘K). That action writes a pair of transactions. Moving budget between envelopes writes no transaction and does not change any balance (AI-7.3, classification CLS-2). One name for both would invite a user to think an envelope move is a transfer or the reverse. Actual keeps them apart too: "Transfer to another category" and "Move to a category" for budget, a transaction transfer for accounts.
- **TB-6.2** **Exact labels.**
  - Register toolbar, command palette: **Move money** (accounts, unchanged).
  - Budget toolbar and its sheet title and button: **Move budget** (envelopes, unchanged).
  - To Budget popover: **Assign to underfunded**, **Fund goals' monthly**, **Hold for next month…**, and the existing **Move to a category**, **Cover from a category**.

## TB-7 Parity with Actual

| Veritas | Actual | Source |
|---|---|---|
| Hold, Reset | `holdForNextMonth`, `resetHold`; the clamp `min(max(amount, -buffered), max(toBudget, 0))` | `actions.ts:34-41, 486-516`; `HoldBufferModal.tsx`; `ToBudgetMenu.tsx:46-54` |
| Pass 1 | `coverOverspending` from `to-budget`, with the movement note | `actions.ts:518-577, 727-777` |
| Pass 2 and TB-4 | `transferAvailable` ("Fund goal from To Budget", `fundAmount = min(goal - budgeted, toBudget)`), no note | `actions.ts:578-593`; `BalanceMenu.tsx:41-48` |
| Underfunded | `GoalStatus`: for a monthly goal, `budgeted - goal < 0` | `GoalStatus.tsx:36-60` |
| Not done | "Apply budget template" and "Overwrite with budget template", which run `#template` notes by priority over the categories (the unbudgeted ones unless forced), spending To Budget, and write `goal` and `long_goal` | `BudgetMonthMenu.tsx:85-93`; `goal-template.ts:71-116, 240-300` |

Actual shows its "Fund goal from To Budget" row and the Underfunded status only when its goal-templates flag is on (`BalanceMenu.tsx:30`, `GoalStatus.tsx:36-60`). Veritas does not depend on that flag (DEBT-8, the Goal definition), so this is a difference of availability and not of rule. TB-3.3 reads only a stored `goal` figure, which exists only after Actual's templates have run, so a file with no stored goals gets pass 1 only, and "Nothing is underfunded" is then true to the stored data. Veritas does not run Actual's template engine (R-BUD-6), and these actions are not an equivalent of it. TB-3 and TB-4 write budgeted amounts only. A budget whose goal categories carry `#template` lines can still run Actual's own apply in Actual. Both write budget cells without a transaction and clamp to To Budget.

## TB-8 Worked examples (tests)

Month `M` is 2026-11. Envelopes in Budget screen order: Groceries, Gas, Insurance, Old Gym (hidden).

1. **Assign to underfunded.** To Budget 50000. Groceries available -12000, Gas available -3000, Old Gym (hidden) available -5000, Insurance budgeted 5000 with a monthly goal of 20000 (`long_goal` 0). Pass 1: Groceries covers 12000 (To Budget 38000), Gas 3000 (35000). Old Gym is left out. Pass 2: Insurance 20000 - 5000 = 15000 (20000 left). Written: Groceries +12000, Gas +3000, Insurance +15000, total 30000, To Budget after 20000. Two movement notes, one per covered envelope, none for Insurance. The subtitle reads "3 envelopes, $300.00" and notes "1 hidden envelope isn't included".
2. **The cap bites.** To Budget 10000, the same envelopes. Pass 1: Groceries covers `min(12000, 10000)` = 10000 (it stays 2000 short), To Budget 0. Gas, Insurance get nothing. Total 10000.
3. **Nothing to do.** To Budget 0 or negative: Assign to underfunded is disabled ("To Budget is $0.00"). With To Budget 50000 and no envelope below 0 or below its monthly goal: "Nothing is underfunded".
4. **Fund goals' monthly.** "New laptop": target 150000, target date 2027-06-15, carry 0, budgeted 5000 in `M`. Months left from 2026-11 through 2027-06 is 8, `monthly = ceil(150000 / 8)` = 18750, need 18750 - 5000 = 13750. "Trip": target 60000 due 2027-03-31, 5 months left, `monthly` 12000, budgeted 12000, need 0. "Emergency" is `reached`: nothing. To Budget 10000: Laptop gets `min(13750, 10000)` = 10000, shown "$100.00 of $137.50". Run again with To Budget 20000 and Laptop budgeted 15000: need 18750 - 15000 = 3750.
5. **Idempotent.** The setup of example 4 but with To Budget of 20000: the first run funds the whole 13750 (budgeted 18750, To Budget 6250). Running the action again writes 0, because budgeted 18750 equals the suggestion (need 0).
6. **Hold.** To Budget 35000, nothing held. The field defaults to 35000. Hold 20000: To Budget 15000, "For next month" 20000. Hold -5000 (a release): change `max(-5000, -20000)` = -5000, held 15000, To Budget 20000. Hold 30000: change `min(30000, 20000)` = 20000, held 35000, To Budget 0. Reset: held 0, To Budget 35000. With To Budget 0 the row is disabled, so a release needs Reset, because Actual's hold does nothing when To Budget is 0 or below.
7. **A budget without 2 places.** A yen budget with ¥50,000 of income (stored 5000000 on the register scale) and ¥49,500 budgeted (stored 49500 on the budget scale). To Budget reads 5000000 - 49500 = ¥4,950,500, against a true ¥500 left to budget. Assign to underfunded, Fund goals' monthly and Hold for next month… are all disabled with "Spending is recorded at 2 places and budgets at 0, so To Budget can't be trusted here." Nothing is written. A 3-place budget reads "…and budgets at 3…".
8. **Fund goals' monthly in another month.** On 2026-11-22 with the Budget screen showing 2026-10 or 2027-01, the action is disabled with "Goals are funded for the current month only." Assign to underfunded and Hold stay available there.
9. **One undo step.** After example 1, Undo restores Groceries, Gas and Insurance to their earlier budgeted amounts and the month's notes to their earlier text, as one step.

## TB-9 Decisions and open questions

**Decisions (Ryan, 2026-11-26)**

1. **Naming.** Both names stay: "Move budget" is for envelopes and "Move money" is for accounts (TB-6).
2. **"Assign to underfunded"** tops up to Actual's own stored goal figure for the month, the non-long goal (TB-3.3). A Veritas goal's own monthly suggestion is filled by "Fund goals' monthly" (TB-4).

**Open questions.** The defaults above stand until answered.

3. **Partial fills.** Default: the first envelope that cannot be filled in full gets what is left (TB-2.2). The alternative skips an envelope that cannot be filled in full.
4. **Hidden envelopes.** Default: left out and mentioned (TB-2.3). The alternative includes them.
5. **Running these actions in a currency without 2 places.** Default: disabled (TB-2.6), since To Budget is a mixed figure there (CLS-12.10, CLS-12.13). The alternative converts To Budget to a true figure on the budget scale (CLS-12.11's conversion) and runs the actions on it, which needs a stated rounding rule and an oracle budget first.

## TB-10 What the contract must carry, and how to implement it

- **TB-10.1** **A core read, not Swift.** `toBudgetActions(month:today:)` returns everything the popover shows, because the shell computes no figure (PLAN section 6.1): the To Budget figure and its state (positive, zero, overbudgeted), and for each of the three actions whether it is enabled, its disabled reason (TB-1.3, TB-1.4, TB-2.6), its plan with the amount for each envelope or goal and the total, the number of hidden envelopes left out (TB-2.3), and the goals skipped with their reasons (TB-4.3). The subtitles are formatted from these figures. The read writes nothing.
- **TB-10.2** **Two new commands.** `assignToUnderfunded(month:)` and `fundGoalsMonthly(month:)`, each one command and one undo step (TB-2.4). Hold uses the existing `holdForNextMonth(month:amount:)` and `resetHold(month:)`. A command re-plans from the sheet inside its transaction (TB-2.4), so it takes no plan from the read.
- **TB-10.3** **Implementation note: apply each write before planning the next.** A command that touches several envelopes must apply each write to its working copy of the sheet, and read To Budget, each budgeted amount and the month's notes from that updated copy, before it plans the next write. Planning every write from the sheet as it was at the start has two faults: the clamps never shrink, so the total can exceed To Budget (TB-2.2), and each movement note is built on the month's old notes and overwrites the one before instead of appending (TB-3.2). Examples 1 and 2 catch both: example 1 must append two notes, and example 2 must stop at 10000.
- **TB-10.4** **Undo.** A budget cell the command created comes back from undo as a 0-budgeted row, because the session's undo retires a created cell and does not delete it. It carries no goal figure: a cell these commands create is written with the budgeted amount only, and the retired form of a created cell clears `goal` and `long_goal` (MIG-7.5). Changed cells and notes return exactly (TB-2.4).

