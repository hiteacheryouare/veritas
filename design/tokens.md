# Design tokens

`tokens.css` is the source of truth. Names are platform-neutral so they map one to one onto Swift (`Color.moneyAvailable`, `Font.figures`, `Spacing.s4`, `Motion.fast`). Drop the `--color-` style prefix and camelCase the rest.

Direction: Quiet native's layout and grouped lists, with drain bars, semantic color for every money state, and SF Pro Rounded figures. Colors come from Ryan's brand (`brand/colors.txt`, D80); the logo and its rules are in `design/brand/README.md`.

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
| `color-accent` | #23169c | #94a8fe | Ryan's primary. Selection, focus ring, primary buttons, links in lists, the tour's Next, Mo Nee's send button and example questions. Never money |
| `color-accent-fill` | #23169c | #94a8fe | Primary button fill. In dark mode it is light, so `color-text-on-accent` flips to dark |
| `color-accent-soft` | rgba(35, 22, 156, 0.1) | rgba(148, 168, 254, 0.14) | Selected and editing rows, question bubbles |
| `color-secondary` | #03698c | #56b8e3 | Ryan's secondary as text: links and secondary emphasis |
| `color-secondary-fill` | #007ea7 | #007ea7 | Ryan's secondary as a fill |
| `color-brand` | #23169c | #23169c | Ryan's primary in both appearances: Wrapped's cards. Its text is `color-text-on-brand` (#f5f5f7) |
| `color-money-available` | #036f46 | #56e39f | Good. Available amounts above the low threshold, on-track goals |
| `color-money-low` | #7d5c02 | #f5b700 | Warning. Envelope at or under 20% left, goals behind schedule |
| `color-money-overspent` | #c50125 | #ff8b86 | Bad. Overspent envelopes, losses, shortfalls |
| `color-money-income` | #03698c | #56b8e3 | Secondary. Income, To Budget, money received |
| `color-money-neutral` | #6c6c75 | #97979f | Zero amounts. 4.5:1 or better on every surface |
| `color-money-*-fill` | | | Bar, ring, dot and chart fills: Ryan's colors exactly, in both appearances. Available #56e39f, low #f5b700, overspent #db162f, income #007ea7 |
| `color-chart-expense` | #6e6e78 | #a5a5ae | Spending in charts. Spending is not a warning, so it is graphite |
| `color-chart-savings` | #56e39f | #56e39f | Savings in Net Profit and the Sankey (good) |
| `color-chart-debt-paydown` | #8b5e34 | #c99a6b | Debt paydown in Net Profit and the Sankey |
| `color-scenario-1..3` | blue, umber, teal | | What-if lines. Baseline uses `color-scenario-baseline` |
| `color-surface-*` | | | window, sidebar (glass), grouped, raised (sheets, popovers), field, paper, scrim |
| `color-text-*` | | | primary, secondary, tertiary, on-accent, paper. Tertiary is #6c6c75 / #97979f: every text token reaches 4.5:1 on every surface |
| `color-surface-selected` | rgba(20, 22, 30, 0.08) | rgba(240, 240, 250, 0.12) | The selected sidebar row. Neutral, so it works with any accent; its text is `color-text-primary` |

### Accent

The accent is Ryan's primary, #23169c (D80, which replaces D40's graphite). In dark mode it is #94a8fe: the same OKLCH hue, lightened until it reads at 4.5:1 on every dark surface, including rows tinted with `color-accent-soft`. It marks what can be acted on: selection, focus, primary buttons, links. Money keeps its own colors, so the accent never sits on an amount.

### Brand colors as text

Ryan's colors are used exactly for fills, bars, dots and chart marks. Where one is text or a small icon, it uses a shade of the same OKLCH hue, darker in light mode and lighter in dark mode, just far enough to reach 4.5:1 on every surface and selected row (Ryan's choice, D80). A brand color that already reaches it keeps its own value. `tools/brand/palette.py` derives the shades, and `ContrastTests` in VeritasDesignTests fails if any token pair drops below 4.5:1 for text or 3:1 for UI.

| Brand | Fill | Text, light | Text, dark |
|---|---|---|---|
| Primary #23169c | accent, brand | #23169c | #94a8fe |
| Secondary #007ea7 | secondary-fill, money-income-fill | #03698c | #56b8e3 |
| Bad #db162f | money-overspent-fill | #c50125 | #ff8b86 |
| Good #56e39f | money-available-fill, chart-savings | #036f46 | #56e39f |
| Warning #f5b700 | money-low-fill | #7d5c02 | #f5b700 |

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

A bar shows what is left: `available / (carried in + budgeted)`, clamped to 0–100%. A full bar is a full envelope. It is good (#56e39f) while more than 20% is left, warning (#f5b700) at 20% or under, empty at zero. An overspent envelope shows a bad (#db162f) hatched bar at full width with no fill. The hatch is the non-color signal.

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
