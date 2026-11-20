import type { CanonicalValue } from './canonicalJson.ts';
import type { ActualSession } from './environment.ts';
import { loadSource } from './reference.ts';
import { connectTo, send } from './shims/connection.ts';

// One dashboard card, as the dashboard stores it.
export interface WidgetRequest {
	id: string;
	type: string;
	meta: any;
	firstDayOfWeekIdx?: string;
	budgetType?: string;
	cardHeight?: number;
	captureRunDetail?: number;
}

const REPORTS = 'packages/desktop-client/src/components/reports';

const day = (date: Date) => {
	const pad = (n: number) => String(n).padStart(2, '0');
	return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
};

// Dates become yyyy-MM-dd, and numbers that aren't integers become their JS
// text, since JSON readers may round a float's last digit.
const plain = (value: unknown): CanonicalValue => {
	if (value instanceof Date) {
		return day(value);
	}
	if (typeof value === 'number' && !Number.isInteger(value)) {
		return String(value);
	}
	if (Array.isArray(value)) {
		return value.map(plain);
	}
	if (value && typeof value === 'object') {
		return Object.fromEntries(Object.entries(value).map(([key, entry]) => [key, plain(entry)]));
	}
	return (value ?? null) as CanonicalValue;
};

// Runs a spreadsheet the way useReport does: it calls back with its data.
const run = async (spreadsheet: (sheet: unknown, setData: (data: unknown) => void) => Promise<void>) => {
	let data: unknown = null;
	await spreadsheet(null, result => {
		data = result;
	});
	return data;
};

// NetWorthGraph.tsx:295-325, which Node can't load as TSX: the accounts with
// a balance, by their balances summed over the points, smallest first, each
// colored by its place in getColorScale('qualitative').
const stackedAccounts = (accounts: Array<{ id: string; name: string }>, points: Array<Record<string, unknown>>) => {
	const totals: Record<string, number> = {};
	for (const account of accounts) {
		totals[account.id] = points.reduce((sum, point) => sum + (Number(point[account.id]) || 0), 0);
	}
	return [...accounts]
		.sort((a, b) => totals[a.id] - totals[b.id])
		.map((account, index) => ({
			id: account.id,
			name: account.name,
			points: points.map(point => point[account.id]),
			color: `var(--color-chartQual${(index % 9) + 1})`
		}));
};

