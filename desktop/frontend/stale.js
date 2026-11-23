// The one staleness guard for async renders. A screen captures the render epoch
// after it paints its loading state; every later setView or resetTransientState
// bumps the epoch, so the captured check fails and the late response is dropped.

/**
 * @param {{ render: number }} state
 * @returns {() => boolean} True while no newer render started.
 */
export function captureRender(state) {
	const token = state.render;
	return () => token === state.render;
}
