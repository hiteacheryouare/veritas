import { primeIds, takeUnusedPrimedIds } from './environment.ts';
import type { ActualSession } from './environment.ts';

export interface Op {
	op: string;
	[field: string]: any;
}

export type TransactionRef = string | { transferOf: string };

type OpHandler = (session: ActualSession, op: Op) => Promise<unknown>;

const required = (op: Op, ...fields: string[]) => {
	const missing = fields.filter(field => op[field] === undefined);
	if (missing.length > 0) {
		throw new Error(`missing field(s): ${missing.join(', ')}`);
	}
};

const createWithId = async (session: ActualSession, table: string, id: string, create: () => Promise<unknown>) => {
	takeUnusedPrimedIds();
	primeIds(id);
	await create();
	const unused = takeUnusedPrimedIds();
	if (unused.length > 0) {
		throw new Error(`Actual never asked for an id, so ${id} was not used`);
	}
	const row = await session.internal.db.first(`SELECT id FROM ${table} WHERE id = ?`, [id]);
	if (!row) {
		throw new Error(`Actual allocated the supplied id ${id} to something other than a ${table} row`);
	}
};

const resolveTransaction = async ({ internal }: ActualSession, ref: TransactionRef): Promise<string> => {
	if (typeof ref === 'string') {
		return ref;
	}
	const row = await internal.db.first('SELECT transferred_id FROM transactions WHERE id = ?', [ref.transferOf]);
	if (!row?.transferred_id) {
		throw new Error(`Transaction ${ref.transferOf} has no transfer counterpart`);
	}
	return row.transferred_id;
};

const transferPayeeOf = async ({ internal }: ActualSession, account: string): Promise<string> => {
	const row = await internal.db.first('SELECT id FROM payees WHERE transfer_acct = ? AND tombstone = 0', [account]);
	if (!row) {
		throw new Error(`Account ${account} has no transfer payee`);
	}
	return row.id;
};

const resolvePayeeRefs = async (session: ActualSession, transaction: any): Promise<any> => {
	const resolved = { ...transaction };
	if (resolved.payee?.transferTo) {
		resolved.payee = await transferPayeeOf(session, resolved.payee.transferTo);
	}
	if (resolved.subtransactions) {
		resolved.subtransactions = await Promise.all(
			resolved.subtransactions.map((child: any) => resolvePayeeRefs(session, child))
		);
	}
	return resolved;
};