// The numbers each card shows, from Actual's own card and spreadsheet code
// for the same meta, as of the oracle's fixed today.
export const widgetData = async ({ internal }: ActualSession, requests: WidgetRequest[]) => {
	connectTo((name, args) => internal.send(name, args));
	const months = await loadSource('packages/loot-core/src/shared/months.ts');
	const ranges = await loadSource(`${REPORTS}/reportRanges.ts`);
	const latest = await internal.send('get-latest-transaction');
	const latestDay = latest ? latest.date : months.currentDay();
	const fullRange = (meta: any) =>
		ranges.calculateTimeRange(
			meta?.timeFrame,
			{ start: months.dayFromDate(months.currentMonth()), end: months.currentDay(), mode: 'full' },
			latestDay
		);

	const cards: Record<string, (request: WidgetRequest) => Promise<unknown>> = {
		'summary-card': async ({ meta }) => {
			const { summarySpreadsheet } = await loadSource(`${REPORTS}/spreadsheets/summary-spreadsheet.ts`);
			const [start, end] = fullRange(meta);
			let content = { type: 'sum' };
			if (meta?.content) {
				try {
					content = JSON.parse(meta.content);
				} catch {
					content = { type: 'sum' };
				}
			}
			return { start, end, data: await run(summarySpreadsheet(start, end, meta?.conditions, meta?.conditionsOp, content, undefined)) };
		},
		// simpleCashFlow, verbatim apart from running its two queries directly, since
		// node can't load the .tsx file it lives in
		// (reference/actual/packages/desktop-client/src/components/reports/spreadsheets/cash-flow-spreadsheet.tsx:18-72).
		'cash-flow-card': async ({ meta }) => {
			const [startMonth, endMonth] = ranges.calculateTimeRange(
				meta?.timeFrame,
				{ start: months.currentMonth(), end: months.currentMonth(), mode: 'sliding-window' },
				latestDay
			);
			const start = months.firstDayOfMonth(startMonth);
			const end = months.lastDayOfMonth(endMonth);
			const { filters } = await internal.send('make-filters-from-conditions', {
				conditions: (meta?.conditions ?? []).filter((cond: any) => !cond.customName)
			});
			const conditionsOpKey = meta?.conditionsOp === 'or' ? '$or' : '$and';
			const { q } = await import('@actual-app/api');
			const makeQuery = () =>
				q('transactions')
					.filter({ [conditionsOpKey]: filters })
					.filter({
						$and: [{ date: { $gte: start } }, { date: { $lte: end > months.currentDay() ? months.currentDay() : end } }],
						'account.offbudget': false,
						'payee.transfer_acct': null
					})
					.calculate({ $sum: '$amount' });
			const sum = async (query: any) => (await internal.send('query', query.serialize())).data;
			return {
				start: startMonth,
				end: endMonth,
				income: await sum(makeQuery().filter({ amount: { $gt: 0 } })),
				expense: await sum(makeQuery().filter({ amount: { $lt: 0 } }))
			};
		},
		// SpendingCard.tsx's range, spreadsheet and headline difference. A request's
		// budgetType stands in for the synced pref the card reads.
		'spending-card': async ({ meta, budgetType }) => {
			const { createSpendingSpreadsheet } = await loadSource(`${REPORTS}/spreadsheets/spending-spreadsheet.ts`);
			const { normalizeSpendingAverageRange } = await loadSource(`${REPORTS}/spendingAverageRange.ts`);
			const stored = await internal.db.first("SELECT value FROM preferences WHERE id = 'budgetType'");
			const type = (budgetType ?? stored?.value) === 'tracking' ? 'tracking' : 'envelope';
			const mode = meta?.mode ?? 'single-month';
			const [compare, compareTo] = ranges.calculateSpendingReportTimeRange(meta ?? {});
			const data: any = await run(
				createSpendingSpreadsheet({
					conditions: meta?.conditions,
					conditionsOp: meta?.conditionsOp,
					compare,
					compareTo,
					averageRange: normalizeSpendingAverageRange(meta?.averageRange),
					budgetType: type
				})
			);
			const selection = mode === 'single-month' ? 'compareTo' : mode;
			const todayDay =
				compare !== months.currentMonth()
					? 27
					: months.getDay(months.currentDay()) - 1 >= 28
						? 27
						: months.getDay(months.currentDay()) - 1;
			const difference = data && Math.round(data.intervalData[todayDay][selection] - data.intervalData[todayDay].compare);
			return { compare, compareTo, difference, data };
		},
		// NetWorthCard.tsx's range and spreadsheet, plus the stacked mode's
		// accounts in NetWorthGraph.tsx's order and colors (stackedAccounts below). The card's labels need a locale
		// and formatter, so only the numbers come back.
		'net-worth-card': async ({ meta, firstDayOfWeekIdx }) => {
			const { createSpreadsheet } = await loadSource(`${REPORTS}/spreadsheets/net-worth-spreadsheet.ts`);
			const [start, end] = ranges.calculateTimeRange(meta?.timeFrame, undefined, latestDay);
			const accounts = await internal.send('accounts-get');
			const data: any = await run(
				createSpreadsheet(
					start,
					end,
					accounts,
					meta?.conditions,
					meta?.conditionsOp,
					undefined,
					meta?.interval || 'Monthly',
					firstDayOfWeekIdx || '0',
					(value: unknown) => String(value),
					'MM/dd/yyyy'
				)
			);
			return {
				start,
				end,
				data: {
					points: data.graphData.data.map((point: any) => point.y),
					hasNegative: data.graphData.hasNegative,
					startDate: data.graphData.start,
					endDate: data.graphData.end,
					netWorth: data.netWorth,
					totalChange: data.totalChange,
					lowestNetWorth: data.lowestNetWorth,
					highestNetWorth: data.highestNetWorth,
					accounts: data.accounts.map((account: any) => account.id),
					details: data.graphData.data.map((point: any) => ({
						assets: Number(point.assets),
						debt: -Number(point.debt),
						change: Number(point.change)
					})),
					stacked: stackedAccounts(data.accounts, data.graphData.data)
				}
			};
		},
		// SankeyCard.tsx's range, category list and graph. topNNodes and
		// getDefaultLayerRange are Sankey.tsx's, which Node can't load as TSX; a
		// request without cardHeight stands for a card tall enough for any topN.
		'sankey-card': async ({ meta, cardHeight }) => {
			const sankey = await loadSource(`${REPORTS}/spreadsheets/sankey-spreadsheet.ts`);
			const earliest = await internal.send('get-earliest-transaction');
			const earliestDay = earliest ? earliest.date : months.currentDay();
			const [start, end] = ranges.calculateTimeRange(meta?.timeFrame, undefined, latestDay);
			const [boundedStart, boundedEnd] = ranges.boundMonthRangeFromDates(earliestDay, latestDay, start, end);
			const { grouped } = await internal.send('get-categories');
			const categories = grouped.map((group: any) => ({
				...group,
				categories: group.categories?.map((cat: any) => ({
					...cat,
					name: cat.name?.toLowerCase() === 'starting balances' ? 'Starting Balances' : cat.name
				}))
			}));
			const mode = meta?.mode ?? 'spent';
			const baseGraph = await run(
				sankey.createBaseGraphSpreadsheet(
					boundedStart,
					boundedEnd,
					categories,
					meta?.conditions ?? [],
					meta?.conditionsOp ?? 'and',
					mode,
					meta?.groupAccounts ?? false,
					meta?.showTransfers ?? false
				)
			);
			const heightBasedTopN = cardHeight === undefined ? Infinity : Math.max(2, Math.floor(cardHeight / 35));
			const topN = Math.min(meta?.topNcategories ?? 15, heightBasedTopN);
			const layered = sankey.isGraphLayer(meta?.layerFrom) && sankey.isGraphLayer(meta?.layerTo);
			const layerFrom = layered ? meta.layerFrom : mode === 'budgeted' ? 'income_category' : 'payee';
			const layerTo = layered ? meta.layerTo : 'category';
			const data = sankey.buildSankeyData(baseGraph, topN, categories, meta?.categorySort ?? 'per-group', layerFrom, layerTo);
			return { start, end, boundedStart, boundedEnd, data };
		},
		'calendar-card': async ({ meta, firstDayOfWeekIdx }) => {
			const { calendarSpreadsheet } = await loadSource(`${REPORTS}/spreadsheets/calendar-spreadsheet.ts`);
			const [start, end] = fullRange(meta);
			return {
				start,
				end,
				data: await run(calendarSpreadsheet(start, end, meta?.conditions, meta?.conditionsOp, firstDayOfWeekIdx))
			};
		},
		'age-of-money-card': async ({ meta }) => {
			const { createAgeOfMoneySpreadsheet } = await loadSource(`${REPORTS}/spreadsheets/age-of-money-spreadsheet.ts`);
			const [start, end] = ranges.calculateTimeRange(meta?.timeFrame, undefined, latestDay);
			const spreadsheet = createAgeOfMoneySpreadsheet({
				start,
				end,
				conditions: meta?.conditions,
				conditionsOp: meta?.conditionsOp,
				granularity: meta?.granularity ?? 'monthly'
			});
			return { start, end, data: await run(spreadsheet) };
		},
		'budget-analysis-card': async ({ meta }) => {
			const { createBudgetAnalysisSpreadsheet } = await loadSource(`${REPORTS}/spreadsheets/budget-analysis-spreadsheet.ts`);
			const timeFrame = meta?.timeFrame ?? {
				start: months.subMonths(months.currentMonth(), 5),
				end: months.currentMonth(),
				mode: 'sliding-window'
			};
			const [startMonth, endMonth] = ranges.calculateTimeRange(timeFrame);
			const startDate = months.monthFromDate(startMonth) + '-01';
			const endDate = months.getMonthEnd(months.monthFromDate(endMonth) + '-01');
			const spreadsheet = createBudgetAnalysisSpreadsheet({
				conditions: meta?.conditions,
				conditionsOp: meta?.conditionsOp,
				startDate,
				endDate,
				showHiddenCategories: meta?.showHiddenCategories ?? false
			});
			return { start: startDate, end: endDate, data: await run(spreadsheet) };
		},
		// BalanceForecastCard.tsx's source, range and request, the server's
		// forecast/generate, and the card's chart data. A request's budgetType
		// stands in for the synced pref the card reads.
		'forecast-card': async ({ meta, budgetType }) => {
			const chart = await loadSource(`${REPORTS}/reports/balanceForecastChartData.ts`);
			const stored = await internal.db.first("SELECT value FROM preferences WHERE id = 'budgetType'");
			const type = (budgetType ?? stored?.value) === 'tracking' ? 'tracking' : 'envelope';
			const source = meta?.source === 'tracking-budget' && type === 'tracking' ? 'tracking-budget' : 'schedules';
			const tracking = source === 'tracking-budget';
			const granularity = meta?.granularity || 'Monthly';
			const defaultTimeFrame = {
				start: months.currentMonth(),
				end: months.addMonths(months.currentMonth(), 11),
				mode: 'static'
			};
			const [start, end] = ranges.calculateTimeRange(meta?.timeFrame, defaultTimeFrame);
			const accounts = await internal.send('accounts-get');
			const request = Object.fromEntries(
				Object.entries({
					accountIds: tracking ? undefined : (meta?.accounts ?? accounts.map((a: any) => a.id)),
					conditions: tracking ? undefined : meta?.conditions,
					conditionsOp: tracking ? undefined : meta?.conditionsOp,
					startDate: months.firstDayOfMonth(start),
					endDate: months.lastDayOfMonth(end),
					includeAccountlessSchedules: tracking ? undefined : meta?.accounts === undefined,
					source
				}).filter(([, value]) => value !== undefined)
			);
			const forecast = await internal.send('forecast/generate', request);
			if (forecast?.error) {
				throw new Error(forecast.error.message ?? String(forecast.error));
			}
			const args = { forecastData: forecast, start, end, granularity };
			const points = chart.buildBalanceForecastChartData(args);
			return {
				start,
				end,
				source,
				forecast,
				points,
				scheduled: chart.countForecastScheduledOccurrences(args),
				lowest: chart.getLowestChartDataPoint(points) ?? null
			};
		},
		// MonteCarloCard.tsx's run, with each linked pot resolved the way
		// useResolvedMonteCarloConfig.ts resolves it, from the accountBalance
		// binding's own query (spreadsheet/bindings.ts:23-31). A request's
		// captureRunDetail records one run, as the report page asks for.
		'monte-carlo-card': async ({ meta, captureRunDetail }) => {
			const engine = await loadSource(`${REPORTS}/reports/monte-carlo/monteCarloSimulation.ts`);
			const { q } = await import('@actual-app/api');
			const config = engine.monteCarloConfigFromMeta(meta);
			const balances: Record<string, number | null> = {};
			for (const accountId of new Set<string>(config.pots.map((pot: any) => pot.accountId).filter((id: unknown) => id != null))) {
				const query = q('transactions').filter({ account: accountId }).options({ splits: 'none' }).calculate({ $sum: '$amount' });
				const { data } = await internal.send('query', query.serialize());
				balances[accountId] = typeof data === 'number' ? data : null;
			}
			const resolved = {
				...config,
				pots: config.pots.map((pot: any) => {
					const balance = pot.accountId != null ? balances[pot.accountId] : null;
					return balance != null ? { ...pot, startingBalance: Math.max(0, balance) } : pot;
				})
			};
			const result = engine.runMonteCarloSimulation({
				...resolved,
				horizonYears: engine.getMonteCarloHorizonYears(resolved),
				deflateToTodaysMoney: true,
				...(captureRunDetail != null && { captureRunDetail })
			});
			return {
				balances,
				successPercent: Math.round(result.successRate * 1000) / 10,
				startAge: config.currentAge,
				endAge: config.currentAge + result.horizonYears,
				result: {
					...result,
					endingBalances: Array.from(result.endingBalances),
					depletionYearBySimulation: Array.from(result.depletionYearBySimulation),
					totalWithdrawnBySimulation: Array.from(result.totalWithdrawnBySimulation)
				}
			};
		},
		// CustomReport.tsx's createCustomSpreadsheet call over a fixed range, with
		// the categories, payees and accounts its hooks load. What comes back is
		// what the graph and its legend draw: the legend's entries in order and the
		// keys of each interval's stacked amounts.
		'custom-report': async ({ meta, firstDayOfWeekIdx }) => {
			const { createCustomSpreadsheet } = await loadSource(`${REPORTS}/spreadsheets/custom-spreadsheet.ts`);
			const data: any = await run(
				createCustomSpreadsheet({
					startDate: meta.startDate,
					endDate: meta.endDate,
					interval: meta.interval,
					categories: await internal.send('get-categories'),
					conditions: meta.conditions ?? [],
					conditionsOp: meta.conditionsOp ?? 'and',
					showEmpty: meta.showEmpty,
					showOffBudget: meta.showOffBudget,
					showHiddenCategories: meta.showHiddenCategories,
					showUncategorized: meta.showUncategorized,
					trimIntervals: meta.trimIntervals,
					groupBy: meta.groupBy,
					balanceTypeOp: meta.balanceTypeOp,
					sortByOp: meta.sortByOp,
					payees: await internal.send('payees-get'),
					accounts: await internal.send('accounts-get'),
					graphType: meta.graphType,
					firstDayOfWeekIdx: firstDayOfWeekIdx || '0',
					dateFormat: 'yyyy-MM-dd'
				})
			);
			const fixed = new Set([
				'date',
				'intervalStartDate',
				'intervalEndDate',
				'totalAssets',
				'totalDebts',
				'netAssets',
				'netDebts',
				'totalTotals',
				'totalBudgeted'
			]);
			return {
				legend: data.legend.map((entry: any) => ({ id: entry.id, name: entry.name, dataKey: entry.dataKey })),
				stacked: data.intervalData.map((interval: any) =>
					Object.fromEntries(Object.entries(interval).filter(([key]) => !fixed.has(key)))
				)
			};
		},
		// FormulaCard.tsx's defaults and useFormulaExecution's evaluation; see
		// executeFormula below.
		'formula-card': async ({ meta }) => {
			const formula = meta?.formula || '=SUM(1, 2, 3)';
			const accounts = await internal.send('accounts-get');
			const simpleAccounts = accounts
				.filter((account: any) => !account.tombstone)
				.map((account: any) => ({ id: account.id, name: account.name }));
			const { result, error } = await executeFormula(formula, meta?.queries ?? {}, undefined, simpleAccounts);
			return { value: result, type: result === null ? 'null' : typeof result, error };
		},
		// CrossoverCard.tsx's range effect and props; defaultTimeFrame is
		// Crossover.tsx's, which Node can't load as TSX.
		'crossover-card': async ({ meta }) => {
			const { createCrossoverSpreadsheet } = await loadSource(`${REPORTS}/spreadsheets/crossover-spreadsheet.ts`);
			const currentMonth = months.currentMonth();
			const previousMonth = months.subMonths(currentMonth, 1);
			const defaultTimeFrame = { start: months.subMonths(currentMonth, 120), end: previousMonth, mode: 'full' };
			const earliestData = await internal.send('get-earliest-transaction');
			const earliestDate = earliestData ? earliestData.date : months.firstDayOfMonth(previousMonth);
			const allMonths = months.rangeInclusive(earliestDate, months.lastDayOfMonth(previousMonth)).reverse();
			const [initialStart, initialEnd, mode] = ranges.calculateTimeRange(meta?.timeFrame, defaultTimeFrame, previousMonth);
			const earliestMonth = allMonths[allMonths.length - 1];
			const latestMonth = allMonths[0];
			const clampMonth = (m: string) =>
				months.isBefore(m, earliestMonth) ? earliestMonth : months.isAfter(m, latestMonth) ? latestMonth : m;
			let start = initialStart;
			let end = initialEnd;
			if (mode === 'sliding-window') {
				start = clampMonth(months.subMonths(start, 1));
				end = clampMonth(months.subMonths(end, 1));
			} else if (mode === 'full') {
				start = earliestMonth;
				end = latestMonth;
			} else {
				start = clampMonth(start);
				end = clampMonth(end);
			}
			if (months.isBefore(end, start)) {
				end = start;
			}
			const { list } = await internal.send('get-categories');
			const showHidden = meta?.showHiddenCategories ?? false;
			const stored = meta?.expenseCategoryIds;
			const expenseCategoryIds = (stored !== undefined ? list.filter((c: any) => stored.includes(c.id)) : list.filter((c: any) => !c.is_income))
				.filter((c: any) => showHidden || !c.hidden)
				.map((c: any) => c.id);
			const accounts = await internal.send('accounts-get');
			const spreadsheet = createCrossoverSpreadsheet({
				start,
				end,
				expenseCategoryIds,
				incomeAccountIds: meta?.incomeAccountIds ?? accounts.map((a: any) => a.id),
				safeWithdrawalRate: meta?.safeWithdrawalRate ?? 0.04,
				estimatedReturn: meta?.estimatedReturn ?? null,
				expectedContribution: meta?.expectedContribution ?? null,
				projectionType: meta?.projectionType ?? 'hampel',
				expenseAdjustmentFactor: meta?.expenseAdjustmentFactor ?? 1.0
			});
			return { start, end, data: await run(spreadsheet) };
		}
	};

	const results: Record<string, CanonicalValue> = {};
	for (const request of requests) {
		// A budget edit ahead of the cards that read it, made through the handler
		// the budget screen calls; the sheet recomputes on a later tick.
		if (request.type === 'budget-amount') {
			await internal.send('budget/budget-amount', request.meta);
			await new Promise(resolve => setTimeout(resolve, 0));
			continue;
		}
		// A bill made the way the schedule editor saves one.
		if (request.type === 'schedule-create') {
			await internal.send('schedule/create', request.meta);
			continue;
		}
		// Switches the budget type the way a synced preference change does. Sync
		// starts setType without awaiting it (loot-core/src/server/sync/index.ts:552),
		// so this waits until the month's numbers stop changing.
		if (request.type === 'budget-type') {
			await internal.send('preferences/save', { id: 'budgetType', value: request.meta.type });
			const month = months.currentMonth();
			let previous = '';
			for (let tries = 0; tries < 200; tries++) {
				await new Promise(resolve => setTimeout(resolve, 10));
				const current = JSON.stringify(await internal.send('api/budget-month', { month }));
				if (current === previous) {
					break;
				}
				previous = current;
			}
			continue;
		}
		const card = cards[request.type];
		if (!card) {
			throw new Error(`The oracle has no card for ${request.type}`);
		}
		try {
			results[request.id] = plain(await card(request));
		} catch (error) {
			results[request.id] = { error: (error as Error).message };
		}
	}
	return results;
};

