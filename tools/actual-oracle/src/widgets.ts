import type { CanonicalValue } from './canonicalJson.ts';
import type { ActualSession } from './environment.ts';
import { loadSource } from './reference.ts';
import { connectTo } from './shims/connection.ts';

// One dashboard card, as the dashboard stores it.
export interface WidgetRequest {
	id: string;
	type: string;
	meta: any;
	firstDayOfWeekIdx?: string;
	budgetType?: string;
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
		// NetWorthCard.tsx's range and spreadsheet. The card's labels need a locale
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
					accounts: data.accounts.map((account: any) => account.id)
				}
			};
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
