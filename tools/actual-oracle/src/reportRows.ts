import type { CanonicalValue } from './canonicalJson.ts';
import type { ActualSession } from './environment.ts';

// One custom-report query as the report page sends it (dates already resolved).
export interface ReportRowsRequest {
	id: string;
	startDate: string;
	endDate: string;
	interval: 'Daily' | 'Weekly' | 'Monthly' | 'Yearly';
	conditions: unknown[];
	conditionsOp: 'and' | 'or';
}

const intervalUnit = { Daily: 'day', Weekly: 'week', Monthly: 'month', Yearly: 'year' };

// The query of desktop-client's makeQuery, verbatim apart from the Tag branch
// (reference/actual/packages/desktop-client/src/components/reports/spreadsheets/makeQuery.ts:6-69).
const makeQuery = (
	q: any,
	name: 'assets' | 'debts',
	request: ReportRowsRequest,
	filters: unknown[]
) => {
	const { interval, startDate, endDate } = request;
	const intervalGroup =
		interval === 'Monthly' ? { $month: '$date' } : interval === 'Yearly' ? { $year: '$date' } : { $day: '$date' };
	const intervalFilter = interval === 'Weekly' ? '$day' : '$' + intervalUnit[interval];
	return q('transactions')
		.filter({ [request.conditionsOp === 'or' ? '$or' : '$and']: filters })
		.filter({
			$and: [
				{ date: { $transform: intervalFilter, $gte: startDate } },
				{ date: { $transform: intervalFilter, $lte: endDate } }
			]
		})
		.filter(name === 'assets' ? { amount: { $gt: 0 } } : { amount: { $lt: 0 } })
		.groupBy([intervalGroup, { $id: '$account' }, { $id: '$payee' }, { $id: '$category' }, { $id: '$payee.transfer_acct.id' }])
		.select([
			{ date: intervalGroup },
			{ category: { $id: '$category.id' } },
			{ categoryHidden: { $id: '$category.hidden' } },
			{ categoryIncome: { $id: '$category.is_income' } },
			{ categoryGroup: { $id: '$category.group.id' } },
			{ categoryGroupHidden: { $id: '$category.group.hidden' } },
			{ account: { $id: '$account.id' } },
			{ accountOffBudget: { $id: '$account.offbudget' } },
			{ payee: { $id: '$payee.id' } },
			{ transferAccount: { $id: '$payee.transfer_acct.id' } },
			{ amount: { $sum: '$amount' } }
		]);
};

const sortRows = (rows: Record<string, CanonicalValue>[]) =>
	rows
		.map(row => ({ row, key: JSON.stringify(Object.keys(row).sort().map(key => [key, row[key]])) }))
		.sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : 0))
		.map(({ row }) => row);

// The rows Actual's own query engine and condition compiler return for each
// request, before any of the report page's aggregation.
export const reportRows = async ({ api, internal }: ActualSession, requests: ReportRowsRequest[]) => {
	const results: Record<string, CanonicalValue> = {};
	for (const request of requests) {
		const { filters } = await internal.send('make-filters-from-conditions', {
			conditions: request.conditions.filter((condition: any) => !condition?.customName)
		});
		const run = async (name: 'assets' | 'debts') =>
			sortRows(((await api.aqlQuery(makeQuery(api.q, name, request, filters))) as { data: any[] }).data);
		results[request.id] = { assets: await run('assets'), debts: await run('debts') };
	}
	return results;
};