// Copied from Actual v26.11.0 (commit 1766241e3f4b3c578e424c4fa15f1eb83c0da17d,
// MIT), packages/desktop-client/src/hooks/useFormulaExecution.ts: the hook's
// non-React flow, verbatim apart from returning {result, error} where the hook
// calls setResult and setError, and pinning the locale to en-US, since Node
// can't run the hook. HyperFormula is bootstrapped the way
// bootstrapHyperFormula does it. HyperFormula itself (GPL-3.0-only) runs only
// here in the test oracle, from its npm package; none of it ships in Veritas.
let formulaModules: any = null;
const loadFormulaModules = async () => {
	if (formulaModules) {
		return formulaModules;
	}
	const { HyperFormula } = await import('hyperformula');
	const { default: enUS } = await import('hyperformula/i18n/languages/enUS');
	const custom = await loadSource('packages/loot-core/src/shared/formulas/customFunctions.ts');
	if (!HyperFormula.getRegisteredLanguagesCodes().includes('enUS')) {
		HyperFormula.registerLanguage('enUS', enUS);
	}
	HyperFormula.registerFunctionPlugin(custom.CustomFunctionsPlugin, custom.customFunctionsTranslations);
	formulaModules = {
		HyperFormula,
		custom,
		monthUtils: await loadSource('packages/loot-core/src/shared/months.ts'),
		util: await loadSource('packages/loot-core/src/shared/util.ts'),
		query: await loadSource('packages/loot-core/src/shared/query.ts'),
		ranges: await loadSource(`${REPORTS}/reportRanges.ts`),
		queryTimeFrame: await loadSource('packages/desktop-client/src/components/formula/queryTimeFrame.ts')
	};
	return formulaModules;
};

