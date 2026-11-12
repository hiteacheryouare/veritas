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
| `INDEX_MIN`, `INDEX_MAX` | 2500, 120000 basis points | Seasonal index clamp (0.25 to 12.00). A month holding the whole 12-month sum gives 12.00, so for a window with no negative month the cap never cuts a real spike. A window with a negative month can give a larger raw index (Dec 10000, five months of 1000 and one month of -8000: `sum12` 7000, raw Dec = 10000*120000/7000 = 171429), and then the cap applies |
| `INDEX_ACTIVE_MIN` | 6 of the last 12 months with activity | Below this, no seasonal index for the category. Applies to each 12-month window separately (FC-1 step 1) |
| `BAND_GROWTH` | 3/4 | The band at horizon `h` grows like `h` to this power (FC-4.2 step 3). 1/2 would assume independent monthly errors, 1 would assume the same error repeats every month |

`WINDOW`, the index clamp, `INDEX_ACTIVE_MIN` and `BAND_GROWTH` are starting values. The backtest (FC-7) may motivate a change, which requires editing this table. Nobody tunes them in code.

### FC-0.1 Changes after the first backtest

The first backtest (`docs/forecast-eval.md` on `ws/forecast` at b83086f) raised three problems. This is what was decided and why.

1. **Seasonality fitting noise (stationary series, `h = 1`).** `SEASON_MIN` (13), the halving of the index for 13 to 24 months and the two-window average from 25 months are unchanged. The loss to "last month repeated" could not be reproduced: a rerun of the FC-1 rules on the same recipe (Groceries and Dining, 36 months, a different random generator, 100 seeds) gave +15% skill at `h = 1` with seasonality on and +19% with it off. Seasonality costs a stationary series about 4 points of skill. Turning it on only from 25 months would buy back under 1 point and cost a real seasonal category (Electric) about 9 points of skill at `h = 1`, so it is not worth it. A single 36-month series gives 32 overlapping origins for target (a) at `h = 1` and 29 for target (b), and one such run is too small to judge skill (FC-7 now asks for repeated runs).
2. **The index clamp hid annual spikes.** `INDEX_MAX` moved from 20000 to 120000 and `INDEX_MIN` from 5000 to 2500. A December that is about 8 times a typical month cannot be forecast with an index capped at 2.00. On the same recipe, Gifts at `h = 1` went from +41% to +55% skill and its bias fell from 32 to 21 dollars a month. Stationary and smoothly seasonal series did not move. The 13 to 24 month halving stays: one December seen once is as likely to be a wedding as a season, so a first-year spike is half believed (F-10). Making a known annual spike a yearly bill turns it into a known flow, which is better than any index.
3. **The band at long horizons.** The `sqrt(h)` band assumed independent monthly errors, but a level error or an annual spike repeats. Coverage of target (b) at 12 months fell to 11% (seasonal) and 33% (household) in the backtest against the 80% label, and to 58% on a stationary series in the rerun. `BAND_GROWTH` of 3/4 replaces 1/2 in FC-4.2. On the same recipe the stationary mix then covers 82% to 86% at every horizon from 3 months, and the household mix 72% to 77%. A smooth exponent is a compromise, not a theorem. Series dominated by annual lumps (Gifts and Electric together) are still under-covered (69% at 3 months, falling to 40% at 12). The detail view says so (FC-4.2).

All three are starting values like the rest of the table. The next backtest reports against them (FC-7).

Rounding rules used in this file:

- **round_half_up(a / b)** for non-negative `a`: `(2a + b) / (2b)` with integer division. Use it only where `a` cannot be negative by construction.
- **round_half_away(a / b)** for signed `a`: round half up on `|a|`, then restore the sign. Every division of a value that can be negative uses this rule, with no exceptions: medians of an even count, de-seasonalized values, seasonal forecasts, band offsets, scenario percent changes and prior-year changes. A negative `b` moves its sign onto `a` first.
- Money arithmetic that overflows is an error, never a clamp or a wrap. A wrong number that looks plausible is worse than no number.
- A "complete month" is a calendar month that has ended. The current month is partial and is never in a history window.

## FC-1 Baseline per category (R-FC-1)

Applies to each non-hidden `expense`-kind category (a spending baseline) and each `income`-kind category not fed by an income schedule (an income baseline). Debt, savings and transfers are not baselined here (FC-3).

**Monthly series.** For category `c` and complete month `m`: `x(c,m) = -sum(amount)` over rows of kind `expense` in `c` in `m` whose `schedule` column is empty (bill-linked rows are known flows, FC-2). For an income category, `x(c,m) = sum(amount)` of kind `income` rows not bill-linked. Refunds reduce `x`. Rows that move money between an on-budget and an off-budget account (CLS-4 rows 8 and 9) are left out of `x` in both directions, the way ST-4 line E2 treats them (FC-3, asset purchases). `x` may be negative.

**Window.** The last `WINDOW` complete months, but never months before the category's first row (a category created last month has one month of history, not six). A month after the first row with no rows counts as 0.

**Level.** `L = median` of the window values. Odd count: the middle value. Even count: `round_half_away((a + b) / 2)` of the two middle values (for a non-negative sum this is the same as `round_half_up`). `L` may be negative.

