import type { CanonicalValue } from './canonicalJson.ts';
import type { ActualSession } from './environment.ts';

interface ForecastTransaction {
	amount: number;
	scheduleId: string;
}

interface ForecastDataPoint {
	date: string;
	accountId: string;
	transactions: ForecastTransaction[];
}

// Every schedule occurrence Actual's own balance forecast books from `today`
// through `endDate` (`forecast/generate`, server/forecast/app.ts:57-170):
// the dates `getFutureOccurrenceDates` expands, the amount
// `getScheduledAmount` gives, after the rules, one entry per account side.
export const forecastOccurrences = async ({ internal }: ActualSession, endDate: string) => {
	const result = (await internal.send('forecast/generate', { endDate })) as { dataPoints: ForecastDataPoint[] };
	const occurrences: CanonicalValue[] = [];
	for (const point of result.dataPoints) {
		for (const transaction of point.transactions) {
			occurrences.push({
				date: point.date,
				account: point.accountId,
				schedule: transaction.scheduleId,
				amount: transaction.amount
			});
		}
	}
	return occurrences;
};
