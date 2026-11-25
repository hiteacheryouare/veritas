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
| `color-accent` | #3b3d45 | #d9dae0 | Graphite. Selection, focus ring, primary buttons, sidebar icons. Never money |
| `color-accent-fill` | #2f3138 | #e2e3e8 | Primary button fill. In dark mode it is light, so `color-text-on-accent` flips to dark |
| `color-money-available` | #127a49 | #4cc47a | Available amounts above the low threshold, on-track goals |
| `color-money-low` | #9a5a00 | #eda23a | Envelope at or under 20% left, goals behind schedule |
| `color-money-overspent` | #c4262e | #ff6a64 | Overspent envelopes, losses, shortfalls |
| `color-money-income` | #1668bd | #5eaaf5 | Income, To Budget, money received |
| `color-money-neutral` | #6c6c75 | #97979f | Zero amounts. 4.5:1 or better on every surface |
| `color-money-*-fill` | | | Bar, ring and chart fills for the same four states (brighter, for large areas) |
| `color-chart-expense` | #6e6e78 | #a5a5ae | Spending in charts. Spending is not a warning, so it is graphite |
| `color-chart-savings` | #26a066 | #3fb06d | Savings in Net Profit and the Sankey |
| `color-chart-debt-paydown` | #8b5e34 | #c99a6b | Debt paydown in Net Profit and the Sankey |
| `color-scenario-1..3` | blue, umber, teal | | What-if lines. Baseline uses `color-scenario-baseline` |
| `color-surface-*` | | | window, sidebar (glass), grouped, raised (sheets, popovers), field, paper, scrim |
| `color-text-*` | | | primary, secondary, tertiary, on-accent, paper. Tertiary is #6c6c75 / #97979f: every text token reaches 4.5:1 on every surface |
| `color-surface-selected` | rgba(20, 22, 30, 0.08) | rgba(240, 240, 250, 0.12) | The selected sidebar row. Neutral, so it works with any accent; its text is `color-text-primary` |

### Accent

The accent is graphite. Interface chrome stays neutral, so the only color on screen belongs to money and to tags.

### Tags

Tags render as pills, as in Actual: the tag's stored `color` from Actual's `tags` table as the background and #fbfbfd text, caption size, radius 9. In dark mode the background is mixed 12% toward white so dark tag colors stay distinct from the dark surface.

### Money states

Color is never the only signal. Every money state has an icon and a word next to the amount:

| State | Icon | Word | Example |
|---|---|---|---|
| Available | check in a circle | left | 93.38 left |
| Low (20% or less left) | arrow into a circle | low | 8.80 low |
| Overspent | triangle | over | −34.75 over |
| Income | tray | To Budget, Income | $312.40 To Budget |

The icon is 12 px at 80% opacity and the word is caption size, so a column of available amounts stays quiet. A zero amount is neutral grey with no icon or word. Negatives always use a real minus sign (U+2212). Statements follow accounting layout: liabilities are positive numbers under their own heading.

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

`VeritasDesign/Motion.swift` mirrors these. Screens name a motion (`.motion(.settle, value:)`, `.motionTransition(.page)`, `Motion.perform(.morph) { }`) and never write a duration, curve or spring; `MotionEnforcementTests` fails the build if they do. The full system is in `motion.md`.

| Token | Value | Reduce Motion | Use |
|---|---|---|---|
| `duration-instant` | 90 ms | 0 | Hover, press, focus |
| `duration-fast` | 150 ms | 0 | Selection, drain bars, a figure cross-fading after a commit, spinners |
| `duration-sheet` | 220 ms | 150 ms fade | Banners, bars, sheet steps, question bubbles |
| `duration-page` | 200 ms | 150 ms fade | A page arriving, after `delay-page` |
| `duration-page-exit` | 100 ms | 90 ms | A page leaving |
| `delay-page` | 60 ms | 0 | The arriving page waits for the old one to mostly fade |
| `duration-story` | 480 ms | 150 ms fade | Wrapped turns |
| `duration-stage` | 700 ms | none | The total budget for a staged entrance |
| `spring-settle` | 320 ms, bounce 0 | 0 | Layout moves: reorder, drop, collapse, sheet height, the coach mark |
| `spring-morph` | 400 ms, bounce 0 | 150 ms fade | A card growing into its page |
| `spring-push` | 320 ms, bounce 0 | 150 ms fade | A row's page pushing in |
| `spring-turn` | 480 ms, bounce 0 | 150 ms fade | Wrapped cards and spreads |
| `easing-standard` | cubic-bezier(0.2, 0.7, 0.2, 1) | | Everything that appears or changes |
| `easing-exit` | cubic-bezier(0.4, 0, 1, 1) | | Things that leave |

Springs never bounce: an overshooting bar would show an amount that isn't real. With Reduce Motion on, anything that moves, scales or resizes becomes a short fade, small changes happen at once, and nothing loops. In debug builds, the launch argument `-MotionTimeScale 5` slows every motion five times.