function createFormulaQueryContext() {
	return {
		queryNames: new Set(),
		queryCountNames: new Set(),
		queryExtractCategoryNames: new Set(),
		queryExtractTimeframeStartNames: new Set(),
		queryExtractTimeframeEndNames: new Set(),
		balanceOfNames: new Set(),
		budgetQueryRequests: new Map(),
		querySumPrefetch: new Map(),
		queryCountPrefetch: new Map(),
		queryExtractCategoriesPrefetch: new Map(),
		queryExtractTimeframeStartPrefetch: new Map(),
		queryExtractTimeframeEndPrefetch: new Map(),
		balanceOfPrefetch: new Map(),
		budgetQueryPrefetch: new Map(),
		budgetQueryErrors: new Map()
	};
}

function isHyperFormulaError(cellValue: unknown): cellValue is { type: string; message?: string } {
	return Boolean(cellValue && typeof cellValue === 'object' && 'type' in cellValue);
}

function evaluateFormulaWithContext({ formula, formulaQueryContext, locale, namedExpressions, throwOnCellError = true }: any) {
	const { HyperFormula } = formulaModules;
	let hfInstance: any = null;

	try {
		hfInstance = HyperFormula.buildEmpty({
			licenseKey: 'gpl-v3',
			language: 'enUS',
			localeLang: typeof locale === 'string' ? locale : 'en-US',
			dateFormats: ['DD/MM/YYYY', 'YYYY-MM-DD', 'YYYY/MM/DD'],
			context: {
				formulaQuery: formulaQueryContext
			}
		});

		const sheetName = hfInstance.addSheet('Sheet1');
		const sheetId = hfInstance.getSheetId(sheetName);

		if (sheetId === undefined) {
			throw new Error('Failed to create sheet');
		}

		if (namedExpressions) {
			for (const [name, value] of Object.entries(namedExpressions)) {
				hfInstance.addNamedExpression(name, typeof value === 'number' ? value : String(value));
			}
		}

		hfInstance.setCellContents({ sheet: sheetId, col: 0, row: 0 }, [[formula]]);

		const cellValue = hfInstance.getCellValue({
			sheet: sheetId,
			col: 0,
			row: 0
		});

		if (isHyperFormulaError(cellValue)) {
			if (throwOnCellError) {
				throw new Error(`Formula error: ${cellValue.type}`);
			}
			return null;
		}

		return cellValue;
	} finally {
		hfInstance?.destroy();
	}
}

