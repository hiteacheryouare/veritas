# Forecasting, suggestions, goal ETA and Wrapped (R-FC-1..4, R-WRAP-1/2)

Status: draft for Gate 0. Owner: Fred. Builds on classification.md (CLS-n), statements.md (ST-n) and debt.md (DEBT-n). Amounts are integer cents. No floating point anywhere: every division below names its rounding rule.

Forecasts are estimates. Every screen that shows one must show the method name, its inputs and the range (FC-4). What-if scenarios (R-FC-2) overlay the baseline in memory and never write to real tables.

## FC-0 Constants and rounding

| Name | Value | Notes |
|---|---|---|
| `WINDOW` | 6 complete months | Trailing median window |
| `MIN_HISTORY` | 1 month with activity | Below `WINDOW`, use what exists and say how many months |
| `SEASON_MIN` | 13 complete months | Seasonality turns on at this history length |
| `BAND_LOW`, `BAND_HIGH` | P10, P90 | An 80% band |
| `BAND_MIN_RESIDUALS` | 6 | Fewer residuals use the fallback (FC-4.3) |
| `INDEX_MIN`, `INDEX_MAX` | 5000, 20000 basis points | Seasonal index clamp (0.50 to 2.00) |
| `INDEX_ACTIVE_MIN` | 6 of the last 12 months with activity | Below this, no seasonal index for the category |

`WINDOW`, the index clamp and `INDEX_ACTIVE_MIN` are starting values. The backtest (FC-7) may motivate a change, which requires editing this table. Nobody tunes them in code.

Rounding rules used in this file:

- **round_half_up(a / b)** for non-negative `a`: `(2a + b) / (2b)` with integer division.
- **round_half_away(a / b)** for signed `a`: round half up on `|a|`, then restore the sign.
- A "complete month" is a calendar month that has ended. The current month is partial and is never in a history window.

## FC-1 Baseline per category (R-FC-1)

Applies to each non-hidden `expense`-kind category (a spending baseline) and each `income`-kind category not fed by an income schedule (an income baseline). Debt, savings and transfers are not baselined here (FC-3).

**Monthly series.** For category `c` and complete month `m`: `x(c,m) = -sum(amount)` over rows of kind `expense` in `c` in `m` whose `schedule` column is empty (bill-linked rows are known flows, FC-2). For an income category, `x(c,m) = sum(amount)` of kind `income` rows not bill-linked. Refunds reduce `x`. `x` may be negative.

**Window.** The last `WINDOW` complete months, but never months before the category's first row (a category created last month has one month of history, not six). A month after the first row with no rows counts as 0.

**Level.** `L = median` of the window values. Odd count: the middle value. Even count: `round_half_up((a + b) / 2)` of the two middle values for non-negative sums, `round_half_away` if the sum is negative.

**Zero history.** No rows ever: baseline 0, label "No history yet". No suggestion is made (FC-5).

**Seasonality (turns on at `SEASON_MIN` = 13 complete months of history for the category):**

1. Take the latest 12 complete months. Each calendar month appears once. `sum12 = sum of x`. If `sum12 <= 0` or fewer than `INDEX_ACTIVE_MIN` of these months have `x != 0`, the category gets no seasonal index (index = 10000).
2. Raw index: `raw(m) = round_half_up(x(m) * 120000 / sum12)` in basis points (10000 = typical month). Negative `x` rounds as `max(0, x)` first.
3. Shrinkage: with 13 to 24 complete months: `index(m) = 10000 + round_half_away((raw(m) - 10000) / 2)`. With 25 or more: `raw` is the average of the latest and the previous 12-month windows' raw indices, each normalized by its own sum (`round_half_up` of the sum divided by 2), and no shrinkage.
4. Clamp `index` to `[INDEX_MIN, INDEX_MAX]`. The 12 indices need not sum to exactly 120000 because of rounding. That is expected.
5. Forecast for target month `t` of calendar month `m`: de-seasonalize each window value, `a(k) = round_half_up(x(k) * 10000 / index(month of k))`, take the median `L'` of the `a(k)`, and forecast `round_half_up(L' * index(m) / 10000)`. Without a seasonal index, forecast = `L`.

**Current month remainder.** For the current partial month the uncertain forecast is `max(0, forecast - month_to_date x)`.

### FC-1 Worked examples