**Zero history.** No rows ever: baseline 0, label "No history yet". No suggestion is made (FC-5).

**Seasonality (turns on at `SEASON_MIN` = 13 complete months of history for the category):**

1. Take the latest 12 complete months. Each calendar month appears once. `sum12 = sum of x`. If `sum12 <= 0` or fewer than `INDEX_ACTIVE_MIN` of these months have `x != 0`, the category gets no seasonal index (index = 10000). With 25 or more months the same test runs on the previous 12 months (the 12 before the latest 12) on their own `sum12` and activity count. If the previous window fails it, step 3 treats the category as having 13 to 24 months (the shrink rule). The previous window never switches the index off when the latest window passes.
2. Raw index: `raw(m) = round_half_up(x(m) * 120000 / sum12)` in basis points (10000 = typical month). Negative `x` rounds as `max(0, x)` first.
3. Shrinkage: with 13 to 24 complete months, or with 25 or more when the previous window failed step 1: `index(m) = 10000 + round_half_away((raw(m) - 10000) / 2)`. With 25 or more months and both windows passing step 1: `raw(m)` is the average of the two windows' raw indices for the same calendar month, each normalized by its own sum (`round_half_up` of the two raw values added and divided by 2), and no shrinkage.
4. Clamp `index` to `[INDEX_MIN, INDEX_MAX]`. The 12 indices need not sum to exactly 120000 because of rounding. That is expected.
5. Forecast for target month `t` of calendar month `m`: de-seasonalize each window value, `a(k) = round_half_away(x(k) * 10000 / index(month of k))` (`x(k)` may be negative), take the median `L'` of the `a(k)`, and forecast `round_half_away(L' * index(m) / 10000)`. Without a seasonal index, forecast = `L`.

**Forecast used downstream.** The FC-1 forecast can be negative (a category whose typical month is a net refund). Every use of it after this section, meaning the projection (FC-3), the band's backtest residuals (FC-4.2), suggestions (FC-5) and scenario percent changes, uses `forecast+ = max(0, forecast)`. `L`, its label and the window values keep their sign. The projection never books a refund stream the history only suggests.

**Current month remainder.** For the current partial month the baseline forecast still to come is `max(0, forecast+ - month_to_date x)`.

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
| F-9 Negative months | A category whose window values include -8000 (refund exceeded spending) | The negative is used as is in the median, never clamped. A forecast that comes out negative is shown as `L` but projected as 0 (`forecast+`) |
| F-10 Annual spike is not clamped | 13 complete months. Latest 12: Dec(prev year) 40000, other 11 months 2000 each, `sum12` 62000. Forecast month Dec | raw Dec = 40000*120000/62000 = 77419.4, so 77419. raw other = 2000*120000/62000 = 3871.0, so 3871. Shrink (13-24 months): Dec `10000 + (77419-10000)/2`, where 67419/2 = 33709.5 rounds half away to 33710, so **43710**. Other `10000 + (3871-10000)/2`, where -6129/2 = -3064.5 rounds half away to -3065, so **6935**. Neither is clamped (2500 to 120000). Window Jun-Nov all 2000: `a(k)` = 2000*10000/6935 = 2883.9, so 2884, `L'` = 2884. Forecast Dec = 2884*43710/10000 = 12605.96, so **12606**. The old clamp (0.50 to 2.00) would have given 2884*20000/10000 = 5768. With 25+ months and the same spike in both years, raw is used as is: `a(k)` = 2000*10000/3871 = 5167, forecast = 5167*77419/10000 = 40002.40, so **40002** |
| F-11 Previous window too sparse | 25 complete months. Latest 12 as in F-7 (Dec 8000, 11 months of 5000). Previous 12: eleven months of 0 and one month of 9000 (1 active month, below `INDEX_ACTIVE_MIN`) | The previous window fails step 1, so the shrink rule applies to the latest window alone: Dec index **12619**, other **9762**, the same as F-7. The category does not lose its index |
| F-12 Signed rounding | Even median of -3 and -2. `a(k)` for `x` = -8000 at index 9762. `a(k)` for `x` = -1 at index 20000 | (-3 + -2)/2 = -2.5, half away from zero is **-3**. -8000*10000/9762 = -8195.04, so **-8195**. -1*10000/20000 = -0.5, so **-1**. Half up would have given -2 and 0 |

## FC-2 Known flows: bills and income schedules