async function executeFormula(formula: string, currentQueries: any, currentNamedExpressions: any, currentAccounts: any) {
	await loadFormulaModules();
	const { custom } = formulaModules;
	const language = undefined;
	const locale = 'en-US';
	if (!formula || !formula.startsWith('=')) {
		return { result: null, error: 'Formula must start with =' };
	}

	try {
		const browserLocale = undefined;
		const formulaLocale = language || browserLocale || locale || 'en-US';

		try {
			custom.setCachedUserPreferences(
				await send('formula-load-user-preferences', {
					selectedLocale: language,
					browserLocale
				})
			);
		} catch (err) {
			console.error('Error loading formula preferences:', err);
		}

		const formulaQueryContext = createFormulaQueryContext();

		evaluateFormulaWithContext({
			formula,
			formulaQueryContext,
			locale: formulaLocale,
			namedExpressions: currentNamedExpressions,
			throwOnCellError: false
		});

		await prefetchFormulaQueries(formulaQueryContext, currentQueries);
		await prefetchAccountBalances(formulaQueryContext, currentAccounts ?? []);

		formulaQueryContext.budgetQueryRequests.clear();
		evaluateFormulaWithContext({
			formula,
			formulaQueryContext,
			locale: formulaLocale,
			namedExpressions: currentNamedExpressions,
			throwOnCellError: false
		});

		await prefetchBudgetQueries(formulaQueryContext);

		const cellValue = evaluateFormulaWithContext({
			formula,
			formulaQueryContext,
			locale: formulaLocale,
			namedExpressions: currentNamedExpressions
		});

		return { result: cellValue, error: null };
	} catch (err) {
		return { result: null, error: err instanceof Error ? err.message : 'Unknown error' };
	}
}