| Case | Inputs | Expected |
|---|---|---|
| F-1 Even median | Groceries last 6: 42000, 42000, 43000, 43000, 44000, 44000 | sorted middle values 43000, 43000, `L` = **43000** |
| F-2 Half cent | Window 4 months: 42000, 42001, 41000, 45000 | sorted 41000, 42000, 42001, 45000, middle 42000 and 42001, `(84001)/2` = 42000.5, `L` = **42001** |
| F-3 Short history | Two months: 10000, 20000 | `L` = 15000, labeled "2-month median" |
| F-4 Zero history | No rows | baseline 0, label "No history yet" |
| F-5 Zero months inside the window | 0, 0, 30000, 0, 0, 30000 | sorted 0,0,0,0,30000,30000, `L` = 0. Honest: this category is lumpy, so the band (FC-4) carries the risk |
| F-6 New category | First row 2 months ago, values 5000, 7000 (2 complete months) | window is those 2 months only, `L` = 6000 |
| F-7 Seasonality | 13+ complete months, 12-month series: Dec(prev year) 8000, other 11 months 5000 each, `sum12` 63000. Forecast month Dec | raw Dec = 8000*120000/63000 = 15238.1, so 15238. raw other = 5000*120000/63000 = 9523.8, so 9524. Shrink (13-24 months): Dec `10000 + (15238-10000)/2` = **12619**. Other `10000 + (9524-10000)/2` = **9762**. Window Jun-Nov all 5000: `a(k)` = 5000*10000/9762 = 5122 each, `L'` = 5122. Forecast Dec = 5122*12619/10000 = 6463.45, so **6463**. Without seasonality it would be 5000 |
| F-8 Sparse category | Only 4 of the last 12 months have activity | No seasonal index (below `INDEX_ACTIVE_MIN`), forecast = `L` |
| F-9 Negative months | A category whose window values include -8000 (refund exceeded spending) | The negative is used as is in the median, never clamped |

## FC-2 Known flows: bills and income schedules

- **Source.** Active, non-completed schedules (R-BILL-*), expanded by the recurrence engine (WS-RULES) into dated occurrences for the horizon. Skipped occurrences and end conditions are honored. "Paid" occurrences already posted are not repeated.
- **Amount per occurrence.** Schedule amount operator `is`: exact. `isapprox`: median of the last up to `WINDOW` transactions linked to the schedule (`transactions.schedule`), else the stated amount. `isbetween`: midpoint `round_half_away((num1 + num2) / 2)`.
- **Known vs uncertain.** Exact bills, exact income and loan payments (DEBT) are known: no uncertainty. `isapprox` and `isbetween` amounts and all FC-1 baselines are uncertain.
- **Double counting.** A category with an active bill gets its baseline from non-bill rows only (FC-1), plus the bill's occurrences. A row the user types by hand instead of using Paid is not linked and will be counted twice. The forecast screen lists categories where this looks likely: any category whose unlinked monthly median is at least half of an active bill's amount.
- **Loan payments.** Balance falls by principal each month. With `rate_bps` set: `interest = round_half_up(outstanding * rate_bps / 120000)` and `principal = payment - interest`. With no rate: repeat the last payment's split. Interest is an expense in the projection, principal is not (net worth unchanged by principal, ST-4).
- **Card payments and transfers.** On-budget card payments are internal transfers and have no effect on net worth. They affect per-account balances only when a schedule exists for them (DEBT-7).

## FC-3 Projection (R-FC-1)

**Horizon.** 3, 6, 10 and 12 months mean the end of the 3rd, 6th, 10th and 12th complete month after the current month, plus the remainder of the current month.

**Net worth.** `NW(t) = NW(today) + sum of forecast net income (income - expenses) up to t`. Principal payments, savings transfers and card payments change no net worth. Non-cash changes (revaluations, adjustments, starting balances) are assumed zero and the screen says so. No market return or inflation is assumed unless a what-if adds it.

**Per-account balances.** Each flow moves the account it posts to. A baseline category posts to the account where the category had the largest sum of `|amount|` over its window (ties: lower account sort order). Known flows post to the schedule's account. Income schedules post to their account. Off-budget account balances change only by explicit flows (transfers, loan principal) and what-ifs.

**Scenario overlay (R-FC-2).** Scenario rows are added on top: one-time events (date, amount), recurring changes (start, optional end, amount, cadence), percent changes to a category's baseline (`round_half_away(baseline * pct_bp / 10000)`, `pct_bp` in basis points), income changes and goal contributions. A scenario is a pure function of read data and never writes to `transactions` or any other table except `veritas_scenario*`.

### FC-3 Worked example

Today is 2027-01-15 and checking holds 388800. One exact bill, Rent -165000 on the 1st. One exact income, Pay +300000 on the 1st. One baseline category, Groceries `L` 43000 with 20000 already spent this month. Next month (Feb):