export const opHandlers: Record<string, OpHandler> = {
	createAccount: (session, op) => {
		required(op, 'id', 'name');
		return createWithId(session, 'accounts', op.id, () =>
			session.api.createAccount(
				{ name: op.name, offbudget: Boolean(op.offBudget), closed: Boolean(op.closed) },
				op.initialBalance
			)
		);
	},
	updateAccount: ({ api }, op) => {
		required(op, 'id', 'fields');
		return api.updateAccount(op.id, op.fields);
	},
	closeAccount: ({ api }, op) => {
		required(op, 'id');
		return api.closeAccount(op.id, op.transferAccount, op.transferCategory);
	},
	reopenAccount: ({ api }, op) => {
		required(op, 'id');
		return api.reopenAccount(op.id);
	},
	createAccountGroup: (session, op) => {
		required(op, 'id', 'name');
		return createWithId(session, 'account_groups', op.id, () => session.api.createAccountGroup({ name: op.name }));
	},

	createCategoryGroup: (session, op) => {
		required(op, 'id', 'name');
		return createWithId(session, 'category_groups', op.id, () =>
			session.api.createCategoryGroup({ name: op.name, hidden: Boolean(op.hidden) })
		);
	},
	updateCategoryGroup: ({ api }, op) => {
		required(op, 'id', 'fields');
		return api.updateCategoryGroup(op.id, op.fields);
	},
	createCategory: (session, op) => {
		required(op, 'id', 'name', 'group');
		return createWithId(session, 'categories', op.id, () =>
			session.api.createCategory({
				name: op.name,
				group_id: op.group,
				is_income: Boolean(op.isIncome),
				hidden: Boolean(op.hidden)
			})
		);
	},
	updateCategory: ({ api }, op) => {
		required(op, 'id', 'fields');
		return api.updateCategory(op.id, op.fields);
	},
	deleteCategory: ({ api }, op) => {
		required(op, 'id');
		return api.deleteCategory(op.id, op.transferCategory);
	},

	createPayee: (session, op) => {
		required(op, 'id', 'name');
		return createWithId(session, 'payees', op.id, () => session.api.createPayee({ name: op.name }));
	},
	mergePayees: ({ api }, op) => {
		required(op, 'target', 'merge');
		return api.mergePayees(op.target, op.merge);
	},
	createPayeeLocation: (session, op) => {
		required(op, 'id', 'payee', 'latitude', 'longitude');
		return createWithId(session, 'payee_locations', op.id, () =>
			session.internal.send('api/payee-location-create', {
				payeeId: op.payee,
				latitude: op.latitude,
				longitude: op.longitude
			})
		);
	},

	addTransactions: async (session, op) => {
		required(op, 'account', 'transactions');
		const transactions = await Promise.all(op.transactions.map((t: any) => resolvePayeeRefs(session, t)));
		return session.api.addTransactions(op.account, transactions, {
			runTransfers: op.runTransfers ?? true,
			learnCategories: op.learnCategories ?? false
		});
	},
	updateTransaction: async (session, op) => {
		required(op, 'id', 'fields');
		const fields = await resolvePayeeRefs(session, op.fields);
		return session.api.updateTransaction(await resolveTransaction(session, op.id), fields);
	},
	deleteTransaction: async (session, op) => {
		required(op, 'id');
		return session.api.deleteTransaction(await resolveTransaction(session, op.id));
	},

	setBudgetAmount: ({ api }, op) => {
		required(op, 'month', 'category', 'amount');
		return api.setBudgetAmount(op.month, op.category, op.amount);
	},
	setCarryover: ({ api }, op) => {
		required(op, 'month', 'category', 'flag');
		return api.setBudgetCarryover(op.month, op.category, op.flag);
	},
	holdForNextMonth: ({ api }, op) => {
		required(op, 'month', 'amount');
		return api.holdBudgetForNextMonth(op.month, op.amount);
	},
	resetHold: ({ api }, op) => {
		required(op, 'month');
		return api.resetBudgetHold(op.month);
	},
	transferAvailable: ({ internal }, op) => {
		required(op, 'month', 'amount', 'category');
		return internal.send('budget/transfer-available', { month: op.month, amount: op.amount, category: op.category });
	},
	moveBudget: ({ internal }, op) => {
		required(op, 'month', 'from', 'to', 'amount');
		return internal.send('budget/transfer-category', {
			month: op.month,
			amount: op.amount,
			from: op.from,
			to: op.to,
			currencyCode: op.currencyCode ?? ''
		});
	},
	coverOverspending: ({ internal }, op) => {
		required(op, 'month', 'from', 'to');
		return internal.send('budget/cover-overspending', {
			month: op.month,
			from: op.from,
			to: op.to,
			amount: op.amount,
			currencyCode: op.currencyCode ?? ''
		});
	},
	applyTemplates: ({ internal }, op) => {
		required(op, 'month');
		return internal.send('budget/apply-goal-template', { month: op.month });
	},
	storeNoteCleanups: ({ internal }) => internal.send('budget/store-note-cleanups'),

	createRule: ({ api }, op) => {
		required(op, 'id', 'conditions', 'actions');
		return api.createRule({
			id: op.id,
			stage: op.stage ?? null,
			conditionsOp: op.conditionsOp ?? 'and',
			conditions: op.conditions,
			actions: op.actions
		});
	},
	createSchedule: (session, op) => {
		required(op, 'id', 'name', 'date');
		return createWithId(session, 'schedules', op.id, () =>
			session.api.createSchedule({
				name: op.name,
				posts_transaction: Boolean(op.postsTransaction),
				payee: op.payee ?? null,
				account: op.account ?? null,
				amount: op.amount,
				amountOp: op.amountOp ?? 'is',
				date: op.date
			})
		);
	},
	runSchedules: ({ internal }) => internal.send('schedule/force-run-service'),

	setNote: ({ api }, op) => {
		required(op, 'id', 'note');
		return api.updateNote(op.id, op.note);
	},
	createTag: (session, op) => {
		required(op, 'id', 'tag');
		return createWithId(session, 'tags', op.id, () =>
			session.api.createTag({ tag: op.tag, color: op.color ?? null, description: op.description ?? null })
		);
	},
	createCustomReport: (session, op) => {
		required(op, 'id', 'report');
		return createWithId(session, 'custom_reports', op.id, () => session.internal.send('report/create', op.report));
	},
	createDashboardPage: (session, op) => {
		required(op, 'id', 'name');
		return createWithId(session, 'dashboard_pages', op.id, () =>
			session.internal.send('dashboard-create', { name: op.name })
		);
	},
	addDashboardWidget: async ({ internal }, op) => {
		required(op, 'id', 'page', 'type', 'width', 'height');
		const { op: _op, page, ...widget } = op;
		await internal.send('dashboard-add-widget', { ...widget, meta: op.meta ?? null, dashboard_page_id: page });
		if (!(await internal.db.first('SELECT id FROM dashboard WHERE id = ?', [op.id]))) {
			throw new Error(`Actual did not keep the supplied widget id ${op.id}`);
		}
	},
	createFilter: (session, op) => {
		required(op, 'id', 'name', 'conditions');
		return createWithId(session, 'transaction_filters', op.id, () =>
			session.internal.send('filter-create', {
				state: { name: op.name, conditions: op.conditions, conditionsOp: op.conditionsOp ?? 'and' },
				filters: []
			})
		);
	},
	setPreference: ({ api }, op) => {
		required(op, 'id', 'value');
		return api.setPreference(op.id, op.value);
	}
};

export const parseOpFile = (text: string): Op[] => {
	const parsed = JSON.parse(text);
	if (parsed?.version !== 1 || !Array.isArray(parsed.ops)) {
		throw new Error('An op file must look like {"version": 1, "ops": [...]}');
	}
	return parsed.ops;
};

export const applyOps = async (session: ActualSession, ops: Op[]) => {
	for (const [index, op] of ops.entries()) {
		const handler = opHandlers[op.op];
		if (!handler) {
			throw new Error(`Op #${index} has unknown type "${op.op}"`);
		}
		try {
			await handler(session, op);
		} catch (error) {
			throw new Error(`Op #${index} (${op.op}) failed: ${(error as Error).message}`, { cause: error });
		}
	}
};