async function prefetchFormulaQueries(formulaQueryContext: any, queries: any) {
	const { util } = formulaModules;
	for (const queryName of formulaQueryContext.queryNames) {
		const queryConfig = queries[queryName];

		if (!queryConfig) {
			console.warn(`Query "${queryName}" not found in queries config`);
			formulaQueryContext.querySumPrefetch.set(queryName, 0);
			continue;
		}

		const data = await fetchQuerySum(queryConfig);
		formulaQueryContext.querySumPrefetch.set(queryName, util.integerToAmount(data, 2));
	}

	for (const queryName of formulaQueryContext.queryCountNames) {
		const queryConfig = queries[queryName];

		if (!queryConfig) {
			console.warn(`Query "${queryName}" not found in queries config`);
			formulaQueryContext.queryCountPrefetch.set(queryName, 0);
			continue;
		}

		formulaQueryContext.queryCountPrefetch.set(queryName, await fetchQueryCount(queryConfig));
	}

	for (const queryName of formulaQueryContext.queryExtractCategoryNames) {
		formulaQueryContext.queryExtractCategoriesPrefetch.set(queryName, await extractQueryCategories(queryName, queries));
	}

	for (const queryName of formulaQueryContext.queryExtractTimeframeStartNames) {
		formulaQueryContext.queryExtractTimeframeStartPrefetch.set(queryName, await extractQueryTimeframeStart(queryName, queries));
	}

	for (const queryName of formulaQueryContext.queryExtractTimeframeEndNames) {
		formulaQueryContext.queryExtractTimeframeEndPrefetch.set(queryName, await extractQueryTimeframeEnd(queryName, queries));
	}
}

async function prefetchAccountBalances(formulaQueryContext: any, accounts: any[]) {
	for (const literal of formulaQueryContext.balanceOfNames) {
		const account = accounts.find(candidate => candidate.id === literal) ?? accounts.find(candidate => candidate.name === literal);

		if (!account) {
			formulaQueryContext.balanceOfPrefetch.set(literal, 0);
			continue;
		}

		formulaQueryContext.balanceOfPrefetch.set(literal, await fetchAccountBalance(account.id));
	}
}

async function fetchAccountBalance(accountId: string): Promise<number> {
	const { query, util } = formulaModules;
	try {
		const balanceQuery = query.q('transactions').filter({ account: accountId }).calculate({ $sum: '$amount' });
		const { data } = await send('query', balanceQuery.serialize());
		return util.integerToAmount(data || 0, 2);
	} catch (err) {
		console.error('Error fetching account balance:', err);
		return 0;
	}
}

