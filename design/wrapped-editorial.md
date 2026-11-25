# Wrapped, editorial

A proposal to turn Wrapped from nine story cards into a short year-in-review magazine: spreads with serif headlines, ranked tables, and charts annotated in place. Nothing here is built yet. Motion references point to `design/motion.md`.

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
- **Paper:** the page is `color-surface-paper` with `color-text-paper` ink in both appearances. In dark mode it sits on the dark window like a printed page on a desk, the same treatment Statements uses. A soft shadow (the current card's 24 pt radius, 18% black) lifts it off the window. Corners are square, as paper is.

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
| Cover | `year`, `first`, `last` (year to date when `last` isn't Dec 31), the open budget's name |
| The ledger | `total_in`, `total_out`, `net`, `prior_year` (changes and `from`), `shares.out_vs_in`, `shares.net` |
| Categories | `top_categories` (name, total, share, labels), `other_categories`, `shares.other_categories`, the `TopCategoryShare` fact; N6 optional |
| Groups | `top_groups`, `other_groups`, `shares.other_groups` |
| Who you paid | `top_payees` (name, total, count, share), `other_payees`, `shares.other_payees` |
| Paid down and saved | `debt_paydown`, `savings`, `shares.debt_paydown`, `shares.savings` |
| The shape of the year | `months`, `biggest_month`, `biggest_vs_typical`, `shares.biggest_month`; N2 for the typical-month rule |
| Quiet days | `no_spend` (days, evaluated days, first, last, longest, current), the `NoSpendShare` fact; N3 for the calendar |
| Goals reached | `completed_goal_ids` and goal names; N4 for reached months |
| Net worth | `net_worth_start`, `net_worth_end`, `net_worth_change`, `shares.net_worth_change`; N1 for the line |
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

| | **A. New York for display figures (recommended)** | B. SF Pro Rounded everywhere |
|---|---|---|
| Hero figures and figures inside headlines | New York, tabular lining | SF Pro Rounded |
| Tables and annotations | SF Pro Rounded | SF Pro Rounded |
| Effect | The page reads as one typeset piece; the serif figure is most of the editorial character | Consistent with the rest of the app; reads as an app screen with serif headlines |

## Color

Neutral until Ryan's palette exists. Accent positions are marked so the accent drops in later without a layout change.

| Use | Value |
|---|---|
| Paper | `color-surface-paper` |
| Ink | `color-text-paper` |
| Secondary ink | A new `color-text-paper-secondary`, about #5f5f67, at 4.5:1 or better on both paper values |
| Hairlines | Ink at 16% |
| Bars, lines | `color-chart-expense` (light value, since paper is light in both appearances) |
| Soft bars | `color-chart-expense-soft` |
| Refund bar | Unfilled, hatched in `color-chart-expense`, the same non-color signal as the overspent drain bar |

Accent positions:

- **ACCENT-1:** the 48 pt rule under the cover year.
- **ACCENT-2:** the highlighted mark in a chart: the biggest month's bar, the net worth line's end point. Until the accent exists, these are solid ink.
- **ACCENT-3:** the focus ring on contents entries, which is `color-focus-ring` today and follows whatever the app's focus color becomes.

Money-state colors (available, low, overspent, income) are not used in Wrapped. Nothing in a year review is in one of those states, and green spending bars would say the wrong thing.

## Charts and annotations

- **Direct labels, no legends.** A mark is named where it is.
- **Two annotations per chart at most.** Each is a short label and a figure, both from the report: "March  $8,120", "Refunds beat spending".
- **Leaders.** 0.5 pt hairline in secondary ink, an elbow with a horizontal run, a 6 pt gap from the mark. The text starts at the end of the run, left-aligned.
- **Placement.** Above and right of the mark if it fits, else above and left, else in the band above the plot. Annotations never cross marks or each other.
- **Axes.** No y-axis and no gridlines. A 0.5 pt baseline in ink at 40%. Month initials below. A reference line (the typical month) is dashed 0.5 pt with its label at the right end.
- **Missing values.** A month with no share reads "n/a" in its slot (W-21), in axis type.
- **Refund months.** Drawn below the baseline, hatched, never clamped (W-20).
- **Hide amounts.** Shapes stay identical, because bars are drawn from the core's shares in both modes. Amount annotations become share annotations. The y-scale never shows money.
- **VoiceOver.** Each chart has an `AXChartDescriptor` (audio graph) built from the same series, and its annotations are read as text after the headline.

## Voice

Plain declarative sentences in the past tense, second person only where it reads naturally. Numbers carry the meaning; adjectives don't.

Rules:

1. **Every figure comes from the report**, formatted by `MoneyFormat` and `WrappedWords`. No rounding beyond what they do, no "about", no "nearly".
2. **Comparisons only from the report:** prior-year changes, `biggest_vs_typical`, the two shares in `fun_facts`. Nothing against other people or averages (FC-8).
3. **No judgment words:** not "only", "just", "impressive", "great", "careful", "splurge".
4. **No growth or decline words** when the prior year is partial (`prior_year.from` set). The ledger shows FC-8's line instead: "No full prior year to compare. Your budget starts November 1, 2025."
5. **Net is "left over" or "came up short"**, never "saved". Savings is its own figure.
6. **No-spend copy** never says the user spent nothing. Bills, loan payments, transfers and refunds don't break a no-spend day.
7. **No filler, eyebrows or reassurance.** Not "What a year", "Let's look back", "Here's your", "Your money story", "Keep it up", "Don't worry", "at a glance", "insights", "journey".

### Examples

Amounts here are made up to show shape. The real text is generated by `WrappedStory`.

| Page | Headline | Dek |
|---|---|---|
| Cover | 2026 | Household budget. January 1 to November 25. |
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
- A no-spend day is a day with no discretionary spending. Bills, loan payments, transfers and refunds don't count against it.
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
  - Net worth: the headline is unchanged ("Net worth went up."), the hero figure becomes "+55.3%" with the dek "since the start of 2026", and the start and end rows are left out.
- **Unchanged:** names, dates, counts, streaks, the calendar, chart shapes.
- **VoiceOver** reads the redacted text and the redacted chart descriptions; no amount is spoken.

## Moving between spreads

Tied to `motion.md`.

- **Turn.** The outgoing spread fades and drifts 12 pt opposite to the direction of travel; the incoming spread fades in from 12 pt on the travel side. Opacity uses `story` (480 ms, `standard`); movement uses the `turn` spring, so a fast second press retargets instead of stacking. No page curl or 3D: it's heavy, skeuomorphic, and fights the precise feel.
- **First-view entrance.** The first time a spread is shown in a session, its parts arrive in order within the 700 ms `stage` budget: headline and dek fade in (0 ms), the hero figure fades in (80 ms), chart marks grow from the baseline with a 30 ms stagger capped at 12 marks (160 ms), annotations fade in after the marks settle. Figures never count up. Revisiting a spread shows it complete, the same full-then-quick budget as ArenaSwap's reveal.
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

R-WRAP-2 requires images. Every export renders on light paper, from the same views as the screen, at 2x (a spread is 2560 by 1600 px, a page 1280 by 1600 px).

| | **A. Menu of three (recommended)** | B. One PNG per spread | C. One contact sheet (today) |
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
- **Contrast.** Every text color reaches 4.5:1 on both paper values; annotation and axis text included.
- **Non-color signals.** The biggest month is marked by its annotation and a heavier bar, not only by ACCENT-2. Refund months are hatched.
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
|  ____  ACCENT-1, 48 pt rule           |  Where it went                    5   |
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
|          ####  ACCENT-2                                                       |
|   ##     ####          ##                           ##                        |
|   ##     ####   ##     ##    ##    ##     ##   ##   ##    ##                  |
|  - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - typical  $6,200  |
|   ##     ####   ##     ##    ##    ##     ##   ##   ##    ##   ##    n/a     |
|  -------------------------------------------------------------------------    |
|          /\/\                                                                 |
|          ----- Refunds beat spending                                          |
|    J  |   F   |  M  |  A  |  M  |  J  |  J  |  A  |  S  |  O  |  N  |  D     |
|                                                                   9  | 10     |
+-------------------------------------------------------------------------------+
```

The chart spans all twelve columns across the spine. February sits below the baseline, hatched. December reads "n/a" when the year to date ends in November. The typical-month rule needs N2.

### 7. Net worth

```
+---------------------------------------+---------------------------------------+
|                                       |                                       |
|  Net worth went up.                   |                            $111,830   |
|                                       |                       +----  Dec 31   |
|                                       |                   ___/  ACCENT-2      |
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

## Choices for Ryan

1. **Display figures.** **A New York** or B SF Pro Rounded.
2. **Paper.** **Always light paper, like Statements**, or paper that follows the appearance (dark paper in dark mode).
3. **Auto-advance.** **Drop it**, or keep a 6 s auto-advance with Space to pause.
4. **Export.** **A menu of three (spread PNG, all PNGs, PDF)**, B PNGs only, C one contact sheet as today.
5. **Contents page.** **Keep it**, or open straight on the ledger with the cover as a single page.
6. **Optional figures.** Ask Fred for N5 and N6 now, or only N1 to N4.
