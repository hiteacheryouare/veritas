# Forecast evaluation (FC-7)

This is the backtest that docs/finance/forecasting.md FC-7 asks for, run against the FC-0 constants as they stand after FC-0.1. The tables come from `crates/veritas-forecast/tests/backtest.rs`, which fails when they no longer match the code. Rerun it with `VERITAS_WRITE_FORECAST_EVAL=1` to rewrite them. Everything outside the generated block is written by hand.

## Method

Each origin `o` is a "today" on the 1st of month `o`. The forecast trains on the complete months before `o`, skips `o` itself (no days have passed) and is scored on `o + 1` through `o + h` for `h` in 1, 3, 6, 10 and 12.

- Target (a) is the baselined net flow of the single month `o + h`: baselined income minus baselined spending. Its band is the one-month band, because one month carries one month of error. Origins need 3 complete months before them.
- Target (b) is the net-worth change over `o + 1` through `o + h` without non-cash changes, which is E1 + E2 of statements.md ST-4. Its band is the `h^(3/4)`-scaled band of FC-4.2. Origins need 6 complete months before them.
- Bill-linked rows in the scored months count as known flows. That is perfect foresight of bills, and it flatters target (b). Target (a) has no bills in it.
- "Skill vs last month" compares against the last complete month before `o`, repeated (`h` times for (b)). "Skill vs last year" compares against the same calendar month in the latest year before `o`, scored only on origins with at least 13 months of history.
- Error is `actual - forecast`, so a negative bias means the model forecast more net income than happened: here, it under-forecast spending.
- Every mix runs on 20 seeds. Each cell is the median across seeds (FC-0: with 20 seeds, the two middle values averaged and rounded half away from zero), with the lowest and highest seed in brackets. "Seeds below 0" counts the seeds whose skill vs last month is negative. The last column reruns the same 20 seeds with seasonality off (it never turns on), so the two skill columns compare matching data.

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
| 1 | 32 | $36.01 ($26.78 to $49.43) | 6.27% (4.60% to 8.38%) | $2.22 (-$14.51 to $8.52) | 72% (50% to 88%) | 15.34% (-7.25% to 28.92%) | 3 of 20 | 13.22% (-8.39% to 30.68%) | 17.05% (-1.11% to 28.48%) |
| 3 | 30 | $36.06 ($25.36 to $47.68) | 6.20% (4.35% to 8.06%) | $0.09 (-$18.93 to $8.96) | 73% (50% to 90%) | 17.18% (-16.52% to 36.24%) | 1 of 20 | 15.44% (-10.59% to 32.26%) | 19.67% (-1.63% to 36.46%) |
| 6 | 27 | $35.86 ($22.29 to $45.44) | 6.25% (3.82% to 7.83%) | $1.55 (-$17.05 to $8.11) | 74% (52% to 89%) | 17.92% (-1.59% to 37.65%) | 1 of 20 | 13.62% (-17.02% to 25.60%) | 15.53% (7.00% to 41.85%) |
| 10 | 23 | $37.06 ($26.41 to $52.48) | 6.48% (4.61% to 9.09%) | -$0.28 (-$18.70 to $10.67) | 70% (43% to 96%) | 15.40% (-7.92% to 36.58%) | 1 of 20 | 11.02% (1.86% to 32.36%) | 18.61% (-2.73% to 32.85%) |
| 12 | 21 | $35.89 ($21.86 to $50.28) | 6.25% (3.81% to 8.62%) | $1.20 (-$17.93 to $13.83) | 71% (38% to 100%) | 22.51% (-15.80% to 35.97%) | 1 of 20 | 15.28% (-19.53% to 33.87%) | 24.22% (-19.81% to 37.28%) |

**(b) Net-worth change over o + 1 to o + h, non-cash changes excluded, bills as perfect foresight**