async function prefetchBudgetQueries(formulaQueryContext: any) {
	const { custom } = formulaModules;
	for (const request of formulaQueryContext.budgetQueryRequests.values()) {
		const key = custom.createBudgetQueryPrefetchKey(request);

		try {
			formulaQueryContext.budgetQueryPrefetch.set(
				key,
				await fetchBudgetDimensionValueDirect(request.dimension, request.categoryIds, request.startMonth, request.endMonth)
			);
			formulaQueryContext.budgetQueryErrors.delete(key);
		} catch (err) {
			console.error('Error evaluating BUDGET_QUERY', err);
			formulaQueryContext.budgetQueryPrefetch.delete(key);
			formulaQueryContext.budgetQueryErrors.set(key, err instanceof Error ? err.message : String(err));
		}
	}
}

async function buildFilteredTransactionsQuery(config: any) {
	const { query, ranges, queryTimeFrame } = formulaModules;
	const conditions = config.conditions || [];
	const conditionsOp = config.conditionsOp || 'and';
	const timeFrame = config.timeFrame;

	const { filters: queryFilters } = await send('make-filters-from-conditions', {
		conditions
	});

	const conditionsOpKey = conditionsOp === 'or' ? '$or' : '$and';

	let transQuery = query.q('transactions');

	if (timeFrame && timeFrame.mode) {
		const [calculatedStart, calculatedEnd] = ranges.calculateTimeRange(ranges.asMonthSlidingTimeFrame(timeFrame));
		const startDate = queryTimeFrame.normalizeQueryTimeFrameStart(calculatedStart);
		const endDate = queryTimeFrame.normalizeQueryTimeFrameEnd(calculatedEnd);

		transQuery = transQuery.filter({
			$and: [{ date: { $gte: startDate } }, { date: { $lte: endDate } }]
		});
	}

	if (queryFilters.length > 0) {
		transQuery = transQuery.filter({ [conditionsOpKey]: queryFilters });
	}

	return transQuery;
}

async function fetchQuerySum(config: any): Promise<number> {
	try {
		const transQuery = await buildFilteredTransactionsQuery(config);
		const summedQuery = transQuery.calculate({ $sum: '$amount' });
		const { data } = await send('query', summedQuery.serialize());
		return data || 0;
	} catch (err) {
		console.error('Error fetching query sum:', err);
		return 0;
	}
}

async function fetchQueryCount(config: any): Promise<number> {
	try {
		const transQuery = await buildFilteredTransactionsQuery(config);
		const countQuery = transQuery.calculate({ $count: '*' });
		const { data } = await send('query', countQuery.serialize());
		return data || 0;
	} catch (err) {
		console.error('Error fetching query count:', err);
		return 0;
	}
}

function extractCategoryConditions(conditions: any[]) {
	return conditions.filter(cond => !cond.customName && (cond.field === 'category' || cond.field === 'category_group'));
}

async function getCategoriesFromConditions(allCategories: any[], conditions: any[], conditionsOp: 'and' | 'or'): Promise<string[]> {
	if (conditions.length === 0) {
		return allCategories.filter((cat: any) => !cat.is_income && !cat.hidden).map((cat: any) => cat.id);
	}

	const { grouped: categoryGroups } = await send('get-categories');
	const groupNameById = new Map(categoryGroups.map((g: { id: string; name: string }) => [g.id, g.name]));

	const conditionResults = conditions.map(cond => {
		const getKey = (cat: any) => (cond.field === 'category_group' ? cat.group : cat.id);

		const matching = allCategories.filter((cat: any) => {
			const key = getKey(cat);

			const textValue = cond.field === 'category_group' ? (groupNameById.get(key) ?? key) : cat.name;

			if (cond.op === 'is') {
				return cond.value === key;
			} else if (cond.op === 'isNot') {
				return cond.value !== key;
			} else if (cond.op === 'oneOf') {
				return cond.value.includes(key);
			} else if (cond.op === 'notOneOf') {
				return !cond.value.includes(key);
			} else if (cond.op === 'contains') {
				return textValue.toLowerCase().includes((cond.value as string).toLowerCase());
			} else if (cond.op === 'doesNotContain') {
				return !textValue.toLowerCase().includes((cond.value as string).toLowerCase());
			} else if (cond.op === 'matches') {
				try {
					return new RegExp(cond.value as string, 'i').test(textValue);
				} catch (e) {
					console.warn('Invalid regexp in matches condition', e);
					return false;
				}
			}
			console.warn(`Unknown category condition operator: ${cond.op}`);
			return false;
		});
		return matching.map((cat: any) => cat.id);
	});

	if (conditionsOp === 'or') {
		const categoryIds = new Set(conditionResults.flat());
		return Array.from(categoryIds);
	} else {
		if (conditionResults.length === 0) {
			return [];
		}
		const firstSet = new Set(conditionResults[0]);
		for (let i = 1; i < conditionResults.length; i++) {
			const currentIds = new Set(conditionResults[i]);
			const toRemove: string[] = [];
			firstSet.forEach(id => {
				if (!currentIds.has(id)) {
					toRemove.push(id);
				}
			});
			toRemove.forEach(id => firstSet.delete(id));
		}
		return Array.from(firstSet);
	}
}

