# Interaction notes

Keyboard flows, the arithmetic field and motion. Shortcuts marked *proposed* are not in PLAN.md yet and need Ryan's yes.

## Global shortcuts

| Keys | Action |
|---|---|
| ⌘K | Command palette: "Go to account", "New bill", "Move money", any envelope, any screen |
| ⌘N | Quick entry sheet, from anywhere |
| ⌘← / ⌘→ | Previous / next month on Budget, Net Profit, Sankey and the register's month filter |
| ⌘1 … ⌘6 | Budget, Accounts, Bills, Goals, Reports, Insights *(proposed)* |
| ⌘F | Search the current list *(proposed)* |
| Esc | Close the top sheet or popover, or leave edit mode |

## Budget

- ↑ / ↓ move the selection between envelope rows. Group heads are skipped.
- ⌥↑ / ⌥↓ jump to the previous or next group *(proposed)*.
- Return, or typing any digit or operator, opens the Budgeted field of the selected row.
- The Dense toggle (⌘D, *proposed*) keeps the selection and the scroll position.
- Move budget opens with the selected envelope as From.

## Arithmetic fields

Every amount field (budgeted, quick entry, Move money, Paid, split amounts) behaves the same way. This is Actual's behavior.

1. **Open.** The field shows the current value as plain text with grouping separators and no currency symbol: `1,650.00`, `412.00`. The whole value is selected.
2. **Type over.** Typing a number replaces the selection.
3. **Append.** If what you type is a single operator (`+ - * / ^`) while the whole value is selected, it is appended to the value instead of replacing it. Typing `+40` on `412.00` gives `412.00+40`. Source: `reference/actual/packages/desktop-client/src/components/table.tsx:371-382`.
4. **Expressions.** `+ - * / ^`, parentheses and a leading minus are allowed. Grouping separators and a currency symbol inside a number are ignored. Grammar: `reference/actual/packages/loot-core/src/shared/arithmetic.ts`.
5. **Preview.** While the text is an expression, the row shows the result beside the field ("Return sets 452.00").
6. **Return** commits. In the budget it also moves to the next envelope's Budgeted field, and **Shift-Return** to the previous one (`reference/actual/packages/desktop-client/src/components/budget/BudgetTable.tsx:205-213`).
7. **Tab** commits too. In the budget it does the same as Return, as in Actual. In sheets and forms it moves to the next field, and **Shift-Tab** to the previous one.
8. **Esc** puts back the value from before editing and leaves the field. Nothing is written.
9. **Invalid input** does not commit. The field gets a red outline and the note "Can't read 412.00+ as an amount. Fix it or press Esc." The text stays as typed.
10. **Negative amounts.** Because a lone `-` appends, typing `-50` over a selected value gives `value-50`. To enter a negative number, delete the value first, then type `-50`.

## Accounts and Move money

- In a register, ↑ / ↓ move between transactions and Return edits the selected one. Transfer rows read as a direction plus the other account: `→ To Savings`, `← From Checking`.
- Move money opens with the current account as From. Tab order: From, To, Amount, Date, then Interest when the destination is a loan. Principal is read-only and is always the rest of the amount.
- Return triggers "Move money" when the amounts add up. Esc cancels.

## Quick entry (⌘N)

- Payee first. Picking a known payee fills the envelope and amount from that payee's history, and the amount is selected so it can be typed over or appended to.
- Tab walks Payee, Amount, Envelope, Account, Date, Notes. Return saves and closes. ⌘Return saves and opens a fresh entry *(proposed)*.

## Bills

- In Upcoming, ↑ / ↓ move between bills. Return on a bill is the same as clicking Paid: the amount field opens with the expected amount selected.
- Return in that field marks the bill paid with the entered amount. Esc puts the row back.

## Goals

- Arrow keys move between goal cards. Return opens the goal.
- Close goal opens a sheet. The destination popup has focus first. Return closes the goal, Esc cancels. When the leftover is negative and the chosen envelope would go over, the warning appears before the button is pressed, not after.

## Reports and insights

- Every number in Net Profit and every Sankey node can take focus. Return opens its transactions, the same as a click.
- Forecast: the horizon control is a segmented control (←/→ when focused). Up to three what-ifs; the add button says how many are left.
- Wrapped: ← / → change cards, Space pauses, H toggles Hide amounts *(proposed)*.

## Motion

| What | Duration | Notes |
|---|---|---|
| Drain bar after a budget change | 150 ms | Width eases to the new value when Return commits |
| Row selection, hover, focus ring | 150 ms | Background only |
| Month change | 150 ms | Cross-fade of the numbers. No sliding |
| Group collapse and Dense toggle | 150 ms | Height change |
| Sheets and popovers | 220 ms | Fade and a short drop from the toolbar. Exit uses `easing-exit` |
| Wrapped card change | 220 ms | Cross-fade. The story advances on its own every 6 s unless paused |

- Amounts never count up or roll. They change in place.
- Nothing moves unless the user did something, apart from Wrapped's auto-advance.
- With Reduce Motion on, every duration is 0. Wrapped still advances, without transitions.

## Focus and accessibility

- The focus ring is `color-focus-ring`, 3.5 px outside the control. It shows on keyboard focus only.
- VoiceOver reads rows as a sentence: "Dining out, budgeted 180 dollars, spent 214 dollars 75, overspent by 34 dollars 75."
- State is never only color. See tokens.md, Color.