- **Source.** Active, non-completed schedules (R-BILL-*), expanded by the recurrence engine (WS-RULES) into dated occurrences for the horizon. Skipped occurrences and end conditions are honored. "Paid" occurrences already posted are not repeated.
- **Unpaid and due today.** An occurrence dated today or earlier that is not yet posted is still money that has to go out (or come in). It is projected in the current month's remainder, as due now. Nothing that is unposted drops out of the projection because its date has passed. The caller removes only posted occurrences. A what-if event dated today counts in the current month. A what-if dated before today is rejected as invalid.
- **Amount per occurrence.** Schedule amount operator `is`: exact. `isapprox`: median of the last up to `WINDOW` transactions linked to the schedule (`transactions.schedule`), else the stated amount. `isbetween`: midpoint `round_half_away((num1 + num2) / 2)`.
- **Known, estimated and baselined.** Exact bills, exact income and loan payments (DEBT) are **known**: no uncertainty. `isapprox` and `isbetween` amounts are **estimated**: a point amount with real uncertainty. All FC-1 category baselines are **baselined**. The band (FC-4.1) covers baselined flows only. Estimated amounts are counted at their expected amount and add no range.
- **Double counting.** A category with an active bill gets its baseline from non-bill rows only (FC-1), plus the bill's occurrences. A row the user types by hand instead of using Paid is not linked and will be counted twice. An income schedule feeds the category on its flow, or when the flow has none, the most common category of the rows linked to it. If neither exists, the income baselines whose median is at least half the schedule's amount are listed as possible double counts, because the schedule's income cannot be kept out of them. The forecast screen lists categories where this looks likely: any category whose unlinked monthly median is at least half of an active bill's amount.
- **Loan payments.** A payment is made of interest, escrow, fees and principal (DEBT-1, DEBT-2). `payment` is the total that leaves the paying account. The balance falls by principal each month. With `rate_bps` set: `interest = round_half_up(outstanding * rate_bps / 120000)`. With no rate: repeat the last payment's interest. `escrow` is the last payment's escrow (DEBT-3), repeated. `fees` are 0 in a forecast. Then `principal = payment - interest - escrow - fees` (DEBT-2.1), floored at 0 and capped at `outstanding` (DEBT-3, final payment: the total shrinks by what the cap removed). The payment covers interest first, then escrow, then principal (DEBT-2.1a). If `payment` is below interest plus escrow, principal is 0 and only what the payment covers is booked, interest first. The part not covered is not recorded and the loan balance moves only by principal (DEBT-2.6). A forecast assumes no non-cash changes (FC-3), so capitalized interest is never projected. The screen warns that the payment is below interest and escrow (DEBT-2.2). Interest and escrow are expenses in the projection, each in the category of the last payment (DEBT-2.4, DEBT-2.5). Principal is not an expense: net worth is unchanged by principal (ST-4). The order of allocation is DEBT-2.1a. Once `outstanding` reaches 0 the loan has no more flows, escrow included.
- **A schedule that pays a loan is a loan payment.** A bill or schedule whose payee is a transfer to an off-budget liability account (a loan or mortgage) is the loan's payment schedule. Its dates and amount are the loan's `payment` and payment dates, and each occurrence is split by the Loan payments rule above. It is never booked as one transfer, because that would treat interest and escrow as principal and overstate net worth by interest plus escrow every month. Example, FL-1 numbers: a 220000 mortgage bill with 50000 escrow and 150000 interest books expense 200000, principal 20000, and net worth falls 200000. The interest and escrow come from the last payment's split: with one expense part it is the interest, with several parts and a rate the part closest to `round_half_up(outstanding * rate_bps / 120000)` is the interest and the rest is escrow, and with several parts and no rate the loan is listed as "not forecast" with that reason. A loan with no recorded payment is also listed as "not forecast".
- **Card payments and transfers.** On-budget card payments are internal transfers and have no effect on net worth. They affect per-account balances only when a schedule exists for them (DEBT-7).

### FC-2 Worked examples

| Case | Inputs | Expected |
|---|---|---|
| FL-1 Loan with escrow | `outstanding` 30000000, `rate_bps` 600, `payment` 220000, `escrow` 50000, two payments | Payment 1: interest 30000000*600/120000 = **150000**, principal 220000 - 150000 - 50000 = **20000**, `outstanding` 29980000. Payment 2: interest 29980000*600/120000 = **149900**, principal 220000 - 149900 - 50000 = **20100**, `outstanding` 29959900. Expense per month 200000 then 199900, so net worth falls by that. Checking falls 220000 each month. Booking everything above interest as principal would give 70000 and overstate net worth by 50000 a month |
| FL-2 Final payment with escrow | `outstanding` 20000, no rate, last interest 100, `escrow` 50000, `payment` 220000 | principal = min(220000 - 100 - 50000, 20000) = **20000**. Total paid 20000 + 100 + 50000 = **70100**. No flows after it |
| FL-3 Payment below interest and escrow | `payment` 160000, interest 150000, `escrow` 50000 | 160000 - 150000 - 50000 = -40000, so principal **0**. The payment covers interest 150000 and 10000 of escrow. Expense booked **160000**. Checking falls **160000**. The loan balance does not change. Net worth falls **160000**. The uncovered 40000 is not recorded. Warning shown |

## FC-3 Projection (R-FC-1)

**Asset purchases.** A transfer from an on-budget account into an off-budget asset with an expense category (CLS-4 row 8), and its reversal (row 9), is not spending: money still owned as an asset has not left net worth (ST-4 E2). These rows are excluded from every baseline series (FC-1), so the projection, the band's residuals and the suggestions never forecast them, and buying an asset does not lower projected net worth. Because the baseline cannot predict when such a purchase will happen, the projection shows "N asset purchase(s) excluded: ${total}" ("1 asset purchase", "2 asset purchases") for the baseline window (`WINDOW` complete months), where `N` is the count of excluded rows, `total` is the net of their amounts rounded half up to whole dollars, and the note is omitted when `N` is 0. Cash will really fall on the day of a purchase. A planned purchase belongs in a what-if as a one-time transfer, not in the baseline.

