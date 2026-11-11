# Forecast evaluation (FC-7)

This is the backtest that docs/finance/forecasting.md FC-7 asks for, run against the FC-0 constants as they stand after FC-0.1. The tables come from `crates/veritas-forecast/tests/backtest.rs`, which fails when they no longer match the code. Rerun it with `VERITAS_WRITE_FORECAST_EVAL=1` to rewrite them. Everything outside the generated block is written by hand.

## Method

Each origin `o` is a "today" on the 1st of month `o`. The forecast trains on the complete months before `o`, skips `o` itself (no days have passed) and is scored on `o + 1` through `o + h` for `h` in 1, 3, 6, 10 and 12.

- Target (a) is the baselined net flow of the single month `o + h`: baselined income minus baselined spending. Its band is the one-month band, because one month carries one month of error. Origins need 3 complete months before them.
- Target (b) is the net-worth change over `o + 1` through `o + h` without non-cash changes, which is E1 + E2 of statements.md ST-4. Its band is the `h^(3/4)`-scaled band of FC-4.2. Origins need 6 complete months before them.
- Bill-linked rows in the scored months count as known flows. That is perfect foresight of bills, and it flatters target (b). Target (a) has no bills in it.
- "Skill vs last month" compares against the last complete month before `o`, repeated (`h` times for (b)). "Skill vs last year" compares against the same calendar month in the latest year before `o`, scored only on origins with at least 13 months of history.
- Error is `actual - forecast`, so a negative bias means the model forecast more net income than happened: here, it under-forecast spending.
- Every mix runs on 20 seeds. Each cell is the median across seeds, with the lowest and highest seed in brackets. "Seeds below 0" counts the seeds whose skill vs last month is negative. The last column reruns the same 20 seeds with seasonality off (it never turns on), so the two skill columns compare matching data.

## Data

36 months of synthetic history, January 2024 to December 2026, made with SplitMix64 so every run is identical. Seeds 1 to 20 each give an independent history.

- Groceries: four rows a month of 107.50 dollars, each with uniform noise of 15 dollars either way.
- Dining: 150 dollars a month with uniform noise of 60 dollars either way.
- Electric: 90 dollars times a fixed season (150% in January, 80% in April, 130% in July), with 8 dollars of noise.
- Gifts: 20 dollars a month with 15 dollars of noise, and 400 dollars with 80 dollars of noise in December.
- The household mix adds a 3000-dollar paycheck and 1650-dollar rent (both bill-linked), side income of about 300 dollars in 30% of months, and a monthly card payment.

<!-- generated:start -->

### Stationary: Groceries and Dining

**(a) Baselined net flow of month o + h**

| h | n | MAE | WAPE | Bias | Band coverage | Skill vs last month | Seeds below 0 | Skill vs last year | Skill vs last month, seasonality off |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 32 | $36.00 ($26.78 to $49.43) | 6.24% (4.60% to 8.38%) | $1.92 (-$14.51 to $8.52) | 72% (50% to 88%) | 14.51% (-7.25% to 28.92%) | 3 of 20 | 13.11% (-8.39% to 30.68%) | 16.88% (-1.11% to 28.48%) |
| 3 | 30 | $35.41 ($25.36 to $47.68) | 6.02% (4.35% to 8.06%) | -$0.09 (-$18.93 to $8.96) | 73% (50% to 90%) | 15.99% (-16.52% to 36.24%) | 1 of 20 | 15.30% (-10.59% to 32.26%) | 19.54% (-1.63% to 36.46%) |
| 6 | 27 | $35.36 ($22.29 to $45.44) | 6.08% (3.82% to 7.83%) | $1.47 (-$17.05 to $8.11) | 74% (52% to 89%) | 15.54% (-1.59% to 37.65%) | 1 of 20 | 13.10% (-17.02% to 25.60%) | 15.14% (7.00% to 41.85%) |
| 10 | 23 | $36.48 ($26.41 to $52.48) | 6.39% (4.61% to 9.09%) | -$1.05 (-$18.70 to $10.67) | 65% (43% to 96%) | 14.81% (-7.92% to 36.58%) | 1 of 20 | 8.92% (1.86% to 32.36%) | 17.87% (-2.73% to 32.85%) |
| 12 | 21 | $35.74 ($21.86 to $50.28) | 6.24% (3.81% to 8.62%) | $0.46 (-$17.93 to $13.83) | 71% (38% to 100%) | 21.35% (-15.80% to 35.97%) | 1 of 20 | 15.05% (-19.53% to 33.87%) | 21.85% (-19.81% to 37.28%) |

