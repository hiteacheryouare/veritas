import type { Op } from '../ops.ts';
import { createRng } from '../rng.ts';
import { INCOME_CATEGORY, INCOME_GROUP, day, idsFor, monthsBetween } from './builders.ts';
import type { FixtureSpec } from './builders.ts';

const id = idsFor('all-features');

const accounts = {
	checking: id('account', 'checking'),
	savings: id('account', 'savings'),
	visa: id('account', 'visa'),
	brokerage: id('account', 'brokerage'),
	carLoan: id('account', 'car-loan'),
	oldChecking: id('account', 'old-checking'),
	wallet: id('account', 'wallet')
};

const groups = {
	bills: id('group', 'bills'),
	everyday: id('group', 'everyday'),
	goals: id('group', 'goals'),
	retired: id('group', 'retired')
};

const categories = {
	rent: id('category', 'rent'),
	internet: id('category', 'internet'),
	carInterest: id('category', 'car-interest'),
	groceries: id('category', 'groceries'),
	dining: id('category', 'dining'),
	household: id('category', 'household'),
	fun: id('category', 'fun'),
	vacation: id('category', 'vacation'),
	emergency: id('category', 'emergency'),
	oldHobby: id('category', 'old-hobby'),
	gifts: id('category', 'gifts'),
	unused: id('category', 'unused'),
	interestIncome: id('category', 'interest-income')
};

const payees = {
	employer: id('payee', 'employer'),
	landlord: id('payee', 'landlord'),
	isp: id('payee', 'isp'),
	grocer: id('payee', 'grocer'),
	grocerDuplicate: id('payee', 'grocer-duplicate'),
	cafe: id('payee', 'cafe'),
	lender: id('payee', 'lender'),
	bank: id('payee', 'bank'),
	gym: id('payee', 'gym'),
	templated: id('payee', 'templated')
};

const reportIds = {
	spendingByCategory: id('report', 'spending-by-category'),
	netWorthOverTime: id('report', 'net-worth-over-time')
};

const FIRST_MONTH = '2025-11';
const LAST_MONTH = '2026-10';

const structure = (): Op[] => [
	{ op: 'createAccountGroup', id: id('account-group', 'cash'), name: 'Cash' },
	{ op: 'createAccountGroup', id: id('account-group', 'debt'), name: 'Debt' },

	{ op: 'createAccount', id: accounts.checking, name: 'Checking' },
	{ op: 'createAccount', id: accounts.savings, name: 'Savings' },
	{ op: 'createAccount', id: accounts.visa, name: 'Visa Card' },
	{ op: 'createAccount', id: accounts.brokerage, name: 'Brokerage', offBudget: true },
	{ op: 'createAccount', id: accounts.carLoan, name: 'Car Loan', offBudget: true },
	{ op: 'createAccount', id: accounts.oldChecking, name: 'Old Checking' },
	{ op: 'createAccount', id: accounts.wallet, name: 'Wallet', initialBalance: 12000 },
	{ op: 'updateAccount', id: accounts.checking, fields: { account_group_id: id('account-group', 'cash') } },
	{ op: 'updateAccount', id: accounts.savings, fields: { account_group_id: id('account-group', 'cash') } },
	{ op: 'updateAccount', id: accounts.visa, fields: { account_group_id: id('account-group', 'debt') } },
	{ op: 'updateAccount', id: accounts.carLoan, fields: { account_group_id: id('account-group', 'debt') } },

	{ op: 'createCategoryGroup', id: groups.bills, name: 'Bills' },
	{ op: 'createCategoryGroup', id: groups.everyday, name: 'Everyday' },
	{ op: 'createCategoryGroup', id: groups.goals, name: 'Goals' },
	{ op: 'createCategoryGroup', id: groups.retired, name: 'Retired', hidden: true },
	{ op: 'createCategory', id: categories.rent, name: 'Rent', group: groups.bills },
	{ op: 'createCategory', id: categories.internet, name: 'Internet', group: groups.bills },
	{ op: 'createCategory', id: categories.carInterest, name: 'Car Loan Interest', group: groups.bills },
	{ op: 'createCategory', id: categories.groceries, name: 'Groceries', group: groups.everyday },
	{ op: 'createCategory', id: categories.dining, name: 'Dining Out', group: groups.everyday },
	{ op: 'createCategory', id: categories.household, name: 'Household', group: groups.everyday },
	{ op: 'createCategory', id: categories.fun, name: 'Fun Money', group: groups.everyday },
	{ op: 'createCategory', id: categories.vacation, name: 'Vacation', group: groups.goals },
	{ op: 'createCategory', id: categories.emergency, name: 'Emergency Fund', group: groups.goals },
	{ op: 'createCategory', id: categories.gifts, name: 'Gifts', group: groups.goals, hidden: true },
	{ op: 'createCategory', id: categories.oldHobby, name: 'Old Hobby', group: groups.retired },
	{ op: 'createCategory', id: categories.unused, name: 'Unused', group: groups.everyday },
	{ op: 'createCategory', id: categories.interestIncome, name: 'Interest', group: INCOME_GROUP, isIncome: true },

	{ op: 'createPayee', id: payees.employer, name: 'Acme Corp' },
	{ op: 'createPayee', id: payees.landlord, name: 'Landlord' },
	{ op: 'createPayee', id: payees.isp, name: 'FiberNet' },
	{ op: 'createPayee', id: payees.grocer, name: 'Corner Grocer' },
	{ op: 'createPayee', id: payees.grocerDuplicate, name: 'CORNER GROCER #42' },
	{ op: 'createPayee', id: payees.cafe, name: 'Bean There Cafe' },
	{ op: 'createPayee', id: payees.lender, name: 'Auto Lender' },
	{ op: 'createPayee', id: payees.bank, name: 'First Bank' },
	{ op: 'createPayee', id: payees.gym, name: 'Iron Gym' },
	{ op: 'createPayee', id: payees.templated, name: 'Templated Payee' },
	{ op: 'createPayeeLocation', id: id('payee-location', 'cafe'), payee: payees.cafe, latitude: 40.7128, longitude: -74.006 },

	{ op: 'createTag', id: id('tag', 'vacation'), tag: 'vacation', color: '#4f86c6', description: 'Trips away from home' },
	{ op: 'createTag', id: id('tag', 'reimbursable'), tag: 'reimbursable' }
];

