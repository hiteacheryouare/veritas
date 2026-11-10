const icons = {
	budget: '<rect x="3" y="6" width="18" height="13" rx="2"/><path d="M3 8l9 6 9-6"/>',
	accounts: '<path d="M3 10l9-5 9 5M5 10v7M10 10v7M14 10v7M19 10v7M3 20h18"/>',
	bills: '<rect x="3.5" y="5" width="17" height="15" rx="2.5"/><path d="M3.5 10h17M8 3v4M16 3v4"/>',
	goals: '<circle cx="12" cy="12" r="8.5"/><circle cx="12" cy="12" r="4.5"/><circle cx="12" cy="12" r="0.8"/>',
	reports: '<path d="M5 20v-9M10 20V5M15 20v-7M20 20V8"/>',
	insights: '<path d="M3 17l5-5 4 3 4-6"/><path d="M16 9l5-3" stroke-dasharray="2 2.5"/>',
	settings: '<path d="M4 7h10M18 7h2M4 17h4M12 17h8"/><circle cx="16" cy="7" r="2"/><circle cx="10" cy="17" r="2"/>',
	'chev-l': '<path d="M14.5 6l-6 6 6 6"/>',
	'chev-r': '<path d="M9.5 6l6 6-6 6"/>',
	'chev-down': '<path d="M7 10l5 5 5-5"/>',
	'chev-updown': '<path d="M8 9.5l4-4 4 4M8 14.5l4 4 4-4"/>',
	search: '<circle cx="10.5" cy="10.5" r="6"/><path d="M15 15l5 5"/>',
	over: '<path d="M12 4l9 16H3z"/><path d="M12 10v4M12 17v.3"/>',
	low: '<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5v7M8.8 11.5L12 14.7l3.2-3.2"/>',
	check: '<circle cx="12" cy="12" r="8.5"/><path d="M8 12.5l2.7 2.7L16 9.5"/>',
	income: '<path d="M12 4v10M7.5 9.5L12 14l4.5-4.5"/><path d="M4 15v3a2 2 0 002 2h12a2 2 0 002-2v-3"/>',
	move: '<path d="M4 8h14M14.5 4.5L18 8l-3.5 3.5M20 16H6M9.5 12.5L6 16l3.5 3.5"/>',
	dense: '<path d="M4 6h16M4 10h16M4 14h16M4 18h16"/>',
	plus: '<path d="M12 5v14M5 12h14"/>',
	clock: '<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/>',
	auto: '<path d="M5 12a7 7 0 0112-4.9M19 12a7 7 0 01-12 4.9"/><path d="M17 3.5v4h-4M7 20.5v-4h4"/>',
	ask: '<circle cx="12" cy="12" r="8.5"/><path d="M9.6 9.5a2.5 2.5 0 114 2c-.9.6-1.6 1.1-1.6 2.2M12 17v.3"/>',
	drill: '<path d="M9.5 6l6 6-6 6"/>',
	share: '<path d="M12 3.5v11M8 7.5l4-4 4 4"/><path d="M6 11v8.5h12V11"/>',
	hide: '<path d="M3 12s3.5-6 9-6 9 6 9 6-3.5 6-9 6-9-6-9-6z"/><circle cx="12" cy="12" r="2.5"/><path d="M4 20L20 4"/>',
	export: '<path d="M14 3.5H7a2 2 0 00-2 2v13a2 2 0 002 2h10a2 2 0 002-2V8.5z"/><path d="M14 3.5v5h5M12 11v6M9.5 14.5L12 17l2.5-2.5"/>',
	archive: '<rect x="3.5" y="4.5" width="17" height="4" rx="1"/><path d="M5 8.5v10a1.5 1.5 0 001.5 1.5h11a1.5 1.5 0 001.5-1.5v-10M10 12.5h4"/>',
	spent: '<path d="M4 7h16v11H4z"/><path d="M4 11h16M8 15h3"/>',
	close: '<path d="M6 6l12 12M18 6L6 18"/>',
	play: '<path d="M8 5.5v13l10-6.5z"/>',
	pause: '<path d="M8 5v14M16 5v14"/>',
	tag: '<path d="M4 4h7.5L20 12.5 12.5 20 4 11.5z"/><circle cx="8.5" cy="8.5" r="1.3"/>',
	calendar: '<rect x="3.5" y="5" width="17" height="15" rx="2.5"/><path d="M3.5 10h17M8 3v4M16 3v4"/>'
};

