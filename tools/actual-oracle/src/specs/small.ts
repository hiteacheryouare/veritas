import type { Op } from '../ops.ts';
import { INCOME_CATEGORY, day, idsFor, monthsBetween } from './builders.ts';
import type { FixtureSpec } from './builders.ts';

const id = idsFor('small');

const build = (): Op[] => {
	const checking = id('account', 'checking');
	const savings = id('account', 'savings');
	const bills = id('group', 'bills');
	const everyday = id('group', 'everyday');
	const rent = id('category', 'rent');
	const utilities = id('category', 'utilities');
	const groceries = id('category', 'groceries');
	const dining = id('category', 'dining');
	const employer = id('payee', 'employer');
	const landlord = id('payee', 'landlord');
	const grocer = id('payee', 'grocer');

	const ops: Op[] = [
		{ op: 'createAccount', id: checking, name: 'Checking' },
		{ op: 'createAccount', id: savings, name: 'Savings' },
		{ op: 'createCategoryGroup', id: bills, name: 'Bills' },
		{ op: 'createCategoryGroup', id: everyday, name: 'Everyday' },
		{ op: 'createCategory', id: rent, name: 'Rent', group: bills },
		{ op: 'createCategory', id: utilities, name: 'Utilities', group: bills },
		{ op: 'createCategory', id: groceries, name: 'Groceries', group: everyday },
		{ op: 'createCategory', id: dining, name: 'Dining Out', group: everyday },
		{ op: 'createPayee', id: employer, name: 'Acme Corp' },
		{ op: 'createPayee', id: landlord, name: 'Landlord' },
		{ op: 'createPayee', id: grocer, name: 'Corner Grocer' }
	];

	const months = monthsBetween('2026-08', '2026-10');
	for (const month of months) {
		ops.push({
			op: 'addTransactions',
			account: checking,
			transactions: [
				{ id: id('txn', `${month}-pay`), date: day(month, 1), amount: 400000, payee: employer, category: INCOME_CATEGORY },
				{ id: id('txn', `${month}-rent`), date: day(month, 2), amount: -150000, payee: landlord, category: rent },
				{ id: id('txn', `${month}-power`), date: day(month, 12), amount: -8423, payee_name: 'City Power', category: utilities },
				{ id: id('txn', `${month}-groceries`), date: day(month, 9), amount: -12657, payee: grocer, category: groceries },
				{ id: id('txn', `${month}-dinner`), date: day(month, 20), amount: -4810, payee_name: 'Noodle Bar', category: dining }
			]
		});
	}
	ops.push({
		op: 'addTransactions',
		account: checking,
		transactions: [{ id: id('txn', 'to-savings'), date: '2026-09-15', amount: -50000, payee: { transferTo: savings } }]
	});

	for (const month of months) {
		ops.push(
			{ op: 'setBudgetAmount', month, category: rent, amount: 150000 },
			{ op: 'setBudgetAmount', month, category: utilities, amount: 9000 },
			{ op: 'setBudgetAmount', month, category: groceries, amount: 12000 },
			{ op: 'setBudgetAmount', month, category: dining, amount: 5000 }
		);
	}
	return ops;
};

export const small: FixtureSpec = { budgetName: 'Veritas Small', build };