- Known: +300000 - 165000 = +135000.
- Uncertain: -43000.
- Net income for Feb = +92000.
- Current-month remainder: Groceries `max(0, 43000 - 20000)` = 23000.
- End of January net worth = 388800 - 23000 = 365800. End of February = 365800 + 92000 = 457800.

## FC-4 Confidence band

### FC-4.1 What is in the band

Only uncertain flows (FC-2). The band covers the uncertain part of net flow `F_t` = uncertain income minus uncertain spending for month `t`. Known flows have none.

### FC-4.2 Method

1. Backtest on the user's own history (rolling origin): for each month `t` with at least 3 complete months before it, compute the FC-1 forecast `f_t` for the uncertain total using only data before `t`, and the actual `a_t`. The residual is `e_t = a_t - f_t` (spending enters with the sign convention of `F_t`).
2. With `n` residuals, sort ascending as `e(1)..e(n)`. Nearest-rank percentile: `P10 = e(ceil(n / 10))`, `P90 = e(ceil(9n / 10))`.
3. For horizon `h` months the band is scaled by an integer square root, assuming monthly errors are independent: `scale_h = isqrt(h * 100000000)` (this is `sqrt(h) * 10000`, truncated). `lo_h = round_half_away(P10 * scale_h / 10000)`, `hi_h = round_half_away(P90 * scale_h / 10000)`.
4. Net worth band at horizon `h` is `[NW_h + lo_h, NW_h + hi_h]`.

The label reads "80% band from N past months". The independence assumption is part of the label's detail view.

### FC-4.3 Fallback and no-history

- `n < BAND_MIN_RESIDUALS`: use `lo = min(F series) - median(F series)` and `hi = max(F series) - median(F series)` from the months available, scaled as above, labeled "Based on N months of history, not a statistical range".
- No complete month of history: no band, "No history yet". A projection with no band is shown only with that sentence.

### FC-4 Worked example

Residuals (n = 10), sorted: -8000, -5000, -3000, -2000, -500, 0, 1000, 2500, 4000, 9000. `P10 = e(ceil(1)) = e(1)` = **-8000**. `P90 = e(ceil(9)) = e(9)` = **4000**.

- h = 1: `scale` = isqrt(100000000) = 10000. Band offsets -8000 and +4000. NW projection 1000000 gives [992000, 1004000].
- h = 4: `scale` = isqrt(400000000) = 20000. Offsets -16000 and +8000. NW projection 1000000 gives [984000, 1008000].
- h = 3: `scale` = isqrt(300000000) = 17320. `lo` = -8000 * 17320 / 10000 = -13856, `hi` = 4000 * 17320 / 10000 = 6928.
- Fallback with only 3 monthly `F` values {-1000, 0, 2000}: median 0, so `lo` = -1000 - 0 = -1000 and `hi` = 2000 - 0 = 2000 (h = 1), labeled "Based on 3 months of history, not a statistical range".

## FC-5 Next-month suggestions and reasons (R-FC-3)

For each non-hidden, non-archived `expense` category (income, hidden and archived categories are skipped):

`suggestion = round_half_up((known bill occurrences in the month + forecast baseline) / 100) * 100`

Rounding: to the nearest whole dollar, half up, because budgeting is done in whole dollars. A negative result is 0. Applying suggestions is one undoable command that sets next month's budget amount for each category.

Goal categories use the goal suggestion in DEBT-8.1 instead of a baseline.

Each suggestion carries exactly one reason line, built from these templates (dollar amounts are the rounded values):

| Case | Reason |
|---|---|
| Baseline only | `{Category} ${amount}, {n}-month median` |
| Seasonal | `{Category} ${amount}, {n}-month median, adjusted {+/-pct}% for {Month}` (`pct = round_half_away((index - 10000) / 100)`) |
| Bills only | `{Category} ${amount}, scheduled: {bill names}` |
| Bills plus baseline | `{Category} ${amount}: ${bills} scheduled + ${rest} {n}-month median` |
| Goal | `{Goal} ${amount}: ${remaining} left over {months} months` |
| No history, no bill | No suggestion. `{Category}: no history yet, keeping ${current budgeted}` |

Notices (never silently scale the numbers): if the sum of suggestions exceeds the forecast income for the month, show "Suggestions exceed expected income by ${amount}". Suggestions never include transfers.

Examples: F-1 gives `Groceries $430, 6-month median`. A median of 42950 gives 430 (42950 / 100 = 429.5 rounds up). A category with Rent -165000 scheduled and no other rows gives `Rent $1650, scheduled: Rent`. Electric with `isapprox` median of last 6 payments 9050 gives `Electric $91, scheduled: Electric` (9050 / 100 = 90.5, rounds up).

