import type { FixtureSpec } from './builders.ts';

// A brand-new budget exactly as Actual makes one, with no edits. Veritas
// copies its db to create budgets of its own (`cargo xtask template`).
export const template: FixtureSpec = {
	budgetName: 'New Budget',
	build: () => [],
	freshBudget: true
};