**Horizon.** 3, 6, 10 and 12 months mean the end of the 3rd, 6th, 10th and 12th complete month after the current month, plus the remainder of the current month.

**Net worth.** `NW(t) = NW(today) + sum of forecast net income (income - expenses) up to t`. Principal payments, savings transfers and card payments change no net worth. Non-cash changes (revaluations, adjustments, starting balances) are assumed zero and the screen says so. No market return or inflation is assumed unless a what-if adds it.

**Spending that is not baselined.** Uncategorized spending (CLS-4 row 15) and spending in hidden or archived categories get no baseline, so the projection leaves them out and net worth reads higher than the history suggests. The screen says so with a line: "Not forecast: ${median} a month in uncategorized or hidden spending", where `median` is the median over the baseline window of that spending's monthly total, rounded half up to whole dollars, and the line is omitted when it is 0. The amount is never added to the projection until the product question of forecasting it as one series is decided.

**Per-account balances.** Each flow moves the account it posts to. A baseline category posts to the account where the category had the largest sum of `|amount|` over its window (ties: lower account sort order). Known flows post to the schedule's account. Income schedules post to their account. Off-budget account balances change only by explicit flows (transfers, loan principal) and what-ifs.

**Scenario overlay (R-FC-2).** Scenario rows are added on top: one-time events (date, amount), recurring changes (start, optional end, amount, cadence: every N months or every N weeks, anchored on the start date, the same recurrence as bills so that a biweekly paycheck gives 26 or 27 occurrences in a calendar year (27 when anchored on Jan 1) and never the 24 of a monthly cadence), percent changes to a category's baseline, income changes and goal contributions. A scenario is a pure function of read data and never writes to `transactions` or any other table except `veritas_scenario*`.

- **Percent change.** A delta on the category's `forecast+` (FC-1) for each month it is active: `delta = round_half_away(forecast+ * pct_bp / 10000)`, `pct_bp` in basis points, and the scenario forecast is `max(0, forecast+ + delta)`. Several active changes on one category add their deltas. Each delta is computed from the same unadjusted `forecast+`, so they do not compound. In the current month the remainder rule applies afterwards: `max(0, scenario forecast - month_to_date x)`.
- **Income change.** An amount added each active month, or `round_half_away(expected_income * pct_bp / 10000)`, where `expected_income` is that month's known income plus income baselines (`forecast+`) for the one category named, or for all income when none is named. For the current month it is the expected income still to come.
- **Goal contribution.** A budget move, not a transaction. It changes a goal's ETA (FC-6) and nothing else: no balance, no net worth, no band, no suggestion. It is not part of the projection's rows.
- **Band.** The band offsets (FC-4.2) come from the baselines' history and are the same with or without a scenario. The scenario only moves the net worth they are added to.

### FC-3 Worked example (single account, not total net worth)

Today is 2027-01-15 and checking holds 388800. One exact bill, Rent -165000 on the 1st. One exact income, Pay +300000 on the 1st. One baseline category, Groceries `L` 43000 with 20000 already spent this month. Next month (Feb):

- Known: +300000 - 165000 = +135000.
- Baselined: -43000.
- Net income for Feb = +92000.
- Current-month remainder: Groceries `max(0, 43000 - 20000)` = 23000.
- End of January net worth = 388800 - 23000 = 365800. End of February = 365800 + 92000 = 457800.

### FC-3 Scenario examples

| Case | Inputs | Expected |
|---|---|---|
| SC-1 Percent change rounds half away | Groceries `forecast+` 43005, change -1000 bp | delta = 43005 * -1000 / 10000 = -4300.5, so **-4301**. Scenario forecast **38704** |
| SC-2 Clamped at 0 | Groceries `forecast+` 43000, change -15000 bp (-150%) | delta = **-64500**. 43000 - 64500 = -21500, clamped to **0**. Spending is never negative because of a percent change |
| SC-3 Deltas add, they do not compound | Groceries `forecast+` 43000, active changes +1000 bp and +500 bp | 4300 + 2150 = 6450. Scenario forecast **49450** (not 49665) |
| SC-4 Negative forecast | A category whose FC-1 forecast is -3000, change +1000 bp | `forecast+` is 0, delta 0, scenario forecast **0** |
| SC-5 Goal contribution | Scenario adds 25000 a month to a goal from February | Net worth, balances and band offsets are identical to the baseline projection. Only the goal's ETA changes (E-7) |
| AP-1 Asset purchase | Fuel window of 2 complete months: Jan 20000 of fuel, Feb 20000 of fuel plus a 50000 transfer from CHK to an off-budget CAR asset in category Fuel (row 8) | The 50000 row is excluded: `x` = 20000, 20000, `L` = **20000**. Counting it would give 20000 and 70000, `L` = 45000 and a projected net worth 25000 lower every month. Note: "1 asset purchase excluded: $500" (one purchase, 50000 cents) |

## FC-4 Confidence band

### FC-4.1 What is in the band

The band covers the category baselines only, for now: `F_t` = baselined income minus baselined spending for month `t` (each baseline as `forecast+`, FC-1). Known flows have none. Bill amounts that are `isapprox` or `isbetween` are estimated (FC-2), but the band does not widen for them: they are counted at their expected amount and the detail view says so. Scenarios do not change the band (FC-3).

