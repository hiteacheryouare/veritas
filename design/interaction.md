# Interaction notes

Keyboard flows, the arithmetic field and motion.

## Global shortcuts

| Keys | Action |
|---|---|
| ⌘K | Command palette: "Go to account", "New bill", "Move money", any envelope, any screen |
| ⌘N | Quick entry sheet, from anywhere |
| ⌘← / ⌘→ | Previous / next month on Budget, Net Profit, Sankey and the register's month filter |
| ⌘1 … ⌘6 | Budget, Accounts, Bills, Goals, Reports, Insights |
| ⌘F | Search the current list |
| Esc | Close the top sheet or popover, or leave edit mode |

## Budget

- ↑ / ↓ move the selection between envelope rows. Group heads are skipped.
- ⌥↑ / ⌥↓ jump to the previous or next group.
- Return, or typing any digit or operator, opens the Budgeted field of the selected row.
- ⌘D toggles Dense. It keeps the selection and the scroll position.
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
- The balance graph covers the register's date filter. Moving the pointer over it shows the balance at the end of that day. Hovering is the only interaction; the date filter changes the range.
- Clicking a tag in Notes sets the tag filter to that tag. The tag filter also opens from its popup, and Esc clears it. While a tag filter is on, the Balance column is hidden and the filter bar shows the count and total of the matching rows.
- Move money opens with the current account as From. Tab order: From, To, Amount, Date, then Interest when the destination is a loan. Principal is read-only and is always the rest of the amount.
- Return triggers "Move money" when the amounts add up. Esc cancels.

## Quick entry (⌘N)

- Payee first. Picking a known payee fills the envelope and amount from that payee's history, and the amount is selected so it can be typed over or appended to.
- Tab walks Payee, Amount, Envelope, Account, Date, Notes. Return saves and closes. ⌘Return saves and opens a fresh entry.
- Typing `#` in Notes opens a list of existing tags, most used first, narrowed as you type. ↑ / ↓ pick one, Return or Tab inserts it. A tag that isn't in the list yet is added to it when the transaction is saved. Actual finds new tags in notes the same way (`tags-discover`, `reference/actual/packages/loot-core/src/server/tags/app.ts:30`).

## Tags (Settings)

- Click a tag name to rename it, or press Return on the selected row. Return saves, Esc cancels.
- The color well opens the system color picker. Description, tax bucket and Hidden save as soon as they change.
- Hidden tags still show in notes and count in reports. They are left out of the tag filter and the `#` list.

## Bills

- In Upcoming, ↑ / ↓ move between bills. Return on a bill is the same as clicking Paid: the amount field opens with the expected amount selected.
- Return in that field marks the bill paid with the entered amount. Esc puts the row back.

## Goals

- Arrow keys move between goal cards. Return opens the goal.
- Close goal opens a sheet. The destination popup has focus first. Return closes the goal, Esc cancels. When the leftover is negative and the chosen envelope would go over, the warning appears before the button is pressed, not after.

## Reports and insights

- Every number in Net Profit and every Sankey node can take focus. Return opens its transactions, the same as a click.
- Forecast: the horizon control is a segmented control (←/→ when focused). Up to three what-ifs; the add button says how many are left.
- Wrapped: ← / → change cards.

## Motion

Tokens are in `tokens.md`; the full system is in `motion.md`.

| What | Motion | Notes |
|---|---|---|
| Sidebar, ⌘1 … ⌘6, ⌘K | Page fade-through | The old page fades out over 100 ms; the new one fades in over 200 ms, starting 60 ms in and rising 4 pt. Its content appears when loaded, and a spinner only after 250 ms |
| A row opening its page (Accounts to a register) | Push | Comes in 24 pt from the trailing side as the list leaves |
| Dashboard card to its page | Morph | The card grows into the page and shrinks back into its slot. The grid keeps its scroll position |
| Drain bar after a budget change | `fast` | Width eases to the new value when Return commits |
| A figure changed by a commit | `fast` cross-fade | Never rolls through digits |
| Row selection, hover, focus ring | `fast` | Background only |
| Month change | `fast` cross-fade of the figures | Drain bars change at once. No sliding. A held ⌘← or ⌘→ changes without animation after the first press |
| Dashboard drag | `settle` | The card follows the pointer, settles into its slot on release and holds it while the core moves it. Neighbours settle around it |
| Banners and the find bar | `settle` | The page below settles as one comes or goes |
| Sheets and popovers | AppKit | New budget's steps cross-fade and the sheet's height settles. A loading sheet opens at its final height |
| Coach mark | `settle` | Moves to its next anchor, or to a target that appears late |
| Mo Nee question | `sheet` | The new question and answer rise 8 pt as they fade in. The answer grows without animation |
| Wrapped card change | `turn` | The next card drifts in 12 pt from the side it came from. The reader pages it; nothing advances on its own |

- Amounts never count up or roll. They cross-fade or change in place, and an amount being typed never animates.
- Nothing moves unless the user did something. The Mo Nee typing dots are the only loop.
- With Reduce Motion on, movement becomes a 150 ms fade at most, and small changes happen at once.

## Focus and accessibility

- The focus ring is `color-focus-ring`, 3.5 px outside the control. It shows on keyboard focus only.
- VoiceOver reads rows as a sentence: "Dining out, budgeted 180 dollars, spent 214 dollars 75, overspent by 34 dollars 75."
- State is never only color. See tokens.md, Color.