| h | n | MAE | WAPE | Bias | Band coverage | Skill vs last month | Seeds below 0 | Skill vs last year | Skill vs last month, seasonality off |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 29 | $36.33 ($25.06 to $52.45) | 6.32% (4.29% to 8.87%) | -$0.20 (-$14.51 to $10.08) | 74% (48% to 93%) | 16.11% (-13.71% to 31.11%) | 2 of 20 | 13.22% (-8.39% to 30.68%) | 19.19% (-2.41% to 29.84%) |
| 3 | 27 | $69.97 ($42.49 to $103.40) | 4.07% (2.47% to 5.83%) | $0.44 (-$40.80 to $33.64) | 81% (63% to 100%) | 37.66% (1.96% to 55.51%) | 0 of 20 | 8.97% (-44.56% to 32.41%) | 27.07% (7.72% to 50.29%) |
| 6 | 24 | $101.70 ($51.21 to $170.22) | 2.95% (1.49% to 4.95%) | $4.41 (-$79.46 to $58.15) | 83% (54% to 100%) | 49.62% (1.99% to 69.91%) | 0 of 20 | -6.82% (-72.77% to 36.80%) | 33.22% (9.18% to 59.08%) |
| 10 | 20 | $172.67 ($75.66 to $288.57) | 2.97% (1.32% to 5.05%) | -$6.66 (-$108.89 to $86.90) | 83% (35% to 100%) | 51.62% (17.66% to 71.77%) | 0 of 20 | -23.60% (-74.56% to 1.26%) | 43.26% (16.44% to 66.11%) |
| 12 | 18 | $191.09 ($72.60 to $325.88) | 2.76% (1.04% to 4.77%) | $10.40 (-$156.46 to $114.20) | 78% (44% to 100%) | 55.29% (17.69% to 76.93%) | 0 of 20 | -24.47% (-80.67% to 0.65%) | 46.96% (12.66% to 71.65%) |

### Seasonal: Electric and Gifts

**(a) Baselined net flow of month o + h**

| h | n | MAE | WAPE | Bias | Band coverage | Skill vs last month | Seeds below 0 | Skill vs last year | Skill vs last month, seasonality off |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 32 | $38.72 ($33.97 to $45.15) | 24.59% (22.26% to 28.61%) | -$22.01 (-$29.69 to -$18.14) | 84% (75% to 91%) | 58.61% (54.20% to 62.21%) | 0 of 20 | -85.06% (-268.48% to -1.26%) | 36.53% (34.09% to 39.72%) |
| 3 | 30 | $40.10 ($35.00 to $45.54) | 25.43% (22.09% to 28.51%) | -$22.48 (-$34.53 to -$17.76) | 83% (67% to 90%) | 58.50% (54.62% to 63.29%) | 0 of 20 | -98.80% (-260.47% to 11.52%) | 38.26% (35.86% to 41.79%) |
| 6 | 27 | $44.90 ($38.19 to $48.81) | 27.39% (24.51% to 30.14%) | -$23.75 (-$32.18 to -$17.50) | 81% (74% to 89%) | 53.86% (49.39% to 57.85%) | 0 of 20 | -98.79% (-259.27% to 5.58%) | 30.68% (25.50% to 33.85%) |
| 10 | 23 | $29.79 ($24.36 to $37.70) | 19.23% (16.06% to 24.32%) | -$8.84 (-$15.23 to -$2.32) | 85% (65% to 96%) | 67.17% (58.76% to 71.98%) | 0 of 20 | -132.99% (-306.28% to -4.20%) | 39.25% (33.81% to 43.08%) |
| 12 | 21 | $51.06 ($43.38 to $59.59) | 32.67% (27.92% to 37.73%) | -$30.93 (-$39.73 to -$21.73) | 81% (67% to 86%) | 33.53% (22.93% to 40.43%) | 0 of 20 | -159.23% (-337.80% to -35.60%) | 20.41% (13.58% to 25.53%) |

**(b) Net-worth change over o + 1 to o + h, non-cash changes excluded, bills as perfect foresight**