### FC-4.2 Method

1. Backtest on the user's own history (rolling origin): for each month `t` with at least 3 complete months before it, compute the FC-1 forecast `f_t` (each category as `forecast+`) for the baselined total using only data before `t`, and the actual `a_t`. The residual is `e_t = a_t - f_t` (spending enters with the sign convention of `F_t`).
2. With `n` residuals, sort ascending as `e(1)..e(n)`. Nearest-rank percentile: `P10 = e(ceil(n / 10))`, `P90 = e(ceil(9n / 10))`.
3. For horizon `h` months the one-month band is scaled by `h` to the power `BAND_GROWTH` = 3/4, by integer arithmetic: `scale_h = isqrt(isqrt(h^3 * 10^16))` (this is `h^0.75 * 10000`, truncated; nested integer square roots give the integer fourth root exactly). `h^3 * 10^16` overflows a signed 64-bit integer from `h = 10` (10^19) and still fits an unsigned 64-bit integer at `h = 12` (1.728 * 10^19), so compute it in an unsigned 64-bit or wider integer. `lo_h = round_half_away(P10 * scale_h / 10000)`, `hi_h = round_half_away(P90 * scale_h / 10000)`. At `h = 1` the scale is 10000, so the one-month band is unchanged. The scale is deliberately wider than independent errors would need (`sqrt(h)`): a level error or an annual lump repeats from month to month.
4. Net worth band at horizon `h` is `[NW_h + lo_h, NW_h + hi_h]`.

The label reads "80% band from N past months". The detail view states the method, N, that errors are assumed to partly repeat (growth 3/4, not independent), that annual lumps such as gifts or insurance can fall outside the range (a yearly bill makes them known), and that the range covers category baselines only (FC-4.1).

### FC-4.3 Fallback and no-history

- `n < BAND_MIN_RESIDUALS`: use `lo = min(F series) - median(F series)` and `hi = max(F series) - median(F series)` from the months available, scaled as above, labeled "Based on N months of history, not a statistical range".
- No complete month of history: no band, "No history yet". A projection with no band is shown only with that sentence.

### FC-4 Worked example

Residuals (n = 10), sorted: -8000, -5000, -3000, -2000, -500, 0, 1000, 2500, 4000, 9000. `P10 = e(ceil(1)) = e(1)` = **-8000**. `P90 = e(ceil(9)) = e(9)` = **4000**.

- h = 1: `scale` = isqrt(isqrt(10^16)) = isqrt(100000000) = 10000. Band offsets -8000 and +4000. NW projection 1000000 gives [992000, 1004000].
- h = 3: `scale` = isqrt(isqrt(27 * 10^16)) = isqrt(519615242) = 22795. `lo` = -8000 * 22795 / 10000 = **-18236**, `hi` = 4000 * 22795 / 10000 = **9118**.
- h = 4: `scale` = isqrt(isqrt(64 * 10^16)) = isqrt(800000000) = 28284. `lo` = -8000 * 28284 / 10000 = -22627.2, so **-22627**. `hi` = 4000 * 28284 / 10000 = 11313.6, so **11314**. NW projection 1000000 gives [977373, 1011314].
- h = 12: `scale` = isqrt(isqrt(1728 * 10^16)) = 64474. `lo` = -8000 * 64474 / 10000 = -51579.2, so **-51579**. `hi` = 4000 * 64474 / 10000 = 25789.6, so **25790**.
- Fallback with only 3 monthly `F` values {-1000, 0, 2000}: median 0, so `lo` = -1000 - 0 = -1000 and `hi` = 2000 - 0 = 2000 (h = 1), labeled "Based on 3 months of history, not a statistical range".

## FC-5 Next-month suggestions and reasons (R-FC-3)

For each non-hidden, non-archived `expense` category (income, hidden and archived categories are skipped):

`suggestion = round_half_up(max(0, bills + baseline) / 100) * 100`

`bills` = minus the sum of the signed amounts of the category's bill occurrences in the month (Rent -165000 gives 165000). `baseline` is `forecast+` (FC-1) for the category, a non-negative spending magnitude. Clamp to 0 first, then round.

Rounding: to the nearest whole dollar, half up, because budgeting is done in whole dollars. Goal suggestions (DEBT-8.1) are the exception and stay in cents. The `debt_principal` envelope's suggestion is the loan's pre-fill principal (DEBT-3), rounded the same way. Applying suggestions is one undoable command that sets next month's budget amount for each category.

Goal categories use the goal suggestion in DEBT-8.1 instead of a baseline. `debt_principal` categories use the line above instead of a baseline.

Each suggestion carries exactly one reason line, built from these templates (dollar amounts are the rounded values):