## FC-6 Goal ETA (R-FC-4)

- **Contribution** `c(k)`: the goal category's budgeted amount for complete month `k` after all budget moves (`zero_budgets.amount`). Spending does not reduce pace.
- **Pace**: the median of the last up to 3 complete months. Optimistic pace: the maximum. Pessimistic pace: the minimum.
- **Remaining** = `max(0, target - available)`, with `available` from Actual's envelope math for the current month.
- **Months to go** = `ceil(remaining / pace)`, integer ceiling.
- **ETA month** = current month plus months to go.
- **Display** = "ETA {month}" with the range "optimistic to pessimistic month". If the pessimistic pace is 0 or less: "No ETA at the slowest pace". If the median is 0 or less: "No ETA: nothing is being added". `remaining = 0`: "Reached". No complete month of history: "No ETA yet".
- **On track** when the ETA month is not after the target month.

| Case | Inputs | Expected |
|---|---|---|
| E-1 | target 150000, available 112000, contributions 25000, 25000, 20000, current month 2027-01 | remaining 38000, pace 25000, 38000 / 25000 = 1.52, ceil **2**, ETA March 2027 (matches the "$1,120 of $1,500, on track for March" example in the plan) |
| E-2 range | same remaining, contributions 30000, 20000, 10000 | median 20000 gives 2 months (March), optimistic 30000 gives 2 months, pessimistic 10000 gives **4** months (May). Shown as "March, range March to May" |
| E-3 | contributions 0, 0, 0 | "No ETA: nothing is being added" |
| E-4 | available 150000 | "Reached" |
| E-5 | one month of history, contribution 25000 | median = min = max = 25000, one point, labeled "1 month of history" |

## FC-7 Backtest and error metrics

The spec for `docs/forecast-eval.md` (WS-FORECAST produces it from synthetic history).

**Method.** Rolling origin. For each origin month `o` with at least 6 complete months before it (3 for the uncertain-spending series), forecast months `o+1 .. o+h` for `h` in {1, 3, 6, 10, 12}, using only data before `o`. Compare with what happened.

**Targets.** (a) total uncertain net flow per month. (b) cumulative net-worth change over `h` months excluding non-cash changes (ST-4 lines E3, E4, E5), because the model does not predict them.

**Metrics, per horizon (each reports `n`, the number of origins):**

| Metric | Definition | Rounding |
|---|---|---|
| MAE (primary) | `sum(|e|) / n`, in cents | `round_half_up` |
| WAPE | `sum(|e|) * 10000 / sum(|actual|)`, in basis points | `round_half_up`. If `sum(|actual|) = 0`: "n/a" |
| Bias | `sum(e) / n` signed, in cents | `round_half_away` |
| Band coverage | `100 * (actuals inside [lo_h, hi_h]) / n` | whole percent, `round_half_up`. Nominal target 80 |
| Skill vs naive | `10000 - round_half_up(MAE_model * 10000 / MAE_naive)` in basis points. `MAE_naive = 0`: "n/a" | Naive = last complete month's value repeated; with at least 13 months, also same-month-last-year |

**Why MAE and WAPE, not MAPE.** Household categories are lumpy and often 0, so percent-of-actual metrics divide by zero or explode. MAPE also weights under-forecasts and over-forecasts unequally. MAE is in cents, a person can read it, and it adds up across categories. WAPE gives a scale-free percentage that stays defined whenever the total is non-zero.

**Initial acceptance targets on synthetic data with at least 24 months of history and known noise (to be confirmed by the first backtest, then recorded here):** band coverage between 70 and 90 at every horizon, and skill vs naive above 0 on stationary series. A series where the model loses to naive is reported, not hidden.

## FC-8 Wrapped metric definitions (R-WRAP-1, R-WRAP-2)

**Period.** A calendar year `Y`: 2027-01-01 to 2027-12-31. The current year is year-to-date: 01-01 to today's local civil date. All rows are classified by CLS-4, `leg != mirror`.

