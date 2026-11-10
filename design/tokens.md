# Design tokens

`tokens.css` is the source of truth. Names are platform-neutral so they map one to one onto Swift (`Color.moneyAvailable`, `Font.figures`, `Spacing.s4`, `Motion.fast`). Drop the `--color-` style prefix and camelCase the rest.

Direction: Quiet native's layout and grouped lists, with drain bars, semantic color for every money state, and SF Pro Rounded figures.

## Type

| Token | Value | Use |
|---|---|---|
| `font-text` | SF Pro Text (`-apple-system`) | All words |
| `font-figures` | SF Pro Rounded (`ui-rounded`), tabular | Every amount, date number and percentage |
| `font-mono` | SF Mono (`ui-monospace`) | Not used in the UI yet; reserved for exports and debug views |
| `font-paper` | New York (`ui-serif`) | Statement PDFs only |
| `text-caption` | 11 | Column heads, notes, sidebar group names |
| `text-body` | 13 | Default (macOS body size) |
| `text-heading` | 15 | Toolbar titles, group heads in charts |
| `text-title` | 20 | Sheet titles, section titles |
| `text-display` | 40 | To Budget and other single hero figures |
| `text-showcase` | 64 | Wrapped cards |
| `weight-*` | 400, 500, 600, 700 | Figures in rows use 500; available and totals 600; heroes 700 |

## Color

Every color has a light and a dark value. There is no pure black or white anywhere.

| Token | Light | Dark | Use |
|---|---|---|---|
| `color-accent` | #5146d8 | #8f87ff | Selection, focus ring, primary buttons, sidebar icons. Never money |
| `color-money-available` | #127a49 | #4cc47a | Available amounts above the low threshold, on-track goals |
| `color-money-low` | #9a5a00 | #eda23a | Envelope at or under 20% left, goals behind schedule |
| `color-money-overspent` | #c4262e | #ff6a64 | Overspent envelopes, losses, shortfalls |
| `color-money-income` | #1668bd | #5eaaf5 | Income, To Budget, money received |
| `color-money-neutral` | #9c9ca5 | #74747d | Zero amounts |
| `color-money-*-fill` | | | Bar, ring and chart fills for the same four states (brighter, for large areas) |
| `color-chart-expense` | #6e6e78 | #a5a5ae | Spending in charts. Spending is not a warning, so it is graphite |
| `color-chart-savings` | #26a066 | #3fb06d | Savings in Net Profit and the Sankey |
| `color-chart-debt-paydown` | #9b3f8a | #d272bf | Debt paydown in Net Profit and the Sankey |
| `color-scenario-1..3` | blue, plum, amber | | What-if lines. Baseline uses `color-scenario-baseline` |
| `color-surface-*` | | | window, sidebar (glass), grouped, raised (sheets, popovers), field, paper, scrim |
| `color-text-*` | | | primary, secondary, tertiary, on-accent, paper |

Color is never the only signal. Low and overspent always carry an icon (`low`: arrow into a circle, `over`: triangle) and words ("Low, $8.80 left", "Overspent by $34.75"). Income carries the tray glyph and the word "received" or "Income". Available is the default state: its words are the column head "Available", and a positive amount never gets an icon, so the warnings stand out. Negatives always use a real minus sign (U+2212): −34.75. Statements are the exception: they follow accounting layout and show a loss in parentheses (docs/finance/statements.md ST-3).

## Drain bars

A bar shows what is left: `available / (carried in + budgeted)`, clamped to 0–100%. A full bar is a full envelope. It is green while more than 20% is left, amber at 20% or under, empty at zero. An overspent envelope shows a red hatched bar at full width with no fill. The hatch is the non-color signal.

## Spacing and size

| Token | Value |
|---|---|
| `space-1..7` | 4, 8, 12, 16, 24, 32, 48 |
| `size-row` / `size-row-dense` | 40 / 24 |
| `size-control` / `size-control-small` | 32 / 26 |
| `size-sidebar` | 252 |
| `size-bar` | 6 |
| `size-ring` / `size-ring-large` | 52 / 96 |

## Radii

| Token | Value |
|---|---|
| `radius-window` | 24 |
| `radius-panel` | 18 (sidebar) |
| `radius-sheet` | 20 |
| `radius-section` | 12 (grouped lists, cards) |
| `radius-control` | 8 |
| `radius-field` | 6 |
| `radius-bar` | 3 |

## Motion

| Token | Value | Use |
|---|---|---|
| `duration-fast` | 150 ms | Bar width, selection, hover, row expand |
| `duration-sheet` | 220 ms | Sheets and popovers in and out |
| `easing-standard` | cubic-bezier(0.2, 0.7, 0.2, 1) | Everything that appears or changes |
| `easing-exit` | cubic-bezier(0.4, 0, 1, 1) | Things that leave |

With Reduce Motion on, both durations become 0 and values change in place. Nothing loops, nothing animates on its own.
