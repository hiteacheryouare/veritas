import type { Op } from '../ops.ts';
import { nameId } from '../rng.ts';

export interface FixtureSpec {
	budgetName: string;
	build: () => Op[];
	// Start from Actual's "create new file" (create-budget), which keeps every
	// seed row, instead of runImport, which deletes the expense seeds.
	freshBudget?: boolean;
}

// Rows that ship in Actual's bundled default-db.sqlite and survive api.runImport.
export const INCOME_GROUP = '2E1F5BDB-209B-43F9-AF2C-3CE28E380C00';
export const INCOME_CATEGORY = '3c1699a5-522a-435e-86dc-93d900a14f0e';
export const STARTING_BALANCES_CATEGORY = '506e8d9d-7ed0-4397-84e4-07a9185dc6b2';

export const idsFor = (spec: string) => (kind: string, name: string | number) => nameId(`${spec}:${kind}:${name}`);

export const monthsBetween = (first: string, last: string): string[] => {
	const months: string[] = [];
	let [year, month] = first.split('-').map(Number);
	const [lastYear, lastMonth] = last.split('-').map(Number);
	while (year < lastYear || (year === lastYear && month <= lastMonth)) {
		months.push(`${year}-${String(month).padStart(2, '0')}`);
		month++;
		if (month > 12) {
			month = 1;
			year++;
		}
	}
	return months;
};

export const day = (month: string, dayOfMonth: number) => `${month}-${String(dayOfMonth).padStart(2, '0')}`;

export const daysInMonth = (month: string) => {
	const [year, monthNumber] = month.split('-').map(Number);
	return new Date(Date.UTC(year, monthNumber, 0)).getUTCDate();
};