const sprite = `<svg width="0" height="0" style="position:absolute" aria-hidden="true">${Object.entries(icons).map(([name, body]) => `<symbol id="i-${name}" viewBox="0 0 24 24">${body}</symbol>`).join('')}</svg>`;
document.body.insertAdjacentHTML('afterbegin', sprite);

const icon = (name) => `<svg class="icon"><use href="#i-${name}"/></svg>`;

const accounts = [
	['Cash', [['checking', 'Checking', '2,140.18'], ['savings', 'Savings', '8,412.00']]],
	['Credit', [['visa', 'Visa', '−612.33']]],
	['Investments', [['brokerage', 'Brokerage', '24,310.55']]],
	['Loans', [['car-loan', 'Car loan', '−9,800.00']]]
];

const sections = [
	['budget', 'Budget', []],
	['accounts', 'Accounts', []],
	['bills', 'Bills', []],
	['goals', 'Goals', []],
	['reports', 'Reports', [['dashboard', 'Dashboard'], ['net-profit', 'Net Profit'], ['sankey', 'Sankey'], ['custom', 'Custom reports']]],
	['insights', 'Insights', [['forecast', 'Forecast'], ['what-ifs', 'What-ifs'], ['wrapped', 'Wrapped'], ['statements', 'Statements'], ['tax', 'Tax']]]
];

const renderSidebar = (nav) => {
	const [section, sub] = (nav.dataset.current || 'budget').split('/');
	const parts = ['<div class="traffic"><span></span><span></span><span></span></div>'];
	sections.forEach(([id, label, children]) => {
		const current = id === section && !sub ? ' current' : '';
		parts.push(`<a class="nav-item${current}">${icon(id)}${label}</a>`);
		if (id === 'accounts') {
			accounts.forEach(([group, rows]) => {
				parts.push(`<div class="account-group">${group}</div>`);
				rows.forEach(([key, name, balance]) => {
					const here = section === 'accounts' && sub === key ? ' current' : '';
					parts.push(`<a class="account${here}"><span>${name}</span><span class="fig">${balance}</span></a>`);
				});
			});
		}
		if (id === section && children.length) {
			children.forEach(([key, name]) => {
				parts.push(`<a class="nav-sub${sub === key ? ' current' : ''}">${name}</a>`);
			});
		}
	});
	parts.push('<div class="sidebar-spacer"></div>', `<a class="nav-item${section === 'settings' ? ' current' : ''}">${icon('settings')}Settings</a>`);
	nav.innerHTML = parts.join('');
};

document.querySelectorAll('nav.sidebar').forEach(renderSidebar);

document.querySelectorAll('.stage[data-theme="light"]').forEach((stage) => {
	const dark = stage.cloneNode(true);
	dark.dataset.theme = 'dark';
	stage.after(dark);
});

const params = new URLSearchParams(location.search);

const setView = (view) => {
	document.body.dataset.view = view;
	document.querySelectorAll('.view-switch[aria-label="Appearance"] button').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.view === view)));
};

const bar = document.querySelector('.page-bar');
if (bar) {
	bar.insertAdjacentHTML('afterbegin', '<a href="index.html">All screens</a>');
	bar.insertAdjacentHTML('beforeend', `<div class="bar-controls"><div class="view-switch" role="group" aria-label="Appearance"><button type="button" data-view="both">Both</button><button type="button" data-view="light">Light</button><button type="button" data-view="dark">Dark</button></div></div>`);
	bar.querySelectorAll('[data-view]').forEach((b) => b.addEventListener('click', () => setView(b.dataset.view)));
}

if (params.has('embed')) {
	document.body.classList.add('embed');
	const theme = params.get('view') || 'light';
	const stages = [...document.querySelectorAll(`.stage[data-theme="${theme}"]`)];
	stages.slice(1).forEach((s) => s.classList.add('hidden-in-embed'));
}
setView(params.get('view') || 'both');

const fit = () => {
	const room = document.body.classList.contains('embed') ? innerWidth : innerWidth - 64;
	document.querySelectorAll('.window').forEach((w) => { w.style.zoom = Math.min(1, room / 1280); });
};
addEventListener('resize', fit);
fit();