const openingBalances = (): Op[] => [
	{
		op: 'addTransactions',
		account: accounts.checking,
		transactions: [
			{
				id: id('txn', 'checking-opening'),
				date: '2025-11-01',
				amount: 350000,
				payee_name: 'Starting Balance',
				category: INCOME_CATEGORY,
				notes: 'Opening balance'
			}
		]
	},
	{
		op: 'addTransactions',
		account: accounts.carLoan,
		transactions: [{ id: id('txn', 'loan-opening'), date: '2025-11-01', amount: -1800000, payee_name: 'Starting Balance' }]
	},
	{
		op: 'addTransactions',
		account: accounts.brokerage,
		transactions: [{ id: id('txn', 'brokerage-opening'), date: '2025-11-01', amount: 2500000, payee_name: 'Starting Balance' }]
	},
	{
		op: 'addTransactions',
		account: accounts.oldChecking,
		transactions: [{ id: id('txn', 'old-checking-opening'), date: '2025-11-01', amount: 15000, payee: payees.bank, category: INCOME_CATEGORY }]
	}
];

const monthActivity = (month: string, index: number, rng: ReturnType<typeof createRng>): Op[] => {
	const txn = (name: string) => id('txn', `${month}-${name}`);
	const checking = [
		{ id: txn('pay-1'), date: day(month, 1), amount: 310000, payee: payees.employer, category: INCOME_CATEGORY },
		{ id: txn('pay-2'), date: day(month, 15), amount: 310000, payee: payees.employer, category: INCOME_CATEGORY },
		{ id: txn('rent'), date: day(month, 3), amount: -180000, payee: payees.landlord, category: categories.rent },
		{ id: txn('internet'), date: day(month, 10), amount: -6999, payee: payees.isp, category: categories.internet, cleared: false },
		{
			id: txn('groceries-split'),
			date: day(month, 7),
			amount: -15420 - rng.int(0, 4000),
			payee: payees.grocer,
			notes: 'Weekly shop',
			subtransactions: [
				{ id: txn('groceries-split-food'), amount: -11000, category: categories.groceries },
				{ id: txn('groceries-split-home'), amount: -4420, category: categories.household, notes: 'Paper towels #reimbursable' }
			]
		},
		{
			id: txn('loan-payment'),
			date: day(month, 20),
			amount: -45000,
			payee: payees.lender,
			notes: 'Car payment',
			subtransactions: [
				{ id: txn('loan-principal'), amount: -38000 - index * 50, payee: { transferTo: accounts.carLoan } },
				{ id: txn('loan-interest'), amount: -7000 + index * 50, category: categories.carInterest }
			]
		},
		{ id: txn('visa-payment'), date: day(month, 25), amount: -12000, payee: { transferTo: accounts.visa }, notes: 'Card payment' },
		{ id: txn('to-savings'), date: day(month, 16), amount: -50000, payee: { transferTo: accounts.savings } },
		{
			id: txn('to-brokerage'),
			date: day(month, 17),
			amount: -25000,
			payee: { transferTo: accounts.brokerage },
			category: categories.emergency,
			notes: 'Invest'
		}
	];
	const groceriesSplit = checking[4] as any;
	groceriesSplit.subtransactions[0].amount = groceriesSplit.amount + 4420;

	const visa = [
		{ id: txn('cafe-1'), date: day(month, 4), amount: -(450 + rng.int(0, 600)), payee: payees.cafe, category: categories.dining },
		{ id: txn('cafe-2'), date: day(month, 18), amount: -(450 + rng.int(0, 600)), payee: payees.cafe, category: categories.dining },
		{ id: txn('dinner'), date: day(month, 22), amount: -(4000 + rng.int(0, 9000)), payee_name: 'Trattoria', category: categories.dining },
		{ id: txn('gym'), date: day(month, 2), amount: -4500, payee: payees.gym, category: categories.fun },
		{ id: txn('refund'), date: day(month, 26), amount: 1299, payee_name: 'Hardware Store', category: categories.household, notes: 'Refund' }
	];

	const ops: Op[] = [
		{ op: 'addTransactions', account: accounts.checking, transactions: checking },
		{ op: 'addTransactions', account: accounts.visa, transactions: visa },
		{
			op: 'addTransactions',
			account: accounts.savings,
			transactions: [{ id: txn('interest'), date: day(month, 28), amount: 1200 + index * 10, payee: payees.bank, category: categories.interestIncome }]
		},
		{
			op: 'addTransactions',
			account: accounts.brokerage,
			transactions: [{ id: txn('dividend'), date: day(month, 27), amount: 3100 + index * 25, payee_name: 'Dividend' }]
		}
	];
	if (index % 4 === 1) {
		ops.push({
			op: 'addTransactions',
			account: accounts.visa,
			transactions: [
				{ id: txn('flight'), date: day(month, 12), amount: -38900, payee_name: 'SkyAir', category: categories.vacation, notes: 'Flight #vacation' }
			]
		});
	}
	if (index < 3) {
		ops.push({
			op: 'addTransactions',
			account: accounts.checking,
			transactions: [{ id: txn('hobby'), date: day(month, 9), amount: -2500, payee_name: 'Model Shop', category: categories.oldHobby }]
		});
	}
	return ops;
};

