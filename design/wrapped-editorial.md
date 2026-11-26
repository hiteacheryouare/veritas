# Wrapped, editorial

The design for turning Wrapped from nine story cards into a short year-in-review magazine: spreads with serif headlines, ranked tables, and charts annotated in place. Ryan's decisions are collected at the end. The build waits for the core figures N1 to N4. Motion references point to `design/motion.md`.

The voice is classy and plain. It reports the year; it doesn't cheer, judge or console. Every figure on every page comes from the core's `WrappedReport` (FC-8), or from a new figure listed at the end for Fred to spec. The app formats; it never computes a money figure of its own.

## What exists today

- `WrappedScreen` shows one 400 by 660 pt card at a time with arrow buttons, a year picker, Hide amounts and an image export.
- `WrappedStory` (VeritasKit) turns the report into finished text per card, with a redacted variant. A card holds only text, so a redacted card cannot contain an amount, and `WrappedRedactionWorstCaseTests` checks that. This guarantee is worth keeping exactly as it is.
- Cards: totals, categories, groups, payees, biggest month (with the only chart), debt and savings, no-spend days, goals, net worth.
- Every card uses the same layout: a rounded-heading title, an optional 64 pt figure, a detail line, rows. The card is graphite (`accentFill`), and the biggest month's bar uses `moneyAvailableFill`, a money-state green that means "available", not "spent".
- The export renders all cards three to a row in one PNG, always in light colors.
- Cards swap with no transition. The 220 ms cross-fade and 6 s auto-advance in `interaction.md` were never built.

## The format

A spread is two facing pages on a fixed canvas, like a printed magazine, scaled to fit the window.

- **Canvas:** 1280 by 800 pt per spread (16:10), two pages of 640 by 800.
- **Scaling:** the spread is laid out at canvas size and scaled as a whole to fit the content area, so the typography keeps its exact proportions at any window size. It never scales above 1.
- **Narrow windows:** below 1000 pt of content width, pages show one at a time in the same order, at 640 by 800.
- **Paper:** follows the appearance, light pages in light mode and dark pages in dark mode (see Color).

## The sequence

Eight spreads. A spread whose data is missing is skipped, and its partner page takes the full spread (see Reflow).

| # | Left page | Right page | Purpose |
|---|---|---|---|
| 1 | Cover | Contents | Sets the year and the period, and lets the reader jump anywhere |
| 2 | The ledger (full spread) | | What came in, what went out, what was left over, against last year |
| 3 | Where it went: categories | Where it went: groups | The top five categories and groups, with shares |
| 4 | Who you paid | Paid down and saved | Top payees by total and count; debt paydown and savings, kept apart from spending |
| 5 | The shape of the year (full spread) | | The monthly spending chart, with the biggest month and refund months annotated |
| 6 | Quiet days | Goals reached | No-spend days on a year calendar; goals reached, with the month each was reached |
| 7 | Net worth (full spread) | | Start, end and change, with the month-end line |
| 8 | Notes | Back cover | How each figure is counted, in plain words, and the period once more |

A year with no data (`has_data` false, W-13) shows only the cover, with the line "No data for 2026." in place of the period, and no contents.

### Reflow

- No groups (all spending uncategorized or a single group): categories take the full spread, with the table on the left page and the share bar chart on the right.
- No debt paydown and no savings: payees take the full spread.
- No goals reached: quiet days take the full spread, with the calendar across both pages.
- No payees and no debt or savings: spread 4 is skipped.
- Contents lists only the spreads that exist, with their page numbers.

## Page sources

Which report fields each page uses. New figures are marked N1 to N6 and specified at the end.