| h | n | MAE | WAPE | Bias | Band coverage | Skill vs last month | Seeds below 0 | Skill vs last year | Skill vs last month, seasonality off |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 29 | $39.37 ($35.11 to $45.97) | 24.81% (22.51% to 28.27%) | -$24.50 (-$33.58 to -$21.07) | 86% (79% to 90%) | 60.06% (55.98% to 63.70%) | 0 of 20 | -85.06% (-268.48% to -1.26%) | 37.58% (35.14% to 40.40%) |
| 3 | 27 | $109.32 ($86.47 to $127.55) | 23.84% (19.65% to 26.70%) | -$81.90 (-$101.02 to -$59.55) | 74% (67% to 78%) | 59.75% (54.22% to 64.67%) | 0 of 20 | -198.95% (-380.58% to -33.39%) | 46.13% (41.11% to 49.53%) |
| 6 | 24 | $193.97 ($156.16 to $246.66) | 21.37% (17.77% to 25.89%) | -$161.97 (-$190.68 to -$117.19) | 54% (50% to 63%) | 61.25% (54.86% to 63.56%) | 0 of 20 | -375.49% (-647.45% to -47.57%) | 49.49% (43.86% to 52.54%) |
| 10 | 20 | $272.88 ($205.38 to $333.33) | 18.16% (13.63% to 22.02%) | -$243.98 (-$313.52 to -$167.96) | 30% (25% to 75%) | 64.11% (59.14% to 70.45%) | 0 of 20 | -570.66% (-1024.23% to -10.50%) | 56.24% (50.89% to 63.11%) |
| 12 | 18 | $343.46 ($239.80 to $436.38) | 18.71% (13.14% to 23.72%) | -$343.46 (-$436.38 to -$239.80) | 20% (11% to 67%) | 48.09% (39.74% to 61.58%) | 0 of 20 | -618.59% (-1240.93% to 20.17%) | 39.48% (29.16% to 52.18%) |

### Household: all of the above, plus bills, pay, side income and card payments

**(a) Baselined net flow of month o + h**

| h | n | MAE | WAPE | Bias | Band coverage | Skill vs last month | Seeds below 0 | Skill vs last year | Skill vs last month, seasonality off |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 32 | $150.77 ($112.03 to $191.28) | 23.48% (16.40% to 30.20%) | $11.54 (-$29.02 to $46.67) | 75% (59% to 81%) | 31.23% (14.90% to 47.69%) | 0 of 20 | 18.62% (-28.83% to 48.07%) | 24.06% (9.34% to 39.10%) |
| 3 | 30 | $148.32 ($117.19 to $188.51) | 22.38% (17.62% to 30.06%) | $9.58 (-$29.55 to $51.77) | 73% (37% to 87%) | 27.55% (5.57% to 46.97%) | 0 of 20 | 21.32% (-45.91% to 48.29%) | 21.86% (2.98% to 43.66%) |
| 6 | 27 | $151.25 ($122.05 to $194.01) | 23.41% (18.34% to 30.94%) | $18.96 (-$39.09 to $60.05) | 69% (52% to 89%) | 28.50% (-2.41% to 40.24%) | 1 of 20 | 13.00% (-17.76% to 48.35%) | 22.27% (2.78% to 32.47%) |
| 10 | 23 | $138.19 ($103.94 to $184.47) | 21.84% (15.81% to 28.83%) | $25.84 (-$37.82 to $78.80) | 74% (48% to 87%) | 33.99% (4.14% to 45.52%) | 0 of 20 | 13.13% (-6.01% to 50.72%) | 24.71% (-3.25% to 38.46%) |
| 12 | 21 | $155.93 ($104.01 to $196.00) | 24.20% (15.93% to 29.68%) | $9.35 (-$73.94 to $80.41) | 71% (52% to 86%) | 28.87% (-1.55% to 39.18%) | 2 of 20 | 16.99% (-143.04% to 62.01%) | 24.77% (-7.25% to 36.74%) |

**(b) Net-worth change over o + 1 to o + h, non-cash changes excluded, bills as perfect foresight**