const budgeting = (months: string[]): Op[] => {
	const ops: Op[] = [];
	for (const [index, month] of months.entries()) {
		ops.push(
			{ op: 'setBudgetAmount', month, category: categories.rent, amount: 180000 },
			{ op: 'setBudgetAmount', month, category: categories.internet, amount: 7000 },
			{ op: 'setBudgetAmount', month, category: categories.carInterest, amount: 7000 },
			{ op: 'setBudgetAmount', month, category: categories.groceries, amount: 14000 },
			{ op: 'setBudgetAmount', month, category: categories.dining, amount: index % 3 === 0 ? 5000 : 12000 },
			{ op: 'setBudgetAmount', month, category: categories.household, amount: 3000 },
			{ op: 'setBudgetAmount', month, category: categories.fun, amount: 5000 },
			{ op: 'setBudgetAmount', month, category: categories.vacation, amount: 20000 },
			{ op: 'setBudgetAmount', month, category: categories.emergency, amount: 30000 }
		);
		if (index < 3) {
			ops.push({ op: 'setBudgetAmount', month, category: categories.oldHobby, amount: 2500 });
		}
	}
	ops.push(
		{ op: 'setCarryover', month: months[0], category: categories.dining, flag: true },
		{ op: 'setCarryover', month: months[2], category: categories.vacation, flag: true },
		{ op: 'moveBudget', month: months[5], from: categories.fun, to: categories.dining, amount: 2000 },
		{ op: 'moveBudget', month: months[6], from: categories.emergency, to: 'to-budget', amount: 5000 },
		{ op: 'transferAvailable', month: months[7], amount: 3000, category: categories.gifts },
		{ op: 'coverOverspending', month: months[3], from: categories.fun, to: categories.dining, amount: 1000 },
		{ op: 'holdForNextMonth', month: months[8], amount: 25000 },
		{ op: 'holdForNextMonth', month: months[9], amount: 10000 },
		{ op: 'resetHold', month: months[9] }
	);
	return ops;
};