| Case | Reason |
|---|---|
| Baseline only | `{Category} ${amount}, {n}-month median` |
| Seasonal | `{Category} ${amount}, {n}-month median, adjusted {+/-pct}% for {Month}` (`pct = round_half_away((index - 10000) / 100)`) |
| Bills only | `{Category} ${amount}, scheduled: {bill names}` |
| Bills plus baseline | `{Category} ${amount}: ${bills} scheduled + ${rest} {n}-month median`. Also used when the baseline is seasonal, with no "adjusted" clause: the bills part has no season. `bills` is the month's bills rounded half up to whole dollars, and `rest = amount - bills`, floored at 0, so the two parts always add up to the suggestion even though each would round on its own |
| Goal | `{Goal} ${amount}: ${remaining} left over {months} months`, with both amounts in dollars and cents because goal suggestions stay in cents (DEBT-8.1) |
| Goal, no target date | No suggestion. `{Goal}: no target date, keeping ${current budgeted}` |
| Debt principal | `{Category} ${amount}, principal of the last loan payment`. `amount` is that principal rounded half up to whole dollars |
| No history, no bill | No suggestion. `{Category}: no history yet, keeping ${current budgeted}` |

In the two "keeping" lines, `{current budgeted}` is the category's budgeted amount for the suggested month, shown in whole dollars rounded half up. It is a label only. The budget cell keeps its exact cents and nothing is written.

Notices (never silently scale the numbers): if the sum of suggestions exceeds the forecast income for the month, show "Suggestions exceed expected income by ${amount}". Suggestions never include transfers.

- **Expected income** for the month is the month's known income flows plus the income baselines (`forecast+`).
- **The comparison is in exact cents**: the sum of the suggested amounts (whole dollars, except goal suggestions in cents) minus expected income. The notice appears when the difference is above 0.
- **The amount shown is in whole dollars**, `round_half_up(difference / 100)`, but never less than $1, so the notice never reads "$0".

Examples: F-1 gives `Groceries $430, 6-month median`. A median of 42950 gives 430 (42950 / 100 = 429.5 rounds up). A category with Rent -165000 scheduled and no other rows gives `Rent $1650, scheduled: Rent`. Electric with `isapprox` median of last 6 payments 9050 gives `Electric $91, scheduled: Electric` (9050 / 100 = 90.5, rounds up).

### FC-5 Worked examples

| Case | Inputs | Expected |
|---|---|---|
| SG-1 Bills plus baseline add up | Electric bill 5049, baseline forecast 2040 | 5049 + 2040 = 7089, rounds to 7100, so `amount` $71. `bills` 5049 rounds to 5000 ($50), `rest` = 7100 - 5000 = 2100. `Electric $71: $50 scheduled + $21 6-month median`. Rounding the baseline alone would give $20 and the parts would add to $70 |
| SG-2 Seasonal plus bills | Electric bill 5070, seasonal baseline forecast 2040 for the month, index 12619 | 7110 rounds to 7100, `bills` 5100 ($51), `rest` 2000. `Electric $71: $51 scheduled + $20 6-month median`. No "adjusted" clause |
| SG-3 Goal with no target date | Goal "New laptop", target 150000, no target date, February budgeted 10000 | No suggestion. `New laptop: no target date, keeping $100` |
| SG-4 Debt principal | Last loan payment's principal 32050 (CHK to the loan, `debt_principal` category), loan still owes more | 32050 / 100 = 320.5 rounds up, so 32100. `Loan principal $321, principal of the last loan payment` |
| SG-5 Notice, exact cents | Suggestions sum 208000, expected income 200000 | Difference 8000, notice `Suggestions exceed expected income by $80` |
| SG-6 Notice under a dollar | A goal suggestion of 21429 plus whole-dollar suggestions of 100000 sum to 121429, expected income 121400 | Difference 29 is above 0, so the notice appears. 29 / 100 rounds to 0, floored to **$1**: `Suggestions exceed expected income by $1`. Equal sums give no notice |
| SG-7 Negative forecast | A category with FC-1 forecast -3000 and no bills | `forecast+` 0, `max(0, 0)` = 0, rounds to 0, `Fun $0, 6-month median` |

## FC-6 Goal ETA (R-FC-4)

- **Contribution** `c(k)`: the goal category's budgeted amount for complete month `k` after all budget moves (`zero_budgets.amount`). Spending does not reduce pace.
- **History** starts at the goal category's earliest budget cell. Months before it are not history and are not counted as 0. A complete month from that cell on with no cell counts as 0. With no cell in a complete month before `M` there is no history.
- **Pace**: the median of the last up to 3 complete months of history. Optimistic pace: the maximum. Pessimistic pace: the minimum. Fewer than 3 months of history use what exists and say how many.
- **Remaining** = `max(0, target - available)`, with `available` from Actual's envelope math for the current month `M`. `available` includes `M`'s own budgeted amount, which is a known contribution, so only later months need the pace. This is the counterpart of DEBT-8.1, which excludes `M`'s amount because it is deciding that amount. Both agree: with pace equal to the suggestion the ETA is the target month.
- **Months to go** = `ceil(remaining / pace)`, integer ceiling.
- **ETA month** = current month plus months to go.
- **With a scenario (R-FC-2).** A goal contribution in the scenario is added to the pace for each month it is active. Month `M + j` adds `pace + extra(j)`, and the ETA is the first month where the running total reaches `remaining`. After the last month with an extra, the steady pace finishes the count with the same ceiling. A goal contribution in the scenario that is active in the current month `M` is added to `available` before the count starts, because `M`'s own budgeted amount is already in `available` and a what-if can add to it. If that alone reaches `remaining`, the ETA is `M` itself. The same rule is applied to the median, optimistic and pessimistic paces. The scenario changes this ETA and nothing else (FC-3).
- **Cap.** An ETA more than 1200 months out is "No ETA", the same as a pace of 0. It is never an error and never skips months.
- **Display** = "ETA {month}" with the range "optimistic to pessimistic month". `remaining = 0`: "Reached". No complete month of history: "No ETA yet". If the median is 0 or less: "No ETA: nothing is being added", which replaces the whole line. Otherwise, if the pessimistic pace is 0 or less, the line reads "ETA {month}, no ETA at the slowest pace": the slowest pace replaces only the range, and the median ETA stays. The checks run in that order: Reached, no history, median, then pessimistic pace. Contributions 0, 0, 0 therefore read "No ETA: nothing is being added" (E-3).
- **On track** when the ETA month is not after the target month.