**(b) Net-worth change over o + 1 to o + h, non-cash changes excluded, bills as perfect foresight**

| h | n | MAE | WAPE | Bias | Band coverage | Skill vs last month | Seeds below 0 | Skill vs last year | Skill vs last month, seasonality off |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 29 | $36.04 ($25.06 to $52.45) | 6.23% (4.29% to 8.87%) | -$0.38 (-$14.51 to $10.08) | 72% (48% to 93%) | 14.35% (-13.71% to 31.11%) | 2 of 20 | 13.11% (-8.39% to 30.68%) | 19.04% (-2.41% to 29.84%) |
| 3 | 27 | $68.62 ($42.49 to $103.40) | 4.04% (2.47% to 5.83%) | $0.32 (-$40.80 to $33.64) | 81% (63% to 100%) | 37.44% (1.96% to 55.51%) | 0 of 20 | 8.93% (-44.56% to 32.41%) | 26.98% (7.72% to 50.29%) |
| 6 | 24 | $101.53 ($51.21 to $170.22) | 2.91% (1.49% to 4.95%) | $1.92 (-$79.46 to $58.15) | 83% (54% to 100%) | 48.51% (1.99% to 69.91%) | 0 of 20 | -9.76% (-72.77% to 36.80%) | 32.74% (9.18% to 59.08%) |
| 10 | 20 | $161.82 ($75.66 to $288.57) | 2.78% (1.32% to 5.05%) | -$16.20 (-$108.89 to $86.90) | 80% (35% to 100%) | 51.43% (17.66% to 71.77%) | 0 of 20 | -27.57% (-74.56% to 1.26%) | 42.64% (16.44% to 66.11%) |
| 12 | 18 | $188.93 ($72.60 to $325.88) | 2.71% (1.04% to 4.77%) | $4.53 (-$156.46 to $114.20) | 78% (44% to 100%) | 54.86% (17.69% to 76.93%) | 0 of 20 | -25.37% (-80.67% to 0.65%) | 46.70% (12.66% to 71.65%) |

### Seasonal: Electric and Gifts

**(a) Baselined net flow of month o + h**

| h | n | MAE | WAPE | Bias | Band coverage | Skill vs last month | Seeds below 0 | Skill vs last year | Skill vs last month, seasonality off |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 32 | $38.01 ($33.97 to $45.15) | 24.48% (22.26% to 28.61%) | -$22.04 (-$29.69 to -$18.14) | 84% (75% to 91%) | 58.56% (54.20% to 62.21%) | 0 of 20 | -86.63% (-268.48% to -1.26%) | 36.41% (34.09% to 39.72%) |
| 3 | 30 | $39.95 ($35.00 to $45.54) | 25.36% (22.09% to 28.51%) | -$22.48 (-$34.53 to -$17.76) | 83% (67% to 90%) | 58.35% (54.62% to 63.29%) | 0 of 20 | -100.74% (-260.47% to 11.52%) | 37.89% (35.86% to 41.79%) |
| 6 | 27 | $44.51 ($38.19 to $48.81) | 27.14% (24.51% to 30.14%) | -$23.78 (-$32.18 to -$17.50) | 81% (74% to 89%) | 53.60% (49.39% to 57.85%) | 0 of 20 | -100.06% (-259.27% to 5.58%) | 30.59% (25.50% to 33.85%) |
| 10 | 23 | $29.53 ($24.36 to $37.70) | 18.76% (16.06% to 24.32%) | -$8.86 (-$15.23 to -$2.32) | 83% (65% to 96%) | 67.13% (58.76% to 71.98%) | 0 of 20 | -141.97% (-306.28% to -4.20%) | 38.79% (33.81% to 43.08%) |
| 12 | 21 | $50.42 ($43.38 to $59.59) | 32.12% (27.92% to 37.73%) | -$31.04 (-$39.73 to -$21.73) | 81% (67% to 86%) | 33.23% (22.93% to 40.43%) | 0 of 20 | -162.06% (-337.80% to -35.60%) | 20.35% (13.58% to 25.53%) |