const rules = (): Op[] => [
	{
		op: 'createRule',
		id: id('rule', 'rename-grocer'),
		stage: 'pre',
		conditionsOp: 'and',
		conditions: [{ field: 'imported_payee', op: 'contains', value: 'CORNER GROCER', type: 'imported_payee' }],
		actions: [{ field: 'payee', op: 'set', value: payees.grocer, type: 'id' }]
	},
	{
		op: 'createRule',
		id: id('rule', 'cafe-dining'),
		stage: null,
		conditionsOp: 'or',
		conditions: [
			{ field: 'payee', op: 'is', value: payees.cafe, type: 'id' },
			{ field: 'imported_payee', op: 'contains', value: 'coffee', type: 'imported_payee' }
		],
		actions: [{ field: 'category', op: 'set', value: categories.dining, type: 'id' }]
	},
	{
		op: 'createRule',
		id: id('rule', 'templated-notes'),
		stage: 'post',
		conditionsOp: 'and',
		conditions: [{ field: 'payee', op: 'is', value: payees.templated, type: 'id' }],
		actions: [
			{ field: 'notes', op: 'set', value: '', type: 'string', options: { template: '{{payee_name}} paid on {{date}}' } },
			{ op: 'append-notes', value: ' #reimbursable', type: 'string' }
		]
	},
	{
		op: 'createRule',
		id: id('rule', 'formula-amount'),
		stage: 'post',
		conditionsOp: 'and',
		conditions: [
			{ field: 'payee', op: 'is', value: payees.templated, type: 'id' },
			{ field: 'amount', op: 'lt', value: -100000, type: 'number' }
		],
		actions: [{ field: 'amount', op: 'set', value: 0, type: 'number', options: { formula: '=amount / 2' } }]
	}
];

const schedules = (): Op[] => [
	{
		op: 'createSchedule',
		id: id('schedule', 'rent'),
		name: 'Rent',
		postsTransaction: true,
		payee: payees.landlord,
		account: accounts.checking,
		amount: -180000,
		amountOp: 'is',
		date: { start: '2025-11-03', frequency: 'monthly', interval: 1, patterns: [], skipWeekend: false, weekendSolveMode: 'after', endMode: 'never' }
	},
	{
		op: 'createSchedule',
		id: id('schedule', 'internet'),
		name: 'Internet',
		postsTransaction: false,
		payee: payees.isp,
		account: accounts.checking,
		amount: -6999,
		amountOp: 'isapprox',
		date: { start: '2025-11-10', frequency: 'monthly', interval: 1, patterns: [], skipWeekend: true, weekendSolveMode: 'before', endMode: 'never' }
	},
	{
		op: 'createSchedule',
		id: id('schedule', 'gym'),
		name: 'Gym',
		postsTransaction: false,
		payee: payees.gym,
		account: accounts.visa,
		amount: { num1: -5000, num2: -4000 },
		amountOp: 'isbetween',
		date: { start: '2026-01-31', frequency: 'monthly', interval: 1, patterns: [], skipWeekend: false, weekendSolveMode: 'after', endMode: 'after_n_occurrences', endOccurrences: 24 }
	},
	{
		op: 'createSchedule',
		id: id('schedule', 'insurance'),
		name: 'Car Insurance',
		postsTransaction: true,
		payee: payees.lender,
		account: accounts.checking,
		amount: -62000,
		amountOp: 'is',
		date: { start: '2024-02-29', frequency: 'yearly', interval: 1, patterns: [], skipWeekend: false, weekendSolveMode: 'after', endMode: 'on_date', endDate: '2030-12-31' }
	},
	{
		op: 'createSchedule',
		id: id('schedule', 'paycheck'),
		name: 'Paycheck',
		postsTransaction: false,
		payee: payees.employer,
		account: accounts.checking,
		amount: 310000,
		amountOp: 'is',
		date: {
			start: '2025-11-01',
			frequency: 'monthly',
			interval: 1,
			patterns: [{ type: 'day', value: 1 }, { type: 'day', value: 15 }],
			skipWeekend: true,
			weekendSolveMode: 'before',
			endMode: 'never'
		}
	},
	{
		op: 'createSchedule',
		id: id('schedule', 'one-off'),
		name: 'Concert Tickets',
		postsTransaction: false,
		account: accounts.visa,
		amount: -15000,
		amountOp: 'is',
		date: '2026-12-05'
	}
];

