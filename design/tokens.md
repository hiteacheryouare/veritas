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
| `color-accent` | #23169c | #98acff | Ryan's primary. Selection, focus ring, primary buttons, links in lists, the tour's Next, Mo Nee's send button and example questions. Never money |
| `color-accent-fill` | #23169c | #98acff | Primary button fill. In dark mode it is light, so `color-text-on-accent` flips to dark |
| `color-accent-soft` | rgba(35, 22, 156, 0.1) | rgba(152, 172, 255, 0.14) | Selected and editing rows, question bubbles |
| `color-brand-secondary` | #00688b | #59bbe6 | Ryan's secondary as text: links and secondary emphasis |
| `color-brand-secondary-fill` | #007ea7 | #007ea7 | Ryan's secondary as a fill |
| `color-brand` | #23169c | #23169c | Ryan's primary in both appearances: Wrapped's cards. Its text is `color-text-on-brand` (#f5f5f7) |
| `color-money-available` | #036f46 | #56e39f | Good. Available amounts above the low threshold, on-track goals |
| `color-money-low` | #7c5b02 | #f5b700 | Warning. Envelope at or under 20% left, goals behind schedule |
| `color-money-overspent` | #c30225 | #ff908a | Bad. Overspent envelopes, losses, shortfalls |
| `color-money-income` | #00688b | #59bbe6 | Secondary. Income, To Budget, money received |
| `color-money-neutral` | #5f5f68 | #afafb7 | Zero amounts. 4.5:1 or better on every surface, selected and hovered rows included |
| `color-money-*-fill` | | | Bar, ring, dot and chart fills: Ryan's colors exactly, in both appearances. Available #56e39f, low #f5b700, overspent #db162f, income #007ea7 |
| `color-chart-expense` | #6e6e78 | #a5a5ae | Spending in charts. Spending is not a warning, so it is graphite |
| `color-chart-savings` | #56e39f | #56e39f | Savings in Net Profit and the Sankey (good) |
| `color-chart-debt-paydown` | #8b5e34 | #c99a6b | Debt paydown in Net Profit and the Sankey |
| `color-category-1..8` | | | Chart categories: custom reports, dashboard cards, Sankey groups. See Categories |
| `color-brand-deep` | #1b107e | #120a4e | The splash's gradient runs from `color-brand` down to this |
| `color-text-on-brand-secondary` | #cfcbf2 | #cfcbf2 | The splash's sentence, under `color-text-on-brand` |
| `color-scenario-1..3` | blue, umber, teal | | What-if lines. Baseline uses `color-scenario-baseline` |
| `color-surface-*` | | | window, sidebar (glass), grouped, raised (sheets, popovers), field, paper, scrim |
| `color-text-*` | | | primary, secondary, tertiary, on-accent, paper. Secondary is #5d5d66 / #b0b0b9 and tertiary #5f5f68 / #afafb7: every text token reaches 4.5:1 on every surface, including rows tinted by selection or hover |
| `color-surface-selected` | rgba(20, 22, 30, 0.08) | rgba(240, 240, 250, 0.12) | The selected sidebar row. Neutral, so it works with any accent; its text is `color-text-primary` |

### Accent

The accent is Ryan's primary, #23169c (D80, which replaces D40's graphite). In dark mode it is #98acff: the same OKLCH hue, lightened until it reads at 4.5:1 on every dark surface, including rows tinted with `color-accent-soft`. It marks what can be acted on: selection, focus, primary buttons, links. Money keeps its own colors, so the accent never sits on an amount.

### Brand colors as text

Ryan's colors are used exactly for fills, bars, dots and chart marks. Where one is text or a small icon, it uses a shade of the same OKLCH hue, darker in light mode and lighter in dark mode (Ryan's choice, D80). A brand color that already reads keeps its own value. `tools/brand/palette.py` derives the shades, and the greys, to 4.65:1, a little over the 4.5:1 they must reach.

| Brand | Fill | Text, light | Text, dark |
|---|---|---|---|
| Primary #23169c | accent, brand | #23169c | #98acff |
| Secondary #007ea7 | brand-secondary-fill, money-income-fill | #00688b | #59bbe6 |
| Bad #db162f | money-overspent-fill | #c30225 | #ff908a |
| Good #56e39f | money-available-fill, chart-savings | #036f46 | #56e39f |
| Warning #f5b700 | money-low-fill | #7c5b02 | #f5b700 |

### What the contrast test checks

`ContrastTests` in VeritasDesignTests fails if any of these drops below its threshold.