| h | n | MAE | WAPE | Bias | Band coverage | Skill vs last month | Seeds below 0 | Skill vs last year | Skill vs last month, seasonality off |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 29 | $149.13 ($112.93 to $191.87) | 21.15% (16.35% to 26.59%) | $24.24 (-$26.15 to $51.84) | 79% (62% to 90%) | 32.84% (15.38% to 48.78%) | 0 of 20 | 18.62% (-28.83% to 48.07%) | 24.47% (12.40% to 40.13%) |
| 3 | 27 | $321.25 ($202.87 to $470.23) | 15.33% (9.71% to 21.87%) | $62.80 (-$65.59 to $172.45) | 81% (41% to 96%) | 43.72% (15.82% to 58.17%) | 0 of 20 | 0.09% (-97.69% to 37.33%) | 39.60% (16.36% to 52.36%) |
| 6 | 24 | $550.16 ($317.50 to $856.48) | 13.07% (7.61% to 18.86%) | $154.07 (-$149.29 to $336.64) | 83% (29% to 100%) | 44.92% (1.94% to 65.19%) | 0 of 20 | -27.58% (-190.72% to 2.20%) | 43.26% (2.23% to 63.52%) |
| 10 | 20 | $848.97 ($431.27 to $1227.02) | 11.97% (6.28% to 16.14%) | $337.23 (-$175.14 to $654.37) | 83% (20% to 100%) | 50.60% (7.57% to 71.65%) | 0 of 20 | -79.51% (-254.04% to 37.08%) | 52.01% (8.28% to 72.08%) |
| 12 | 18 | $1021.01 ($487.51 to $1576.43) | 11.83% (5.92% to 17.33%) | $286.18 (-$391.69 to $909.66) | 83% (6% to 100%) | 45.23% (4.47% to 70.24%) | 0 of 20 | -125.89% (-256.60% to 37.37%) | 47.60% (6.47% to 70.95%) |

<!-- generated:end -->

## Findings

1. **The first backtest's loss to naive was one seed, not the model.** The first run used a single xorshift64* history. Its output stream has no correlated draws: over 200,000 draws of the same `around(1500)` the lag 1 to 24 autocorrelations are all within 0.006 of zero, which is noise at that sample size. Raw xorshift does have one trap for repeated runs: seeds 1, 2, 3 start out linear in the seed (first draws 240, 480, 720 modulo 3001), so the generator is now SplitMix64, whose nearby seeds give unrelated streams. Over 20 seeds, the stationary mix beats "last month repeated" at every horizon. The median is +15.34% at `h = 1` on target (a), with 3 of 20 seeds below 0 and a range of -7.25% to +28.92%. That matches Fred's rerun in FC-0.1 (+15%).
2. **Seasonality costs a stationary series about 2 points and pays on seasonal ones.** With seasonality off, the stationary mix's `h = 1` skill rises from +15.34% to +17.05%. On the seasonal mix it falls from +58.61% to +36.53%, and on the household mix from +31.23% to +24.06%. This supports FC-0.1 keeping `SEASON_MIN` at 13.
3. **Band coverage meets the target on target (b) for stationary and household histories.** At 3 months and beyond the medians are 78% to 83%, inside FC-7's 70 to 90. On target (a) the stationary medians are 70% to 74%, just inside the target. The household mix's 69% at `h = 6` misses it. The single-seed ranges are wide (38% to 100%), because one history gives only 18 to 32 origins.
4. **Annual lumps are still under-forecast and under-covered, as FC-0.1 expects.** The seasonal mix has a negative bias at every horizon: it forecasts less spending than happens, by 9 to 31 dollars a month on target (a) depending on the horizon. "Same month last year" beats the model by a wide margin. On target (b) the coverage falls from 86% at one month to 20% at 12. FC-7 doesn't gate this mix on coverage. Its December gifts are better made into a yearly bill.
5. **Accuracy on target (b) improves with the horizon in relative terms.** Stationary WAPE falls from 6.32% at one month to 2.76% at 12. Household WAPE falls from 21.15% to 11.83%, with perfect bill foresight.