async function getMonthBudgetData(month: string): Promise<Array<{ name: string; value: string | number | boolean }>> {
	const monthData = await send('envelope-budget-month', { month });
	return monthData || [];
}

function getMonthDataValue(monthData: Array<{ name: string; value: string | number | boolean }>, pattern: string, catId: string) {
	const fieldName = pattern.replace('{catId}', catId);
	const cell = monthData.find(c => c.name.endsWith(fieldName));
	return cell?.value ?? 0;
}

async function extractQueryCategories(queryName: string, queries: any): Promise<string[]> {
	const queryConfig = queries[queryName];
	if (!queryConfig) {
		console.warn(`Query "${queryName}" not found in queries config`);
		return [];
	}

	const categoryConditions = extractCategoryConditions(queryConfig.conditions || []);
	const { list: allCategories } = await send('get-categories');
	return getCategoriesFromConditions(allCategories, categoryConditions, queryConfig.conditionsOp || 'and');
}

async function extractQueryTimeframeStart(queryName: string, queries: any): Promise<string> {
	const { monthUtils, ranges } = formulaModules;
	const queryConfig = queries[queryName];
	if (!queryConfig || !queryConfig.timeFrame) {
		console.warn(`Query "${queryName}" not found or has no timeframe; cannot extract start`);
		return monthUtils.currentMonth();
	}

	const [startMonth] = ranges.calculateTimeRange(ranges.asMonthSlidingTimeFrame(queryConfig.timeFrame));
	return startMonth;
}

async function extractQueryTimeframeEnd(queryName: string, queries: any): Promise<string> {
	const { monthUtils, ranges } = formulaModules;
	const queryConfig = queries[queryName];
	if (!queryConfig || !queryConfig.timeFrame) {
		console.warn(`Query "${queryName}" not found or has no timeframe; cannot extract end`);
		return monthUtils.currentMonth();
	}

	const [, endMonth] = ranges.calculateTimeRange(ranges.asMonthSlidingTimeFrame(queryConfig.timeFrame));
	return endMonth;
}

async function fetchBudgetDimensionValueDirect(dimension: string, categoryIds: string[], startMonth: string, endMonth: string): Promise<number> {
	const { monthUtils, util } = formulaModules;
	const allowed = new Set(['budgeted', 'spent', 'balance_start', 'balance_end', 'goal']);
	const dim = dimension.toLowerCase();
	if (!allowed.has(dim)) {
		throw new Error(`Invalid BUDGET_QUERY dimension: ${dimension}`);
	}

	const intervals = monthUtils.rangeInclusive(startMonth, endMonth);

	const sumDimension = async (fieldPattern: string): Promise<number> => {
		let total = 0;
		for (const month of intervals) {
			const monthData = await getMonthBudgetData(month);
			for (const catId of categoryIds) {
				total += getMonthDataValue(monthData, fieldPattern, catId) as number;
			}
		}
		return total;
	};

	if (dim === 'budgeted') {
		return util.integerToAmount(await sumDimension('budget-{catId}'), 2);
	}

	if (dim === 'spent') {
		return util.integerToAmount(await sumDimension('sum-amount-{catId}'), 2);
	}

	if (dim === 'goal') {
		return util.integerToAmount(await sumDimension('goal-{catId}'), 2);
	}

	if (dim === 'balance_start' || dim === 'balance_end') {
		let runningBalance = 0;
		const monthBeforeStart = monthUtils.subMonths(startMonth, 1);
		const prevMonthData = await getMonthBudgetData(monthBeforeStart);

		for (const catId of categoryIds) {
			const catBalance = getMonthDataValue(prevMonthData, 'leftover-{catId}', catId) as number;
			const hasCarryover = Boolean(getMonthDataValue(prevMonthData, 'carryover-{catId}', catId));
			if (catBalance > 0 || (catBalance < 0 && hasCarryover)) {
				runningBalance += catBalance;
			}
		}

		const balances: Record<string, { start: number; end: number }> = {};

		for (const month of intervals) {
			const monthData = await getMonthBudgetData(month);
			let budgeted = 0;
			let spent = 0;
			let carryoverToNextMonth = 0;

			for (const catId of categoryIds) {
				const catBudgeted = Number(getMonthDataValue(monthData, 'budget-{catId}', catId)) || 0;
				const catSpent = Number(getMonthDataValue(monthData, 'sum-amount-{catId}', catId)) || 0;
				const catBalance = Number(getMonthDataValue(monthData, 'leftover-{catId}', catId)) || 0;
				const hasCarryover = Boolean(getMonthDataValue(monthData, 'carryover-{catId}', catId));

				budgeted += catBudgeted;
				spent += catSpent;

				if (catBalance > 0 || (catBalance < 0 && hasCarryover)) {
					carryoverToNextMonth += catBalance;
				}
			}

			const balanceStart = runningBalance;
			const balanceEnd = budgeted + spent + runningBalance;

			balances[month] = { start: balanceStart, end: balanceEnd };
			runningBalance = carryoverToNextMonth;
		}

		if (dim === 'balance_start') {
			return util.integerToAmount(balances[intervals[0]]?.start || 0, 2);
		}
		return util.integerToAmount(balances[intervals[intervals.length - 1]]?.end || 0, 2);
	}

	return 0;
}