| Case | Inputs | Expected |
|---|---|---|
| E-1 | target 150000, available 112000, contributions 25000, 25000, 20000, current month 2027-01 | remaining 38000, pace 25000, 38000 / 25000 = 1.52, ceil **2**, ETA March 2027 (matches the "$1,120 of $1,500, on track for March" example in the plan) |
| E-2 range | same remaining, contributions 30000, 20000, 10000 | median 20000 gives 2 months (March), optimistic 30000 gives 2 months, pessimistic 10000 gives **4** months (May). Shown as "March, range March to May" |
| E-3 | contributions 0, 0, 0 | "No ETA: nothing is being added" |
| E-4 | available 150000 | "Reached" |
| E-5 | one month of history, contribution 25000 | median = min = max = 25000, one point, labeled "1 month of history" |
| E-6 Consistency with DEBT-8.1 | target 150000, target month June 2027, current month January, budgeted 25000, available 25000, pace 25000 | remaining 125000, ceil(125000 / 25000) = 5, ETA = January + 5 = June 2027 = the target month, on track |
| E-7 Scenario contribution | target 150000, available 0, current month 2027-01, one prior month at 10000 (pace 10000). Scenario adds 40000 in February and March | running totals: Feb 50000, Mar 100000, then 10000 a month: Apr 110000, May 120000, Jun 130000, Jul 140000, Aug 150000. ETA **August 2027**. Without the scenario: ceil(150000 / 10000) = 15, April 2028. Open-ended +15000 from February instead: 25000 a month, ceil(150000 / 25000) = 6, **July 2027** |
| E-8 A gap inside the history | cells: Oct 2026 25000, Dec 2026 20000, none for Nov. Current month 2027-01 | history starts in October, so November counts as 0: contributions 25000, 0, 20000. Median **20000**, optimistic 25000, pessimistic **0**. With remaining 38000 the median gives ceil(38000 / 20000) = 2, so the line reads "ETA March 2027, no ETA at the slowest pace" |
| E-9 A goal that starts late | The goal category exists since October, but its first budget cell is December 2026 at 25000. Current month 2027-01 | History is December only: 1 month, pace **25000**. October and November are not counted as 0 (that would make the median 0 and read "nothing is being added") |
| E-10 First cell is this month | The goal's only budget cell is the current month | No complete month of history: "No ETA yet" (unless `remaining = 0`, then "Reached") |

## FC-7 Backtest and error metrics

The spec for `docs/forecast-eval.md` (WS-FORECAST produces it from synthetic history).

**Method.** Rolling origin. Each origin `o` is a "today" on the 1st of month `o`. For each origin with enough complete months before it (3 for target (a), 6 for target (b)), forecast months `o+1 .. o+h` for `h` in {1, 3, 6, 10, 12}, using only data before `o`. Month `o` has no days elapsed, so it is skipped and scoring starts at `o+1`. Compare with what happened.

- **Naive** is the last complete month before `o`, repeated: once for target (a), `h` times for target (b).
- **Bills are perfect foresight.** Bill-linked rows in the scored months are taken as they actually posted, as known flows. This flatters target (b) and is stated next to its numbers. Target (a) has no bills in it.
- **Band for target (a)** is the one-month band (`h = 1`), because (a) is one month and carries one month of error. **Band for target (b)** is the `h`-scaled band of FC-4.2 step 3.
- **Error sign.** `e = actual - forecast`, so a negative bias on net flow means more spending than forecast.

**Targets.** (a) total baselined net flow per month (baselined income minus baselined spending). (b) cumulative net-worth change over `h` months excluding non-cash changes (ST-4 lines E3, E4, E5, E6 and E7), because the model does not predict them.

**Metrics, per horizon (each reports `n`, the number of origins):**

| Metric | Definition | Rounding |
|---|---|---|
| MAE (primary) | `sum(|e|) / n`, in cents | `round_half_up` |
| WAPE | `sum(|e|) * 10000 / sum(|actual|)`, in basis points | `round_half_up`. If `sum(|actual|) = 0`: "n/a" |
| Bias | `sum(e) / n` signed, in cents | `round_half_away` |
| Band coverage | `100 * (actuals inside [lo_h, hi_h]) / n` | whole percent, `round_half_up`. Nominal target 80 |
| Skill vs naive | `10000 - round_half_up(MAE_model * 10000 / MAE_naive)` in basis points. `MAE_naive = 0`: "n/a" | Naive = last complete month's value repeated; with at least 13 months, also same-month-last-year |

