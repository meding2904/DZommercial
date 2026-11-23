import test from 'node:test';
import assert from 'node:assert/strict';

import { orderActionPlan } from '../order-actions.js';

test('order steps follow the shared transition table without cancel in the flow', () => {
	const plan = orderActionPlan({ status: 'pending', delivery_type: 'home' }, 'admin');
	assert.deepEqual(plan.steps, ['confirmed']);
	assert.equal(plan.canCancel, true);
	assert.equal(plan.showCourier, true);
	assert.equal(plan.hasTracking, false);

	const delivered = orderActionPlan({ status: 'delivered', delivery_type: 'home' }, 'admin');
	assert.deepEqual(delivered.steps, []);
	assert.equal(delivered.canCancel, false);
});

test('only an administrator can cancel an order', () => {
	const employee = orderActionPlan({ status: 'pending', delivery_type: 'home' }, 'employee');
	assert.equal(employee.canCancel, false);
	assert.deepEqual(employee.steps, ['confirmed']);
});

test('pickup orders offer no courier actions', () => {
	const plan = orderActionPlan({ status: 'packed', delivery_type: 'pickup' }, 'admin');
	assert.equal(plan.showCourier, false);
	assert.equal(plan.hasTracking, false);
});

test('a courier parcel offers create and, once tracked, refresh', () => {
	const untracked = orderActionPlan({ status: 'packed', delivery_type: 'stopdesk' }, 'admin');
	assert.equal(untracked.showCourier, true);
	assert.equal(untracked.hasTracking, false);

	const tracked = orderActionPlan(
		{ status: 'shipped', delivery_type: 'home', tracking_number: 'TRK-1' },
		'admin'
	);
	assert.equal(tracked.showCourier, true);
	assert.equal(tracked.hasTracking, true);
});