| Page | Fields |
|---|---|
| Cover | `year`, `first`, `last` (year to date when `last` isn't Dec 31), the budget's name, which FC-8's copy rule (#111) lists as an allowed field |
| The ledger | `total_in`, `total_out`, `net`, `prior_year` (changes and `from`), `shares.out_vs_in`, `shares.net` |
| Categories | `top_categories` (name, total, share, labels), `other_categories`, `shares.other_categories`, the `TopCategoryShare` fact; N6 optional |
| Groups | `top_groups`, `other_groups`, `shares.other_groups` |
| Who you paid | `top_payees` (name, total, count, share), `other_payees`, `shares.other_payees` |
| Paid down and saved | `debt_paydown`, `savings`, `shares.debt_paydown`, `shares.savings` |
| The shape of the year | `months`, `biggest_month`, `biggest_vs_typical`, `shares.biggest_month`; N2 for the typical-month rule |
| Quiet days | `no_spend` (days, evaluated days, first, last, longest, current), the `NoSpendShare` fact; N3 for the calendar |
| Goals reached | `completed_goal_ids` and goal names; N4 for reached months |
| Net worth | `net_worth_start`, `net_worth_end`, `net_worth_change`, `shares.net_worth_change`; N1 for the line and its redacted shares |
| Notes | FC-8's definitions, as copy Fred reviews; `first`, `last` |

## Layout grid

Per page, at canvas size:

- Margins: 72 pt top, 64 pt bottom, 64 pt outer, 40 pt at the spine. Live area 536 by 664 pt.
- Six columns of 76 pt with 16 pt gutters. A full spread is twelve columns plus the 80 pt spine gap.
- 8 pt baseline grid. Every leading in the type table is a multiple of 4, and block spacing is a multiple of 8, matching `space-*`.
- Headlines start on the first baseline of the live area. A headline and its dek take columns 1 to 5 of the left page (column 6 stays empty, so lines stay short).
- Folios sit in the bottom outer corner, 24 pt below the live area.

## Typography

New York carries the voice; SF Pro carries the data. All sizes are in points at canvas scale.

| Role | Face | Size / leading | Weight | Notes |
|---|---|---|---|---|
| Cover year | New York | 168 / 160 | Regular | Tabular lining figures, tracking -2% |
| Headline | New York | 40 / 48 | Semibold | Sentence case, ends with a period, up to three lines |
| Dek | New York | 20 / 28 | Regular | Secondary ink, up to three lines |
| Hero figure | New York | 96 / 96 | Medium | Tabular lining figures. One per spread at most, and only when the headline doesn't already carry the figure |
| Page heading | New York | 24 / 32 | Semibold | "Groups", "Paid down and saved". A heading, never a label above a heading |
| Contents entry | New York | 17 / 32 | Regular | Page number right-aligned in SF Pro Rounded 13 |
| Table label | SF Pro Text | 13 / 20 | Regular | |
| Table figure | SF Pro Rounded | 13 / 20 | Medium | Tabular, right-aligned, as `Font.figures` |
| Table total | SF Pro Rounded | 13 / 20 | Semibold | Above a hairline |
| Annotation | SF Pro Text | 11 / 16 | Medium | Secondary ink, at the end of a leader |
| Axis | SF Pro Text | 11 / 16 | Regular | Tertiary ink, month initials only |
| Notes body | SF Pro Text | 13 / 20 | Regular | |
| Folio | SF Pro Rounded | 11 / 16 | Regular | Tertiary ink, page number only |

There are no eyebrow labels, kickers or running heads. Each page opens on its headline.

### Display figures: New York or SF Pro Rounded

`tokens.md` puts every amount in SF Pro Rounded. Wrapped is the one place an exception could earn its keep.

| | **A. New York for display figures (chosen)** | B. SF Pro Rounded everywhere |
|---|---|---|
| Hero figures and figures inside headlines | New York, tabular lining | SF Pro Rounded |
| Tables and annotations | SF Pro Rounded | SF Pro Rounded |
| Effect | The page reads as one typeset piece; the serif figure is most of the editorial character | Consistent with the rest of the app; reads as an app screen with serif headlines |

## Color

Paper follows the appearance: light pages in light mode, dark pages in dark mode. The brand accent marks at most one thing per page. Colors are named by token; their values live in `design/tokens.css`, where `ws/brand` puts the brand palette.

### Page colors

New Wrapped tokens, each defined as an existing token so there is no new hex value to keep in step.

| Token | Light | Dark |
|---|---|---|
| `color-wrapped-paper` | `color-surface-paper` | `color-surface-grouped` |
| `color-wrapped-ink` | `color-text-paper` | `color-text-primary` |
| `color-wrapped-ink-secondary` | `color-text-secondary` | `color-text-secondary` |
| `color-wrapped-rule` | `color-wrapped-ink` at 16% | `color-wrapped-ink` at 20% |
| `color-wrapped-edge` | none (a shadow lifts the page) | `color-edge-grouped`, 1 pt |

- **Light pages** sit on `color-surface-window` with a soft shadow (24 pt radius, 18% black). Corners are square, as paper is.
- **Dark pages** sit on the dark window, where a shadow barely shows, so the page gets the 1 pt edge plus a deeper shadow (24 pt radius, 40% black). The page is lighter than the window, so it still reads as a sheet on a desk.
- Every text token reaches 4.5:1 on its page in both appearances.

### Marks

| Use | Token |
|---|---|
| Bars, lines | `color-chart-expense` (each appearance's own value) |
| Soft bars | `color-chart-expense-soft` |
| Refund bar | Unfilled, hatched in `color-chart-expense`, the same non-color signal as the overspent drain bar |
| Cover rule | `color-accent` |
| The one highlighted mark per chart (the biggest month's bar, the net worth line's end point, the longest no-spend run) | `color-accent` |
| No-spend days on the calendar | `color-accent-secondary` at 35% |
| Focus ring on contents entries | `color-focus-ring` |

`color-accent` is the brand primary, with the derived dark-mode shade in dark mode. `color-accent-secondary` is the brand secondary. Both names follow whatever `ws/brand` lands. Each accent mark must reach 3:1 against its page in both appearances (WCAG 1.4.11); check this once `ws/brand` merges, and fall back to the ink color for any mark that misses. Text is never set in an accent color.

The good, bad and warning colors, and the money-state colors (available, low, overspent, income), are not used in Wrapped. Nothing in a year review is in one of those states, and green spending bars would say the wrong thing.

## Charts and annotations

- **Direct labels, no legends.** A mark is named where it is.
- **Two annotations per chart at most.** Each is a short label and a figure, both from the report: "March  $8,120", "Refunds beat spending".
- **Leaders.** 0.5 pt hairline in secondary ink, an elbow with a horizontal run, a 6 pt gap from the mark. The text starts at the end of the run, left-aligned.
- **Placement.** Above and right of the mark if it fits, else above and left, else in the band above the plot. Annotations never cross marks or each other.
- **Axes.** No y-axis and no gridlines. A 0.5 pt baseline in ink at 40%. Month initials below. A reference line (the typical month) is dashed 0.5 pt with its label at the right end.
- **Missing values.** "n/a" has one meaning: the core gave no share because `out <= 0` (W-21). Those slots read "n/a" in axis type. A month after the period's last day isn't in the series at all. Its slot stays empty, with only its month initial in tertiary ink, the same "not yet" treatment the calendar gives days that aren't evaluated.
- **Refund months.** Drawn below the baseline, hatched, never clamped (W-20).
- **Hide amounts.** Shapes stay identical, because bars are drawn from the core's shares in both modes. Amount annotations become share annotations. The y-scale never shows money. The one exception is the net worth line when opening net worth is 0 or less: N1 gives no redacted shares then, so the redacted page has no line (see Hide amounts).
- **VoiceOver.** Each chart has an `AXChartDescriptor` (audio graph) built from the same series, and its annotations are read as text after the headline.

## Voice

Plain declarative sentences in the past tense, second person only where it reads naturally. Numbers carry the meaning; adjectives don't.

Rules:

1. **Every figure comes from the report**, formatted by `MoneyFormat` and `WrappedWords`. No rounding beyond what they do, no "about", no "nearly". Page numbers and folios are layout numbers, not figures, and the budget's name is a field the copy rule allows (#111).
2. **Comparisons only from the report:** prior-year changes, `biggest_vs_typical`, the two shares in `fun_facts`. Nothing against other people or averages (FC-8).
3. **No judgment words:** not "only", "just", "impressive", "great", "careful", "splurge".
4. **No growth or decline words** when the prior year is partial (`prior_year.from` set). The ledger shows FC-8's line instead: "No full prior year to compare. Your budget starts November 1, 2025."
5. **Net is "left over" or "came up short"**, never "saved". Savings is its own figure.
6. **No-spend copy** never says the user spent nothing. Bills, loan payments, refunds, and transfers, card payments and savings moves between the user's own accounts don't break a no-spend day. A transfer booked as spending (CLS-4 row 8) does (FC-8 as revised in #111; Fred to confirm the words).
7. **No filler, eyebrows or reassurance.** Not "What a year", "Let's look back", "Here's your", "Your money story", "Keep it up", "Don't worry", "at a glance", "insights", "journey".

### Examples

Amounts here are made up to show shape. The real text is generated by `WrappedStory`.

| Page | Headline | Dek |
|---|---|---|
| Cover | 2026 | Household budget. January 1 to November 25. (The budget's name is the copy-rule field from #111.) |
| The ledger | $84,210 came in. $71,380 went out. | That left $12,830, 21.4% more than in 2025. |
| The ledger, short year | $52,000 came in. $54,140 went out. | That came up $2,140 short. |
| The ledger, partial prior year | $84,210 came in. $71,380 went out. | That left $12,830. No full prior year to compare. Your budget starts November 1, 2025. |
| Categories | Groceries took the largest share. | $9,412, 18.3% of categorized spending. |
| Groups | Groups | Everyday was the biggest group, at $21,060. |
| Who you paid | You paid Kroger the most. | $6,980 across 112 payments. |
| Paid down and saved | Paid down and saved | $6,000 to debt and $4,800 to savings. Neither is counted in what went out. |
| The shape of the year | March was the biggest month for spending. | $8,120, 31% more than your typical month. |
| Quiet days | 142 no-spend days. | 39.0% of the 364 days from January 1 to December 30. The longest run was 9 days, from April 4. |
| Goals reached | You reached 3 goals. | Emergency fund in March, New laptop in July, Trip to Lisbon in October. |
| Net worth | Net worth went up. | None. The hero figure is +$39,830, with Start of 2026 $72,000 and End of 2026 $111,830 in the table beside it. |
| Notes | How these figures are counted | See Notes below. |

Prior-year phrasing follows FC-8 exactly: "+21.4% vs 2025" in tables, "21.4% more than in 2025" in prose, "New this year" when last year's figure was 0, and "vs this point in 2025" for a year to date.

### Notes page

Short, factual sentences, reviewed by Fred, drawn from FC-8:

- "Went out" is spending after refunds. Debt paydown and savings are counted separately.
- Categories and groups rank by spending after refunds. Uncategorized spending isn't ranked.
- Payees rank by what was paid to them. Transfers aren't payees.
- A no-spend day is a day with no discretionary spending. Bills, loan payments, refunds, and transfers, card payments and savings moves between your own accounts don't count against it. A transfer recorded as spending does.
- Net worth at the start is the balance at the end of the day before January 1.
- Percentages compare the same dates a year earlier.

## Hide amounts

Hide amounts (R-WRAP-2) keeps every layout identical and swaps each amount for the core's share.

- **Source.** Redacted text comes only from `WrappedStory.cards(redacted: true)`, extended to spreads. A redacted spread holds no amount, and the worst-case redaction test covers every new figure and annotation.
- **No reflow.** Each figure slot reserves the width of the longer of its two forms, so toggling never moves the page.
- **Motion.** Toggling cross-fades only the figures, over `Motion.fast`. Instant under Reduce Motion.
- **Copy.** Headlines that contain an amount have a redacted form:
  - Ledger: "84.8% of what came in went out." / "15.2% was left over."
  - Categories: "Groceries took the largest share." / "18.3% of categorized spending."
  - Shape of the year: "March was the biggest month for spending." / "12.4% of the year's spending, 31% more than your typical month."
  - Net worth: the headline is unchanged ("Net worth went up."), the hero figure becomes "+55.3%" with the dek "since the start of 2026", and the start and end rows are left out. When opening net worth is 0 or less, there is no `shares.net_worth_change` and no N1 shares, so the hero reads "n/a", as other share slots do, and the line is left out. The headline still says which way net worth went, from the sign of `net_worth_change`.
- **Unchanged:** names, dates, counts, streaks, the calendar, chart shapes.
- **VoiceOver** reads the redacted text and the redacted chart descriptions; no amount is spoken.

## Moving between spreads

Tied to `motion.md`.

- **Turn.** The outgoing spread fades and drifts 12 pt opposite to the direction of travel; the incoming spread fades in from 12 pt on the travel side. Opacity uses `story` (480 ms, `standard`); movement uses the `turn` spring, so a fast second press retargets instead of stacking. No page curl or 3D: it's heavy, skeuomorphic, and fights the precise feel.
- **First-view entrance.** The first time a spread is shown in a session, its parts arrive in order within the 700 ms `stage` budget: headline and dek fade in (0 ms), the hero figure fades in (80 ms), chart marks fade in at their full height with a 30 ms stagger capped at 12 marks (160 ms), annotations fade in after the marks. Marks never grow from the baseline: a bar partway up is the chart form of counting up, an amount that isn't real. Figures never count up either. Revisiting a spread shows it complete, the same full-then-quick budget as ArenaSwap's reveal.
- **No auto-advance.** A magazine is paged by the reader. This replaces the 6 s auto-advance in `interaction.md`.
- **Contents.** Choosing an entry turns straight to that spread (one turn, not one per page). Optionally, the entry's title morphs into the spread's headline (`motion.md`, continuity moment 5).
- **Year change.** Cross-fades to the new year's cover over `story`.
- **Reduce Motion.** A turn is a cross-fade over `fast` with no drift. No first-view entrance; spreads appear complete.

### Keys

| Keys | Action |
|---|---|
| → or Space | Next spread |
| ← or Shift-Space | Previous spread |
| Home / End | Cover / back cover |
| ⌘[ | Contents |
| Return on a contents entry | Turn to that spread |

## Export

R-WRAP-2 requires images. Every export renders from the same views as the screen, at 2x (a spread is 2560 by 1600 px, a page 1280 by 1600 px). The save panel has a Paper popup, Light or Dark, set to the appearance on screen, so a dark-mode reader can still send a light page.

| | **A. Menu of three (chosen)** | B. One PNG per spread | C. One contact sheet (today) |
|---|---|---|---|
| Items | "This spread as image…", "All spreads as images…" (a folder of numbered PNGs), "All spreads as PDF…" | Folder of PNGs only | All spreads tiled into one PNG |
| Hide amounts | Each item has a "without amounts" twin, as today | Same | Same |

The PDF has real, selectable text, which makes it searchable and readable by VoiceOver, unlike a PNG. File names come from `WrappedModel.imageFileName(redacted:)`, extended with a spread number.

## Accessibility

- **Reading order** per page: headline, dek, hero figure, table rows or chart description, annotations, folio skipped. Each spread is one container; each page a group within it.
- **Headings.** Headlines and page headings carry the header trait, so the VoiceOver rotor can jump between them.
- **Tables** read each row as a sentence: "Groceries, 9,412 dollars, 18.3% of categorized spending."
- **Charts** expose an audio graph (`AXChartDescriptor`) and a one-sentence summary built from the same report text.
- **Calendar** reads as a summary ("142 no-spend days, longest run 9 days from April 4"), with each month navigable as a group listing its no-spend dates.
- **Contrast.** Every text color reaches 4.5:1 on both pages, light and dark, annotation and axis text included. Accent marks reach 3:1 or fall back to ink.
- **Non-color signals.** The biggest month is marked by its annotation and a heavier bar, not only by its accent color. Refund months are hatched.
- **Zoom.** The spread scales with the window, and macOS zoom works as on any view. In single-page mode at 640 pt, nothing is smaller than 11 pt.
- **Keyboard.** Everything works from the keys above; contents entries are focusable buttons with the focus ring.
- **Reduce Motion** as described in Moving between spreads.

## Sketches

At canvas scale, not to exact proportion. `|` in the middle is the spine.

### 1. Cover and contents

```
+---------------------------------------+---------------------------------------+
|                                       |                                       |
|  2026                                 |  Contents                             |
|  New York 168, tabular                |                                       |
|                                       |  The ledger                       3   |
|  ____  48 pt rule, color-accent       |  Where it went                    5   |
|                                       |  Who you paid                     7   |
|  Household budget                     |  Paid down and saved              8   |
|  January 1 to November 25             |  The shape of the year            9   |
|  New York 20, secondary ink           |  Quiet days                      11   |
|                                       |  Goals reached                   12   |
|                                       |  Net worth                       13   |
|                                       |  Notes                           15   |
|                                       |                                       |
|                                       |  New York 17 entries, page numbers in |
|                                       |  SF Pro Rounded 13, each a button     |
|                                       |                                       |
|                                       |                                       |
|                                   1   |   2                                   |
+---------------------------------------+---------------------------------------+
```

### 2. The ledger

```
+---------------------------------------+---------------------------------------+
|                                       |                                       |
|  $84,210 came in.                     |                                       |
|  $71,380 went out.                    |  Came in            $84,210  +6.2%    |
|  New York 40/48 semibold              |  ---------------------------------    |
|                                       |  Went out           $71,380  +3.9%    |
|  That left $12,830, 21.4% more        |  ---------------------------------    |
|  than in 2025.                        |  Left over          $12,830 +21.4%    |
|  New York 20/28, secondary ink        |                     vs 2025           |
|                                       |                                       |
|                                       |  |##############################|... |
|                                       |  went out 84.8%            left 15.2% |
|                                       |  one bar: shares.out_vs_in, net       |
|                                       |                                       |
|                                       |  Debt paydown and savings aren't in   |
|                                       |  "went out". See page 8.              |
|                                   3   |   4                                   |
+---------------------------------------+---------------------------------------+
```

### 5. The shape of the year

```
+-------------------------------------------------------------------------------+
|  March was the biggest month                                                  |
|  for spending.                                                                |
|  $8,120, 31% more than your typical month.                                    |
|                                                                               |
|                 March  $8,120                                                 |
|            +----                                                              |
|          ####  color-accent                                                   |
|   ##     ####          ##                           ##                        |
|   ##     ####   ##     ##    ##    ##     ##   ##   ##    ##                  |
|  - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - typical  $6,200  |
|   ##     ####   ##     ##    ##    ##     ##   ##   ##    ##   ##            |
|  -------------------------------------------------------------------------    |
|          /\/\                                                                 |
|          ----- Refunds beat spending                                          |
|    J  |   F   |  M  |  A  |  M  |  J  |  J  |  A  |  S  |  O  |  N  |  D     |
|                                                                   9  | 10     |
+-------------------------------------------------------------------------------+
```

The chart spans all twelve columns across the spine. February sits below the baseline, hatched. For a year to date that ends in November, December's slot is empty, with only its initial: it isn't in the series. The typical-month rule needs N2.

### 7. Net worth

```
+---------------------------------------+---------------------------------------+
|                                       |                                       |
|  Net worth went up.                   |                            $111,830   |
|                                       |                       +----  Dec 31   |
|                                       |                ___/  color-accent   |
|                                       |              ____/                    |
|                                       |   ___    ___/                         |
|                                       |  /   \__/                             |
|  +$39,830                             |  |     +---- lowest month end         |
|  New York 96 hero figure              |  |          February  $70,200         |
|                                       |  ---------------------------------    |
|  Start of 2026           $72,000      |  J F M A M J J A S O N D              |
|  End of 2026            $111,830      |                                       |
|  SF Pro 13 table, hairline between    |  month-end line needs N1              |
|                                       |                                       |
|                                  13   |  14                                   |
+---------------------------------------+---------------------------------------+
```

## New figures for the core

For Fred to spec in FC-8. Each page works without its new figure, so Wrapped can ship before all of them land: the shape of the year drops the typical rule, quiet days show the streak figures without the calendar, goals list names without months, and net worth shows start and end without the line.

| # | Figure | Used on | Definition to settle | Redacted form |
|---|---|---|---|---|
| N1 | Month-end net worth series | Net worth | `NW` at the end of each month in the period (ST-2), today for the current month. Plus the lowest and highest month-end, ties to the earliest, so the app doesn't rank | `round_half_away((NW(m) - NW(start)) * 1000 / NW(start))` tenths per month; none when `NW(start) <= 0` |
| N2 | Typical month | The shape of the year | The median month `biggest_vs_typical` already uses, as an amount | `round_half_away(median * 1000 / out)` tenths; none when `out <= 0` |
| N3 | No-spend dates | Quiet days | Every no-spend date in the evaluated range, or a per-day flag over it, so the calendar can be drawn | Same; dates are not amounts |
| N4 | Reached month per goal | Goals reached | `(goal id, reached_month)` for each completed goal, from the stored `reached_month` (DEBT-8.2) | Same |
| N5 | Monthly income series (optional) | The ledger, if it gains an in-and-out by month chart | `income_total` per month of the period, like the spending series | Shares of `total_in` |
| N6 | Prior-year change per top category (optional) | Categories, as an annotation | Basis points per top category vs the same range a year earlier, under the same partial-prior-year and `prev = 0` rules as the totals | Already a percentage |

## Decisions

Ryan's picks, November 2026:

1. **Display figures:** New York (A).
2. **Paper:** follows the appearance, so dark mode gets dark pages. Both are specified under Color.
3. **Auto-advance:** none. The reader pages it.
4. **Export:** this spread as PNG, all spreads as PNGs, or the issue as a PDF (A), each with a "without amounts" twin.
5. **Contents page:** kept.
6. **Core figures:** N1 to N4 now, N5 and N6 later.