**(b) Net-worth change over o + 1 to o + h, non-cash changes excluded, bills as perfect foresight**

| h | n | MAE | WAPE | Bias | Band coverage | Skill vs last month | Seeds below 0 | Skill vs last year | Skill vs last month, seasonality off |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 29 | $39.35 ($35.11 to $45.97) | 24.81% (22.51% to 28.27%) | -$24.62 (-$33.58 to -$21.07) | 86% (79% to 90%) | 59.94% (55.98% to 63.70%) | 0 of 20 | -86.63% (-268.48% to -1.26%) | 37.47% (35.14% to 40.40%) |
| 3 | 27 | $109.28 ($86.47 to $127.55) | 23.71% (19.65% to 26.70%) | -$82.49 (-$101.02 to -$59.55) | 74% (67% to 78%) | 59.68% (54.22% to 64.67%) | 0 of 20 | -202.86% (-380.58% to -33.39%) | 46.12% (41.11% to 49.53%) |
| 6 | 24 | $191.77 ($156.16 to $246.66) | 21.19% (17.77% to 25.89%) | -$166.68 (-$190.68 to -$117.19) | 54% (50% to 63%) | 60.99% (54.86% to 63.56%) | 0 of 20 | -388.00% (-647.45% to -47.57%) | 49.14% (43.86% to 52.54%) |
| 10 | 20 | $270.01 ($205.38 to $333.33) | 18.15% (13.63% to 22.02%) | -$249.75 (-$313.52 to -$167.96) | 30% (25% to 75%) | 64.10% (59.14% to 70.45%) | 0 of 20 | -609.17% (-1024.23% to -10.50%) | 56.12% (50.89% to 63.11%) |
| 12 | 18 | $342.33 ($239.80 to $436.38) | 18.64% (13.14% to 23.72%) | -$344.59 (-$436.38 to -$239.80) | 17% (11% to 67%) | 47.96% (39.74% to 61.58%) | 0 of 20 | -647.24% (-1240.93% to 20.17%) | 38.86% (29.16% to 52.18%) |

### Household: all of the above, plus bills, pay, side income and card payments

**(a) Baselined net flow of month o + h**

| h | n | MAE | WAPE | Bias | Band coverage | Skill vs last month | Seeds below 0 | Skill vs last year | Skill vs last month, seasonality off |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 32 | $150.48 ($112.03 to $191.28) | 23.40% (16.40% to 30.20%) | $11.22 (-$29.02 to $46.67) | 75% (59% to 81%) | 30.13% (14.90% to 47.69%) | 0 of 20 | 17.57% (-28.83% to 48.07%) | 24.00% (9.34% to 39.10%) |
| 3 | 30 | $147.74 ($117.19 to $188.51) | 21.89% (17.62% to 30.06%) | $6.84 (-$29.55 to $51.77) | 73% (37% to 87%) | 25.47% (5.57% to 46.97%) | 0 of 20 | 18.97% (-45.91% to 48.29%) | 21.81% (2.98% to 43.66%) |
| 6 | 27 | $150.48 ($122.05 to $194.01) | 23.38% (18.34% to 30.94%) | $14.29 (-$39.09 to $60.05) | 67% (52% to 89%) | 27.89% (-2.41% to 40.24%) | 1 of 20 | 12.66% (-17.76% to 48.35%) | 21.51% (2.78% to 32.47%) |
| 10 | 23 | $137.01 ($103.94 to $184.47) | 21.72% (15.81% to 28.83%) | $15.33 (-$37.82 to $78.80) | 74% (48% to 87%) | 32.97% (4.14% to 45.52%) | 0 of 20 | 12.92% (-6.01% to 50.72%) | 24.32% (-3.25% to 38.46%) |
| 12 | 21 | $154.58 ($104.01 to $196.00) | 24.10% (15.93% to 29.68%) | $0.42 (-$73.94 to $80.41) | 71% (52% to 86%) | 27.89% (-1.55% to 39.18%) | 2 of 20 | 15.88% (-143.04% to 62.01%) | 24.56% (-7.25% to 36.74%) |