- **Splash, 4.5:1:** text-on-brand and text-on-brand-secondary on brand and on brand-deep, and text-on-brand on a splash button's fill and its pressed fill over each.
- **4.5:1, in both appearances:** every text token (primary, secondary, tertiary, accent, brand-secondary) and every money text token (available, low, overspent, income, neutral) on the window, grouped, raised and field surfaces. The same holds on each of those surfaces tinted by selection (`color-accent-soft`) or hover, on the sidebar and its selected row, and on RuleApplySheet's reconciled-warning row (money-low at 8%). Also text-on-accent on accent-fill, text-on-brand on brand, and the statement's notice on paper.
- **3:1, in both appearances:** the focus ring and accent fill on the four surfaces, and every brand fill's chart edge and chart mark on the window, grouped and raised surfaces, where charts sit (dashboard cards draw on raised).
- **7:1:** primary text on the selected sidebar row.

The fills themselves are not 3:1 everywhere. In light mode the mint is 1.6:1 and the yellow 1.8:1; in dark mode the red is about 3:1 and the secondary 2.9:1 on raised. So:

- **Chart marks that take a stroke** keep the fill and draw a 1 pt edge in the fill's text shade (`ColorToken.chartEdge`): Sankey nodes, the calendar card's bars, the money in and out bars, and legend dots.
- **Chart marks that can't** (lines, and Swift Charts series) use `ColorToken.chartMark`: the fill where it reaches 3:1, else the edge shade.
- **Drain bars and goal rings are exempt.** The amount beside them, with its icon and word, carries the state, so the bar is decoration and keeps the plain fill.

### Categories

Charts that color by category (custom reports, dashboard cards, the dashboard Sankey, and the spending groups on the Sankey screen) use eight hues from Ryan's brand, in this order. Actual's chart colors 1 to 9 map onto them in turn.

| # | Light | Dark | Hue |
|---|---|---|---|
| 1 | #23169c | #7285db | Indigo (Ryan's primary) |
| 2 | #007ea7 | #86d7fd | Cerulean (Ryan's secondary) |
| 3 | #9c76e3 | #af92eb | Orchid |
| 4 | #b87c58 | #e0a17c | Copper |
| 5 | #6b106c | #b66eb4 | Plum |
| 6 | #03544b | #70b3a8 | Pine |
| 7 | #4e6376 | #aac1d6 | Slate |
| 8 | #707102 | #909235 | Olive |

- **Not money:** no category sits within 26° of bad's OKLCH hue, 20° of warning's or 22° of good's. Good, warning and bad keep their meaning, and only a money state uses them. On the Sankey screen, the spending groups skip cerulean and copper, because income is cerulean and debt paydown is brown.
- **Readable:** every category reaches 3:1 on the window, grouped and raised surfaces in its own appearance, so its marks need no edge.
- **Distinct:** in OKLab ×100, every pair stays at least 7 apart in light mode and 5 apart in dark mode. That holds under normal vision and under full protanopia, deuteranopia and tritanopia (Machado, Oliveira and Fernandes 2009). The legend and tooltips name every series, so color is never the only cue.
- **Dark mode:** each category keeps its light hue, with chroma capped at 0.13 so nothing glows.
- **Checks:** `tools/brand/chart-palette.py` chose the palette and prints these figures. `CategoryPaletteTests` and `ContrastTests` fail if any of them slips.

### Balance graphs

The account page's balance graph is green above zero and red below. It changes color exactly where the balance crosses zero. The line takes each fill's chart mark (`money-available` and `money-overspent`, at 3:1). The area between the line and zero keeps the fills, faint.

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
| `duration-mint-in` | 1200 ms | 150 ms fade | The splash's coin mint-in, once per launch. One even clock: the rings draw (0–45%), the $ fades up (30–60%), the wordmark settles (42–80%), the buttons rise in (66–100%), each eased with `easing-standard`. Then nothing moves |
| `spring-settle` | 320 ms, bounce 0 | 0 | Layout moves: reorder, drop, collapse, sheet height, the coach mark |
| `spring-morph` | 400 ms, bounce 0 | 150 ms fade | A card growing into its page |
| `spring-push` | 320 ms, bounce 0 | 150 ms fade | A row's page pushing in |
| `spring-turn` | 480 ms, bounce 0 | 150 ms fade | Wrapped cards and spreads |
| `easing-standard` | cubic-bezier(0.2, 0.7, 0.2, 1) | | Everything that appears or changes |
| `easing-exit` | cubic-bezier(0.4, 0, 1, 1) | | Things that leave |

Springs never bounce: an overshooting bar would show an amount that isn't real. With Reduce Motion on, anything that moves, scales or resizes becomes a short fade, small changes happen at once, and nothing loops. In debug builds, the launch argument `-MotionTimeScale 5` slows every motion five times.
