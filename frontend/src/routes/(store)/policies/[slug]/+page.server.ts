import { error } from '@sveltejs/kit';
import {
	FREE_SHIPPING_OVER_CENTS,
	SHIPPING_CENTS
} from '#lib/components/store/shipping';
import { money } from '#lib/format';
import type { PageServerLoad } from './$types';

type PolicySection = { heading: string; body: string };

type Policy = {
	title: string;
	description: string;
	sections: PolicySection[];
};

const POLICIES: Record<string, Policy> = {
	shipping: {
		title: 'Shipping',
		description: 'How orders are packed, what delivery costs and how cash on delivery works.',
		sections: [
			{
				heading: 'Dispatch',
				body: 'We pack orders in Algiers and dispatch within two working days. The courier then carries the parcel across Algeria. If an order needs longer, we write to the email on the order.'
			},
			{
				heading: 'Cost',
				body: `Shipping is ${money(SHIPPING_CENTS)}. Orders over ${money(FREE_SHIPPING_OVER_CENTS)} ship free. The total is shown in the bag and again at checkout before the order is placed.`
			},
			{
				heading: 'Cash on delivery',
				body: 'Cash on delivery is the only method. You pay the courier when the parcel arrives. This site does not take card details and no card gateway is connected.'
			},
			{
				heading: 'Before delivery',
				body: 'We call the phone number on the order to confirm it before dispatch. The courier also calls before delivery. Keep the phone within reach on the day.'
			},
			{
				heading: 'Demo note',
				body: 'Azahar is a demonstration storefront. The catalog, prices and this policy are sample content, not a live delivery service.'
			}
		]
	},
	returns: {
		title: 'Returns',
		description: 'The window for returns, what can come back and how to start a return.',
		sections: [
			{
				heading: 'The window',
				body: 'You can return an order within 14 days of delivery. Write to hello@azahar.example with the order number and what is wrong before sending anything back.'
			},
			{
				heading: 'What can come back',
				body: 'Unopened formulas in their original packaging can come back. Opened skincare cannot be returned for hygiene reasons, unless the parcel arrived damaged or the wrong formula was sent.'
			},
			{
				heading: 'Damaged or wrong',
				body: 'If something arrived damaged or is not what you ordered, write to us with the order number and a photograph. We will arrange a replacement or a refund.'
			},
			{
				heading: 'Refunds',
				body: 'Once the parcel reaches us, we refund the price of the returned items. We agree the method with you, since cash on delivery orders are paid to the courier.'
			},
			{
				heading: 'Demo note',
				body: 'Azahar is a demonstration storefront. This returns policy is sample content and no real return can be booked through it.'
			}
		]
	},
	privacy: {
		title: 'Privacy',
		description: 'What this demo store keeps, where it lives and how to ask for a copy or deletion.',
		sections: [
			{
				heading: 'What we store',
				body: 'For an order, we store the name, email, phone number, delivery address, order contents, total and status. For an account, we also store your name, email and a password hash. We do not store card details.'
			},
			{
				heading: 'Cookies and local storage',
				body: 'One cookie, azahar_session, keeps you signed in. It is HttpOnly and SameSite=Lax. The bag is kept in your browser local storage, not in a cookie. There are no analytics or advertising scripts on this site.'
			},
			{
				heading: 'Where it lives',
				body: 'The catalog, accounts and orders are kept in Turso, a hosted SQLite service. The storefront and API run on the same server. Order confirmation links are stored only as a SHA-256 hash.'
			},
			{
				heading: 'Your choices',
				body: 'Write to hello@azahar.example to ask for a copy of the account data we hold, or to close the account. Order records stay while the demo runs so the order history remains consistent.'
			},
			{
				heading: 'Demo note',
				body: 'Azahar is a demonstration storefront. Do not send real personal data. This page describes the demo build and is not a legal compliance claim.'
			}
		]
	},
	terms: {
		title: 'Terms',
		description: 'The terms for a demonstration storefront with sample products and cash on delivery.',
		sections: [
			{
				heading: 'A demonstration store',
				body: 'Azahar is a demonstration storefront. The products, prices, photographs, story and policies are sample content. An order placed here does not create a binding sale.'
			},
			{
				heading: 'Orders and stock',
				body: 'Prices are in Algerian dinar and stored as centimes. Stock is checked again at checkout. If a formula is no longer available, the order is declined with a message and nothing is charged.'
			},
			{
				heading: 'Payment',
				body: 'Cash on delivery is the only method. You pay the courier when the order arrives. No card details are collected and no card gateway is connected.'
			},
			{
				heading: 'Content and photographs',
				body: 'The photographs are licensed stand-ins from Pexels and the bottles in them are blank mockups. They are credited on the credits page. The product names and descriptions are sample content.'
			},
			{
				heading: 'Changes',
				body: 'These terms may change as the demo is built. The page is not legal advice and makes no claim of certification or compliance.'
			}
		]
	}
};

export const load: PageServerLoad = ({ params }) => {
	const policy = POLICIES[params.slug];

	if (!policy) {
		error(404, 'That page is not here.');
	}

	return {
		slug: params.slug,
		policy
	};
};
