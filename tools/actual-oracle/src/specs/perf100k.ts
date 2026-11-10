import type { Op } from '../ops.ts';
import { createRng } from '../rng.ts';
import { INCOME_CATEGORY, day, daysInMonth, idsFor, monthsBetween } from './builders.ts';
import type { FixtureSpec } from './builders.ts';

const id = idsFor('perf-100k');

const FIRST_MONTH = '2011-11';
const LAST_MONTH = '2026-10';
const GROUP_COUNT = 25;
const CATEGORIES_PER_GROUP = 20;
const PAYEE_COUNT = 400;
const SPENDING_PER_MONTH = 540;
const BUDGETED_CATEGORIES_PER_MONTH = 80;

const spendingAccounts = [
	{ key: 'checking', name: 'Checking', share: 0.45 },
	{ key: 'visa', name: 'Visa', share: 0.3 },
	{ key: 'amex', name: 'Amex', share: 0.2 },
	{ key: 'cash', name: 'Cash', share: 0.05 }
];

const build = (): Op[] => {
	const rng = createRng(100_000);
	const months = monthsBetween(FIRST_MONTH, LAST_MONTH);
	const account = (key: string) => id('account', key);

	const ops: Op[] = [
		...spendingAccounts.map(({ key, name }) => ({ op: 'createAccount', id: account(key), name })),
		{ op: 'createAccount', id: account('savings'), name: 'Savings' },
		{ op: 'createAccount', id: account('retirement'), name: 'Retirement', offBudget: true },
		{ op: 'createAccount', id: account('mortgage'), name: 'Mortgage', offBudget: true }
	];

	const categoryIds: string[] = [];
	for (let g = 0; g < GROUP_COUNT; g++) {
		const group = id('group', g);
		ops.push({ op: 'createCategoryGroup', id: group, name: `Group ${String(g + 1).padStart(2, '0')}`, hidden: g === GROUP_COUNT - 1 });
		for (let c = 0; c < CATEGORIES_PER_GROUP; c++) {
			const category = id('category', `${g}-${c}`);
			categoryIds.push(category);
			ops.push({ op: 'createCategory', id: category, name: `Category ${g + 1}.${c + 1}`, group, hidden: c === CATEGORIES_PER_GROUP - 1 });
		}
	}

	const payeeIds: string[] = [];
	for (let p = 0; p < PAYEE_COUNT; p++) {
		const payee = id('payee', p);
		payeeIds.push(payee);
		ops.push({ op: 'createPayee', id: payee, name: `Merchant ${String(p + 1).padStart(3, '0')}` });
	}
	const employer = id('payee', 'employer');
	ops.push({ op: 'createPayee', id: employer, name: 'Employer' });

	ops.push(
		{
			op: 'addTransactions',
			account: account('retirement'),
			transactions: [{ id: id('txn', 'retirement-opening'), date: day(FIRST_MONTH, 1), amount: 1_500_000, payee_name: 'Starting Balance' }]
		},
		{
			op: 'addTransactions',
			account: account('mortgage'),
			transactions: [{ id: id('txn', 'mortgage-opening'), date: day(FIRST_MONTH, 1), amount: -32_000_000, payee_name: 'Starting Balance' }]
		}
	);

	const hotCategories = categoryIds.slice(0, 200);
	for (const [monthIndex, month] of months.entries()) {
		const days = daysInMonth(month);
		const txn = (name: string | number) => id('txn', `${month}-${name}`);
		ops.push({
			op: 'addTransactions',
			account: account('checking'),
			transactions: [
				{ id: txn('pay-1'), date: day(month, 1), amount: 420_000 + monthIndex * 500, payee: employer, category: INCOME_CATEGORY },
				{ id: txn('pay-2'), date: day(month, 15), amount: 420_000 + monthIndex * 500, payee: employer, category: INCOME_CATEGORY },
				{ id: txn('savings'), date: day(month, 16), amount: -60_000, payee: { transferTo: account('savings') } },
				{ id: txn('retirement'), date: day(month, 17), amount: -40_000, payee: { transferTo: account('retirement') }, category: categoryIds[0] },
				{ id: txn('mortgage'), date: day(month, 2), amount: -210_000, payee: { transferTo: account('mortgage') }, category: categoryIds[1] },
				{ id: txn('visa-payment'), date: day(month, 25), amount: -150_000, payee: { transferTo: account('visa') } },
				{ id: txn('amex-payment'), date: day(month, 26), amount: -100_000, payee: { transferTo: account('amex') } }
			]
		});

		const perAccount = new Map<string, any[]>(spendingAccounts.map(({ key }) => [key, []]));
		for (let i = 0; i < SPENDING_PER_MONTH; i++) {
			const roll = rng.next();
			let cumulative = 0;
			const target = spendingAccounts.find(({ share }) => (cumulative += share) >= roll) ?? spendingAccounts[0];
			const amount = -rng.int(150, 25_000);
			const transaction: any = {
				id: txn(i),
				date: day(month, rng.int(1, days)),
				amount,
				payee: rng.pick(payeeIds),
				cleared: rng.chance(0.9)
			};
			if (rng.chance(0.03)) {
				const first = -rng.int(100, -amount - 50);
				transaction.subtransactions = [
					{ id: txn(`${i}-a`), amount: first, category: rng.pick(hotCategories) },
					{ id: txn(`${i}-b`), amount: amount - first, category: rng.pick(categoryIds) }
				];
			} else {
				transaction.category = rng.chance(0.8) ? rng.pick(hotCategories) : rng.pick(categoryIds);
			}
			if (rng.chance(0.05)) {
				transaction.notes = `Note ${i} for ${month}`;
			}
			perAccount.get(target.key)?.push(transaction);
		}
		for (const [key, transactions] of perAccount) {
			ops.push({ op: 'addTransactions', account: account(key), transactions });
		}
	}

	for (const [monthIndex, month] of months.entries()) {
		for (let i = 0; i < BUDGETED_CATEGORIES_PER_MONTH; i++) {
			ops.push({
				op: 'setBudgetAmount',
				month,
				category: categoryIds[(i * 7 + monthIndex) % categoryIds.length],
				amount: rng.int(1, 400) * 500
			});
		}
	}
	return ops;
};

export const perf100k: FixtureSpec = { budgetName: 'Veritas Perf 100k', build };