**Why MAE and WAPE, not MAPE.** Household categories are lumpy and often 0, so percent-of-actual metrics divide by zero or explode. MAPE also weights under-forecasts and over-forecasts unequally. MAE is in cents, a person can read it, and it adds up across categories. WAPE gives a scale-free percentage that stays defined whenever the total is non-zero.

**Repeated runs.** One 36-month series gives 32 overlapping origins at `h = 1` for target (a), falling to 21 at `h = 12`, and 29 falling to 18 for target (b), and a single run is too small to judge skill: in a rerun of the first backtest's recipe the skill of single seeds at `h = 1` ranged from -45% to +44% around a median of +15%. Every mix is run with at least 20 seeds. The report gives the median of each metric across seeds, the lowest and highest, and the share of seeds with skill below 0. The cross-seed median is the FC-0 median: for an even count, `round_half_away((a + b) / 2)` of the two middle values, in integers. With 20 seeds the median is therefore the average of the 10th and 11th values, and never the lower one: the lower middle value reports a smaller error than the typical one. Example: seed values 10 and 20 in the middle give 15, and -3 and -2 give -3. Each mix is also run with seasonality off (`SEASON_MIN` set past the history) on the same 20 or more seeds, so the report shows what seasonality adds or costs on matching data.

**Initial acceptance targets on synthetic data with at least 24 months of history and known noise (the first backtest and its rerun are recorded in FC-0.1):**

- Stationary and household mixes: median band coverage between 70 and 90 at every horizon, and median skill vs naive above 0 on stationary series at every horizon.
- Seasonal mixes dominated by annual lumps (gifts, insurance) are reported and not gated on band coverage. Their lumps are better made into yearly bills (known flows), and the report says so.

A series where the model loses to naive, or a band that misses its target, is reported, not hidden.

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
| Goals completed | Number of goals whose stored `reached_month` (DEBT-8.2) falls in `Y`. A goal reached in an earlier year is not counted again. Closed goals count. A goal spent after being reached still counts, because `reached` never reverts |
| Net-worth change | `NW(end) - NW(start)` where `start` is the day before the period (ST-2). If the first row is inside the period, `NW(start) = 0`. Optional breakdown: ST-4 lines E1 to E7 |
| Prior-year change | For each of total in, total out and net: `round_half_away((cur - prev) * 10000 / |prev|)` basis points. `prev` is the same measure over the same date range one year earlier: for a full year the previous full year, for the current year-to-date 01-01 through the same month and day (Feb 29 maps to Feb 28). `prev = 0`: "new this year". If the first row in the budget falls after the start of the prior range, the prior figure covers only the days since that row and is labeled "prior year from {date}" |
| Biggest month vs typical | `round_half_away((max - median) * 10000 / median)` basis points, median over months with data. `median <= 0`: not shown |
| Fun comparisons | Computed only from the user's own numbers: vs prior year, vs own median month, share of total. No external benchmark ("average household") appears unless a primary source is cited here |

### Redaction mode (R-WRAP-2)

No dollar amount appears. Replace amounts with shares, in tenths of a percent:

- "Everything else" is the sum of the positive nets of the categories outside the top 5. Zero and negative nets are in neither the top 5 nor "Everything else". Uncategorized rows are not a category: they are left out of the ranking, of "Everything else" and of the total the shares divide by.
- Shares across a set (top categories plus "Everything else") sum to exactly 1000 using largest remainder: floor each `amount * 1000 / total`, then give each missing tenth to the largest fractional remainder, ties to the earlier row.
- "Out vs in": `round_half_up(out * 1000 / in)` tenths. `in <= 0`: "n/a".
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
| W-9b Prior-year net, negative base | prev net -20000, cur net +10000 | (30000 * 10000) / 20000 = **15000** bp (+150%). Year-to-date on 2027-03-10 compares with 2026-01-01 to 2026-03-10 |
| W-10 Goal completion | Goal target 150000, month-end available: Feb 130000, Mar 150000, Apr 2000 (after the purchase) | `reached_month` = March, stored when March's available hit 150000, and not reverted by the April purchase. Counted once in that year. Not counted in later years |
| W-11 Biggest month tie | Jan and Jun both 300000 | January |
| W-12 Current year | Today is 2027-03-10 | Range 2027-01-01 to 2027-03-10. Biggest month and streaks use only that range |
| W-13 Empty year | No rows in the year | All counts 0, "No data for this year". No division happens |
| W-14 Everything else and uncategorized | Net by category: Groceries 43000, Dining 30000, Fuel 17000, Fun 10000, Books 6000, Gifts 5000, Pets 4000, Refunds -2000. Uncategorized spending 9000 | Top 5: Groceries, Dining, Fuel, Fun, Books. Everything else = Gifts + Pets = **9000**. Refunds and uncategorized are in neither. Total = 115000. Floors 373, 260, 147, 86, 52, 78 sum 996. Missing 4 go to the largest remainders: Fun, Groceries, Dining, Fuel. Shares **374, 261, 148, 87, 52, 78** (Groceries, Dining, Fuel, Fun, Books, Everything else), sum 1000 |
| W-15 Out vs in | in 100000, out 50000. Then in 0. Then in -3000 | 500 tenths (50.0%). Then "n/a". Then "n/a" |