**(b) Net-worth change over o + 1 to o + h, non-cash changes excluded, bills as perfect foresight**

| h | n | MAE | WAPE | Bias | Band coverage | Skill vs last month | Seeds below 0 | Skill vs last year | Skill vs last month, seasonality off |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 29 | $147.48 ($112.93 to $191.87) | 20.94% (16.35% to 26.59%) | $22.64 (-$26.15 to $51.84) | 79% (62% to 90%) | 32.66% (15.38% to 48.78%) | 0 of 20 | 17.57% (-28.83% to 48.07%) | 24.39% (12.40% to 40.13%) |
| 3 | 27 | $319.74 ($202.87 to $470.23) | 15.28% (9.71% to 21.87%) | $58.52 (-$65.59 to $172.45) | 81% (41% to 96%) | 43.15% (15.82% to 58.17%) | 0 of 20 | -2.40% (-97.69% to 37.33%) | 38.78% (16.36% to 52.36%) |
| 6 | 24 | $534.00 ($317.50 to $856.48) | 12.58% (7.61% to 18.86%) | $137.26 (-$149.29 to $336.64) | 83% (29% to 100%) | 43.70% (1.94% to 65.19%) | 0 of 20 | -27.77% (-190.72% to 2.20%) | 41.57% (2.23% to 63.52%) |
| 10 | 20 | $832.19 ($431.27 to $1227.02) | 11.46% (6.28% to 16.14%) | $321.62 (-$175.14 to $654.37) | 80% (20% to 100%) | 49.42% (7.57% to 71.65%) | 0 of 20 | -92.35% (-254.04% to 37.08%) | 51.67% (8.28% to 72.08%) |
| 12 | 18 | $1015.14 ($487.51 to $1576.43) | 11.77% (5.92% to 17.33%) | $258.59 (-$391.69 to $909.66) | 83% (6% to 100%) | 44.31% (4.47% to 70.24%) | 0 of 20 | -127.35% (-256.60% to 37.37%) | 47.55% (6.47% to 70.95%) |

<!-- generated:end -->

## Findings

1. **The first backtest's loss to naive was one seed, not the model.** The first run used a single xorshift64* history. Its output stream has no correlated draws: over 200,000 draws of the same `around(1500)` the lag 1 to 24 autocorrelations are all within 0.006 of zero, which is noise at that sample size. Raw xorshift does have one trap for repeated runs: seeds 1, 2, 3 start out linear in the seed (first draws 240, 480, 720 modulo 3001), so the generator is now SplitMix64, whose nearby seeds give unrelated streams. Over 20 seeds, the stationary mix beats "last month repeated" at every horizon. The median is +14.51% at `h = 1` on target (a), with 3 of 20 seeds below 0 and a range of -7.25% to +28.92%. That matches Fred's rerun in FC-0.1 (+15%).
2. **Seasonality costs a stationary series about 2 points and pays on seasonal ones.** With seasonality off, the stationary mix's `h = 1` skill rises from +14.51% to +16.88%. On the seasonal mix it falls from +58.56% to +36.41%, and on the household mix from +30.13% to +24.00%. This supports FC-0.1 keeping `SEASON_MIN` at 13.
3. **Band coverage meets the target on target (b) for stationary and household histories.** At 3 months and beyond the medians are 78% to 83%, inside FC-7's 70 to 90. On target (a) the stationary medians are 65% to 74%, and 65% at `h = 10` misses the target. The household mix's 67% at `h = 6` misses it too. The single-seed ranges are wide (38% to 100%), because one history gives only 18 to 32 origins.
4. **Annual lumps are still under-forecast and under-covered, as FC-0.1 expects.** The seasonal mix has a negative bias at every horizon: it forecasts less spending than happens, by 9 to 31 dollars a month on target (a) depending on the horizon. "Same month last year" beats the model by a wide margin. On target (b) the coverage falls from 86% at one month to 17% at 12. FC-7 doesn't gate this mix on coverage. Its December gifts are better made into a yearly bill.
5. **Accuracy on target (b) improves with the horizon in relative terms.** Stationary WAPE falls from 6.23% at one month to 2.71% at 12. Household WAPE falls from 20.94% to 11.77%, with perfect bill foresight.