const reportsAndDashboard = (): Op[] => [
	{
		op: 'createCustomReport',
		id: reportIds.spendingByCategory,
		report: {
			name: 'Spending by category',
			startDate: '2025-11-01',
			endDate: '2026-10-31',
			isDateStatic: false,
			dateRange: 'Last 6 months',
			mode: 'total',
			groupBy: 'Category',
			interval: 'Monthly',
			balanceType: 'Payment',
			sortBy: 'desc',
			showEmpty: false,
			showOffBudget: false,
			showHiddenCategories: true,
			includeCurrentInterval: false,
			showUncategorized: false,
			trimIntervals: false,
			showTrendLines: false,
			graphType: 'BarGraph',
			conditions: [{ field: 'category', op: 'oneOf', value: [categories.groceries, categories.dining], type: 'id' }],
			conditionsOp: 'and'
		}
	},
	{
		op: 'createCustomReport',
		id: reportIds.netWorthOverTime,
		report: {
			name: 'Net worth over time',
			startDate: '2025-11-01',
			endDate: '2026-10-31',
			isDateStatic: true,
			dateRange: 'Static',
			mode: 'time',
			groupBy: 'Account',
			interval: 'Monthly',
			balanceType: 'Net',
			sortBy: 'name',
			showEmpty: true,
			showOffBudget: true,
			showHiddenCategories: false,
			includeCurrentInterval: true,
			showUncategorized: true,
			trimIntervals: true,
			showTrendLines: true,
			graphType: 'StackedBarGraph',
			conditions: [],
			conditionsOp: 'and'
		}
	},
	{ op: 'createDashboardPage', id: id('dashboard-page', 'deep-dive'), name: 'Deep dive' },
	...[
		{ type: 'net-worth-card', width: 8, height: 2, meta: { name: 'Net worth', mode: 'stacked', interval: 'Monthly' } },
		{ type: 'cash-flow-card', width: 4, height: 2, meta: { name: 'Cash flow', showBalance: true } },
		{ type: 'spending-card', width: 4, height: 2, meta: { name: 'Spending', mode: 'average', averageRange: { mode: 'last-n-months', months: 6 } } },
		{ type: 'custom-report', width: 4, height: 2, meta: { id: reportIds.spendingByCategory } },
		{ type: 'markdown-card', width: 4, height: 2, meta: { content: '## Budget notes\n\nPay the card **in full**.', text_align: 'left' } },
		{ type: 'summary-card', width: 4, height: 2, meta: { name: 'Avg dining', content: '{"type":"avgPerMonth"}', conditions: [{ field: 'category', op: 'is', value: categories.dining, type: 'id' }], conditionsOp: 'and' } },
		{ type: 'calendar-card', width: 4, height: 2, meta: { name: 'Calendar' } },
		{ type: 'sankey-card', width: 8, height: 3, meta: { name: 'Where it went', mode: 'spent', topNcategories: 8 } },
		{ type: 'formula-card', width: 4, height: 2, meta: { name: 'Savings rate', formula: '=1-(SPENT/INCOME)' } },
		{ type: 'future-widget-card', width: 4, height: 2, meta: { anything: ['Veritas', 'does', 'not', 'know', 'this'] } }
	].map((widget, index) => ({
		op: 'addDashboardWidget',
		id: id('widget', index),
		page: id('dashboard-page', 'deep-dive'),
		x: (index % 3) * 4,
		y: Math.floor(index / 3) * 2,
		...widget
	})),
	{
		op: 'createFilter',
		id: id('filter', 'vacation'),
		name: 'Vacation spending',
		conditionsOp: 'and',
		conditions: [{ field: 'notes', op: 'hasTags', value: '#vacation', type: 'string' }]
	}
];

