# Third-party notices

Veritas ports algorithms from Actual Budget. Each ported module is listed below
with the Actual source it derives from. Actual is distributed under the following
license.

## Actual Budget

Source: https://github.com/actualbudget/actual (pinned to v26.11.0)

```
Copyright James Long

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

### Ported modules

| Veritas | Actual source |
|---|---|
| `crates/veritas-engine/src/arithmetic.rs` | `packages/loot-core/src/shared/arithmetic.ts`; `currencyToAmount` and `getNumberFormat` in `packages/loot-core/src/shared/util.ts` |
| `crates/veritas-engine/src/amount_field.rs` | `amountToInteger` in `packages/loot-core/src/shared/util.ts`; `forEdit` in `packages/desktop-client/src/hooks/useFormat.ts`; the single-operator rule in `InputValue`, `packages/desktop-client/src/components/table.tsx` |
| `crates/veritas-engine/src/envelope.rs` | `packages/loot-core/src/server/budget/envelope.ts`; `createAllBudgets`, `getBudgetRange` and the `sum-amount` query in `packages/loot-core/src/server/budget/base.ts` |
| `crates/veritas-engine/tests/arithmetic.rs`, `crates/veritas-engine/tests/fixtures/actual-arithmetic-vectors.json` | `packages/loot-core/src/shared/arithmetic.test.ts` (test vectors), and outputs of the Actual code above |
| `crates/veritas-reports/src/filter.rs` | `conditionsToAQL` in `packages/loot-core/src/server/transactions/transaction-rules.ts`; the AQL compiler's null rules in `packages/loot-core/src/server/aql/compiler.ts` |
| `crates/veritas-reports/src/report_query.rs` | `makeQuery` and `getLiveRange` in `packages/desktop-client/src/components/reports` |
| `crates/veritas-reports/src/report_eval.rs` | `createCustomSpreadsheet`, `recalculate`, `filterHiddenItems`, `filterEmptyRows`, `trimIntervals` and `sortData` in `packages/desktop-client/src/components/reports` |
| `crates/veritas-reports/src/custom_report.rs`, `crates/veritas-reports/src/dashboard.rs`, `crates/veritas-reports/src/widget_meta.rs` | the custom report and dashboard models and handlers in `packages/loot-core/src/server/reports/app.ts` and `packages/loot-core/src/server/dashboard/app.ts` |
| `crates/veritas-reports/src/json.rs` | the `JSON.stringify` output Actual's report and widget columns store |
| `crates/veritas-reports/src/widgets/mod.rs` | the card components and spreadsheets in `packages/desktop-client/src/components/reports` |
| `crates/veritas-reports/src/widgets/time_frame.rs` | `calculateTimeRange` and `calculateSpendingReportTimeRange` in `packages/desktop-client/src/components/reports/reportRanges.ts` |
| `crates/veritas-reports/src/widgets/summary.rs` | `packages/desktop-client/src/components/reports/reports/SummaryCard.tsx`, `packages/desktop-client/src/components/reports/spreadsheets/summary-spreadsheet.ts` |
| `crates/veritas-reports/src/widgets/calendar.rs` | `packages/desktop-client/src/components/reports/reports/CalendarCard.tsx`, `packages/desktop-client/src/components/reports/spreadsheets/calendar-spreadsheet.ts` |
| `crates/veritas-reports/src/widgets/age_of_money.rs` | `packages/desktop-client/src/components/reports/reports/AgeOfMoneyCard.tsx`, `packages/desktop-client/src/components/reports/spreadsheets/age-of-money-spreadsheet.ts` |
| `crates/veritas-reports/src/widgets/budget_analysis.rs` | `packages/desktop-client/src/components/reports/reports/BudgetAnalysisCard.tsx`, `packages/desktop-client/src/components/reports/spreadsheets/budget-analysis-spreadsheet.ts` |
| `crates/veritas-reports/src/widgets/crossover.rs` | `packages/desktop-client/src/components/reports/reports/CrossoverCard.tsx`, `packages/desktop-client/src/components/reports/spreadsheets/crossover-spreadsheet.ts` |
| `crates/veritas-reports/src/widgets/cash_flow.rs` | `simpleCashFlow` in `packages/desktop-client/src/components/reports/spreadsheets/cash-flow-spreadsheet.tsx` |
| `crates/veritas-reports/src/widgets/net_worth.rs` | `packages/desktop-client/src/components/reports/reports/NetWorthCard.tsx`, `packages/desktop-client/src/components/reports/spreadsheets/net-worth-spreadsheet.ts`, and the stacked order and colors in `packages/desktop-client/src/components/reports/graphs/NetWorthGraph.tsx` |
| `crates/veritas-reports/src/widgets/spending.rs` | `packages/desktop-client/src/components/reports/reports/SpendingCard.tsx`, `packages/desktop-client/src/components/reports/spreadsheets/spending-spreadsheet.ts`, `packages/desktop-client/src/components/reports/spendingAverageRange.ts` |
| `crates/veritas-reports/src/widgets/sankey.rs` | `packages/desktop-client/src/components/reports/reports/SankeyCard.tsx`, `packages/desktop-client/src/components/reports/spreadsheets/sankey-spreadsheet.ts`, `topNNodes` in `packages/desktop-client/src/components/reports/reports/Sankey.tsx` |
| `crates/veritas-reports/src/widgets/forecast.rs` | the `forecast/generate` handler in `packages/loot-core/src/server/forecast` |
| `crates/veritas-reports/src/widgets/balance_forecast.rs` | `packages/desktop-client/src/components/reports/reports/BalanceForecastCard.tsx`, `packages/desktop-client/src/components/reports/reports/balanceForecastChartData.ts` |
| `crates/veritas-reports/src/widgets/monte_carlo/` (`mod.rs`, `config.rs`, `simulation.rs`, `investment.rs`, `detail.rs`, `history.rs`, and `mulberry32` in `js.rs`) | `packages/desktop-client/src/components/reports/reports/monte-carlo/MonteCarloCard.tsx`, `monteCarloSimulation.ts`, `monteCarloHistoricalReturns.ts` and `useResolvedMonteCarloConfig.ts`; `accountBalance` in `packages/desktop-client/src/spreadsheet/bindings.ts` |
| `crates/veritas-reports/src/widgets/formula/mod.rs`, `crates/veritas-reports/src/widgets/formula/engine.rs` | `packages/desktop-client/src/components/reports/reports/FormulaCard.tsx`, `FormulaResult.tsx`, and the query and budget plumbing of `packages/desktop-client/src/hooks/useFormulaExecution.ts` (not its formula engine, D75) |
| `crates/veritas-reports/src/widgets/formula/months.rs` | `_parse` in `packages/loot-core/src/shared/months.ts` |
| `tools/actual-oracle/src/widgets.ts` (test tooling, not shipped) | verbatim copies of `simpleCashFlow`, the non-React flow of `useFormulaExecution.ts`, `topNNodes` and `getDefaultLayerRange` from `Sankey.tsx`, and the stacked order of `NetWorthGraph.tsx` |

## fdlibm

`crates/veritas-reports/src/widgets/monte_carlo/fdlibm.rs` ports `__ieee754_log`, `__kernel_cos`, `__kernel_sin`, `__ieee754_rem_pio2` and `cos` from fdlibm as V8 adapts it in `src/base/ieee754.cc`, so the Monte Carlo card's random draws match Actual's to the bit. fdlibm's notice, as V8 keeps it:

```
====================================================
Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.

Developed at SunSoft, a Sun Microsystems, Inc. business.
Permission to use, copy, modify, and distribute this
software is freely granted, provided that this notice
is preserved.
====================================================
```

## V8

The same file follows V8's adaptation of fdlibm. "The original source code covered by the above license above has been modified significantly by Google Inc. Copyright 2016 the V8 project authors. All rights reserved." V8 is distributed under the following license.

```
Copyright 2014, the V8 project authors. All rights reserved.
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

    * Redistributions of source code must retain the above copyright
      notice, this list of conditions and the following disclaimer.
    * Redistributions in binary form must reproduce the above
      copyright notice, this list of conditions and the following
      disclaimer in the documentation and/or other materials provided
      with the distribution.
    * Neither the name of Google Inc. nor the names of its
      contributors may be used to endorse or promote products derived
      from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```
