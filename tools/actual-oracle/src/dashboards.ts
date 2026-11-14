import type { CanonicalValue } from './canonicalJson.ts';
import type { ActualSession } from './environment.ts';

// What Actual's dashboard page and report list read: live pages, live widgets
// with their meta parsed, and the saved reports through report/get.
export const dashboardReadback = async ({ api, internal }: ActualSession) => {
	const query = async (table: string) => {
		const { data } = (await api.aqlQuery(api.q(table).select('*'))) as { data: any[] };
		return data.filter(row => !row.tombstone);
	};
	return {
		pages: await query('dashboard_pages'),
		widgets: await query('dashboard'),
		reports: await internal.send('report/get')
	} as unknown as CanonicalValue;
};