const notesAndPrefs = (): Op[] => [
	{ op: 'setNote', id: categories.groceries, note: 'Includes household basics.\n#template 140' },
	{ op: 'setNote', id: categories.vacation, note: '#template 200 up to 1500' },
	{ op: 'setNote', id: categories.fun, note: '#cleanup Discretionary source' },
	{ op: 'setNote', id: categories.dining, note: '#cleanup Discretionary sink' },
	{ op: 'setNote', id: `account-${accounts.checking}`, note: 'Primary account at First Bank.' },
	{ op: 'setNote', id: id('report-note', 'unused'), note: 'A note whose id matches nothing.' },
	{ op: 'setPreference', id: 'dateFormat', value: 'MM/dd/yyyy' },
	{ op: 'setPreference', id: 'numberFormat', value: 'comma-dot' },
	{ op: 'setPreference', id: 'firstDayOfWeekIdx', value: '1' }
];

const build = (): Op[] => {
	const rng = createRng(20261109);
	const months = monthsBetween(FIRST_MONTH, LAST_MONTH);
	const ops: Op[] = [...structure(), ...rules(), ...openingBalances()];
	for (const [index, month] of months.entries()) {
		ops.push(...monthActivity(month, index, rng));
	}
	ops.push(
		{
			op: 'addTransactions',
			account: accounts.brokerage,
			transactions: [{ id: id('txn', 'brokerage-withdrawal'), date: '2026-06-05', amount: -100000, payee: { transferTo: accounts.checking }, notes: 'Off-budget to on-budget' }]
		},
		{
			op: 'addTransactions',
			account: accounts.checking,
			transactions: [{ id: id('txn', 'gift-purchase'), date: '2026-07-19', amount: -3000, payee_name: 'Gift Shop', category: categories.gifts }]
		},
		{
			op: 'addTransactions',
			account: accounts.checking,
			transactions: [{ id: id('txn', 'duplicate-payee'), date: '2026-08-08', amount: -2300, payee: payees.grocerDuplicate, category: categories.groceries }]
		},
		{ op: 'mergePayees', target: payees.grocer, merge: [payees.grocerDuplicate] },
		{ op: 'updateTransaction', id: id('txn', '2026-03-dinner'), fields: { notes: 'Birthday dinner #reimbursable', cleared: false } },
		{ op: 'updateTransaction', id: { transferOf: id('txn', '2026-04-to-savings') }, fields: { notes: 'Edited on the savings side' } },
		{ op: 'deleteTransaction', id: id('txn', '2026-05-cafe-2') },
		{ op: 'deleteTransaction', id: id('txn', '2026-06-to-savings') }
	);
	ops.push(...budgeting(months), ...schedules(), ...reportsAndDashboard(), ...notesAndPrefs());
	ops.push(
		{ op: 'applyTemplates', month: '2026-11' },
		{ op: 'storeNoteCleanups' },
		{ op: 'updateCategory', id: categories.oldHobby, fields: { hidden: true } },
		{ op: 'deleteCategory', id: categories.unused, transferCategory: categories.fun },
		{
			op: 'addTransactions',
			account: accounts.oldChecking,
			transactions: [{ id: id('txn', 'old-checking-close'), date: '2026-01-15', amount: -15000, payee: { transferTo: accounts.checking } }]
		},
		{ op: 'closeAccount', id: accounts.oldChecking },
		{ op: 'setPreference', id: 'budgetType', value: 'tracking' },
		{ op: 'setBudgetAmount', month: '2026-10', category: categories.groceries, amount: 15000 },
		{ op: 'setBudgetAmount', month: '2026-10', category: categories.rent, amount: 180000 },
		{ op: 'setPreference', id: 'budgetType', value: 'envelope' }
	);
	return ops;
};

export const allFeatures: FixtureSpec = { budgetName: 'Veritas All Features', build };