| Metric | Definition |
|---|---|
| Total in | `income_total` for the period (CLS-6) |
| Total out | `expense_total` for the period. Debt paydown and savings are shown as separate cards, never inside "out" |
| Net | Total in - Total out |
| Top categories | Per category, net `expense` (refunds netted). Keep only positive nets. Top 5 by net, ties by category name ascending (Unicode code point), then id. Hidden and archived categories included and labeled. Group totals are a second view with the same rule |
| Top payees | Per payee (after `payee_mapping`), `-sum(amount)` of `expense` rows. Exclude transfer payees. Keep positive nets, top 5, ties by name ascending, then id. Also show row counts |
| Biggest month | The month with the largest `expense_total`. Ties: earliest. A partial first month counts as is |
| No-spend day | A civil date `d` in the evaluated range on which no discretionary row exists. A **discretionary row** is a row of kind `expense` with `amount < 0`, `schedule` empty, and not a part of a loan payment (DEBT-6). Refunds, card payments, transfers, savings moves, income and bill-linked rows never break a no-spend day |
| Evaluated range | `max(Jan 1, date of the first row in the budget)` through `min(Dec 31, today)`. Days before the first row do not count as no-spend |
| No-spend count | Number of no-spend days in the evaluated range |
| Longest no-spend streak | Longest run of consecutive no-spend dates. Ties: earliest start. Also report the current streak as of today for the current year |
| Goals completed | Number of goals whose first month-end with `available >= target` falls in `Y` (a goal reached in an earlier year is not counted again). Closed goals count. The derived month uses Actual's month-end available |
| Net-worth change | `NW(end) - NW(start)` where `start` is the day before the period (ST-2). If the first row is inside the period, `NW(start) = 0`. Optional breakdown: ST-4 lines E1 to E5 |
| Prior-year change | `round_half_away((cur - prev) * 10000 / |prev|)` basis points. `prev = 0`: "new this year" |
| Biggest month vs typical | `round_half_away((max - median) * 10000 / median)` basis points, median over months with data. `median <= 0`: not shown |
| Fun comparisons | Computed only from the user's own numbers: vs prior year, vs own median month, share of total. No external benchmark ("average household") appears unless a primary source is cited here |

### Redaction mode (R-WRAP-2)

No dollar amount appears. Replace amounts with shares, in tenths of a percent:

- Shares across a set (top categories plus "Everything else") sum to exactly 1000 using largest remainder: floor each `amount * 1000 / total`, then give each missing tenth to the largest fractional remainder, ties to the earlier row.
- "Out vs in": `round_half_up(out * 1000 / in)` tenths. `in = 0`: "n/a".
- Net-worth change: change over opening net worth, `round_half_away`, tenths. Opening net worth <= 0: "n/a".
- Counts (days, streaks, goals) are shown as is. Month names are shown as is.

### FC-8 Worked examples

| Case | Inputs | Expected |
|---|---|---|
| W-1 No-spend days | Range 2027-01-01 to 2027-01-10. Discretionary expense rows on Jan 2, 3, 6, 9. Jan 1: rent bill-linked only. Jan 7: refund +1500. Jan 8: card payment | No-spend days: Jan 1, 4, 5, 7, 8, 10 = **6**. Longest streak **2** (Jan 4-5, earliest start; Jan 7-8 also 2). Current streak at Jan 10: 1 |
| W-2 Before first row | First row ever on Jan 5. Year range Jan 1 to Jan 10 | Evaluated range Jan 5 to Jan 10. Jan 1 to 4 not counted |
| W-3 Loan part | Loan payment interest row (kind `expense`, part of a loan payment) on Jan 10 | Does not break the day |
| W-4 Top categories | Net by category: Groceries 43000, Dining 30000, Fuel 17000, Gifts -2000, Fun 10000 | Top: Groceries, Dining, Fuel, Fun. Gifts excluded (negative net) |
| W-5 Tie | Two categories with 20000 each, names "Fun" and "Books" | "Books" first |
| W-6 Redaction shares | Groceries 43000, Dining 30000, Fuel 17000, Everything else 10000, total 100000 | 430, 300, 170, 100 tenths (43.0%, 30.0%, 17.0%, 10.0%), sum 1000 |
| W-7 Largest remainder | Amounts 1, 1, 1 over total 3 | Exact 333.33 each. Floors 333, 333, 333 sum 999. The missing tenth goes to the first: **334, 333, 333** |
| W-8 Net-worth change | `NW(start)` 720000, `NW(end)` 1118300 | Change **398300**. Redacted: 398300 * 1000 / 720000 = 553.19 tenths, so 553 (55.3%) |
| W-9 Prior-year change | prev 400000, cur 430000 | (30000 * 10000) / 400000 = 750 bp (7.5%) |
| W-10 Goal completion | Goal target 150000, month-end available: Feb 130000, Mar 150000, Apr 2000 (after the purchase) | Reached in March of its year. Counted once in that year. Not counted in later years |
| W-11 Biggest month tie | Jan and Jun both 300000 | January |
| W-12 Current year | Today is 2027-03-10 | Range 2027-01-01 to 2027-03-10. Biggest month and streaks use only that range |
| W-13 Empty year | No rows in the year | All counts 0, "No data for this year". No division happens |
