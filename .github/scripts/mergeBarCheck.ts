import { evaluate, trustedSignoffs } from './mergeBar.ts';

const { GITHUB_TOKEN, GITHUB_REPOSITORY, PR_NUMBER, GITHUB_SERVER_URL, GITHUB_RUN_ID } = process.env;
if (!GITHUB_TOKEN || !GITHUB_REPOSITORY || !PR_NUMBER) {
	throw new Error('GITHUB_TOKEN, GITHUB_REPOSITORY and PR_NUMBER must be set');
}

const api = async (path: string, init: RequestInit = {}) => {
	const response = await fetch(`https://api.github.com/repos/${GITHUB_REPOSITORY}${path}`, {
		...init,
		headers: {
			accept: 'application/vnd.github+json',
			'content-type': 'application/json',
			authorization: `Bearer ${GITHUB_TOKEN}`,
			'x-github-api-version': '2022-11-28'
		}
	});
	if (!response.ok) {
		throw new Error(`${init.method ?? 'GET'} ${path}: ${response.status} ${await response.text()}`);
	}
	return response.json();
};

const paged = async (path: string) => {
	const items = [];
	for (let page = 1; ; page++) {
		const batch = await api(`${path}?per_page=100&page=${page}`);
		items.push(...batch);
		if (batch.length < 100) {
			return items;
		}
	}
};

const pull = await api(`/pulls/${PR_NUMBER}`);
const comments = await paged(`/issues/${PR_NUMBER}/comments`);
const reviews = await paged(`/pulls/${PR_NUMBER}/reviews`);
const posts = [...comments, ...reviews.filter((review) => review.state !== 'DISMISSED')]
	.map((post) => ({
		body: post.body,
		authorAssociation: post.author_association,
		at: post.submitted_at ?? post.created_at
	}))
	.sort((a, b) => a.at.localeCompare(b.at));

const verdict = evaluate(
	{
		baseRef: pull.base.ref,
		headRef: pull.head.ref,
		headSha: pull.head.sha,
		body: pull.body ?? '',
		labels: pull.labels.map((label: { name: string }) => label.name)
	},
	trustedSignoffs(posts)
);

await api(`/statuses/${pull.head.sha}`, {
	method: 'POST',
	body: JSON.stringify({
		state: verdict.state,
		context: 'merge-bar',
		description: verdict.description,
		target_url: `${GITHUB_SERVER_URL}/${GITHUB_REPOSITORY}/actions/runs/${GITHUB_RUN_ID}`
	})
});
console.log(`merge-bar: ${verdict.state}: ${verdict.description}`);
