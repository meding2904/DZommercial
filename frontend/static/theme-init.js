(function () {
	try {
		var saved = localStorage.getItem('azahar-store-theme');
		var theme =
			saved === 'light' || saved === 'dark'
				? saved
				: window.matchMedia('(prefers-color-scheme: dark)').matches
					? 'dark'
					: 'light';
		document.documentElement.setAttribute('data-store-theme', theme);
		var meta = document.querySelector('meta[name="theme-color"]');
		if (meta) meta.setAttribute('content', theme === 'dark' ? '#171311' : '#f6f0eb');
	} catch (error) {
		document.documentElement.setAttribute('data-store-theme', 'light');
	}
})();
