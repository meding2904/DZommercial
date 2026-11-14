import { error } from '@sveltejs/kit';
import {
	FREE_SHIPPING_OVER_CENTS,
	SHIPPING_CENTS
} from '#lib/components/store/shipping';
import { money } from '#lib/format';
import { TERMS_VERSION } from '#lib/legal';
import type { PageServerLoad } from './$types';

type PolicySection = { heading: string; body: string };

type Policy = {
	title: string;
	description: string;
	version?: string;
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
				body: 'For an order, we store the name, email, phone number, delivery address, order notes, order contents, total and status. For an account, we also store your name, email and a password hash. We do not store card details.'
			},
			{
				heading: 'Cookies and local storage',
				body: 'Two cookies are set. azahar_session keeps you signed in; it is HttpOnly and SameSite=Lax. azahar_csrf guards forms against cross-site requests; it is readable by the page and lasts seven days. The bag and your theme choice are kept in your browser local storage, not in a cookie. There are no analytics or advertising scripts on this site.'
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
		title: 'Terms of service',
		description:
			'The terms for this demonstration storefront, its sample orders and the limits of what it promises.',
		version: TERMS_VERSION,
		sections: [
			{
				heading: 'A demonstration, not a live shop',
				body: 'Azahar is a demonstration storefront. The products, prices, photographs, story and policies are sample content. An order placed here does not create a binding sale, a delivery obligation or a payment obligation. Do not use this demo for a real purchase or send real personal data.'
			},
			{
				heading: 'Orders and acceptance',
				body: 'An order is an offer to buy sample goods. The demo may decline or cancel an order at any time, including when stock is unavailable. No contract is formed until a real shop confirms the order in writing. You confirm that the delivery details you enter are yours to use and that you can receive the order.'
			},
			{
				heading: 'Prices, stock and payment',
				body: 'Prices are in Algerian dinar and stored as centimes. The server checks stock again at checkout. Cash on delivery is the only method: you pay the courier when the parcel arrives. No card details are collected and no card gateway is connected.'
			},
			{
				heading: 'Delivery',
				body: 'Delivery addresses and courier details are sample data. A real courier service, delivery time or parcel is not guaranteed. The shipping page explains the sample costs and the demo limits.'
			},
			{
				heading: 'Returns and cancellations',
				body: 'The returns page describes the sample return window and what can come back. Because this is a demo, no real return, refund or replacement can be booked through it.'
			},
			{
				heading: 'Product information',
				body: 'Descriptions, ingredients and how-to text are sample content. They are not medical advice and make no claim to diagnose, treat or cure a condition. A real product should be patch tested and used under the guidance of a qualified professional.'
			},
			{
				heading: 'Limitation of liability',
				body: 'To the fullest extent permitted by law, the demo owner is not liable for indirect or consequential loss, loss of data, or loss arising from reliance on sample content, a sample order or a courier detail. Nothing in these terms excludes or limits liability that cannot be excluded or limited by law, including liability for death or personal injury caused by negligence, or for fraud.'
			},
			{
				heading: 'Privacy',
				body: 'The privacy page explains what this demo stores and how to ask for a copy or deletion. Do not send real personal data. The demo keeps order records so the admin views stay consistent.'
			},
			{
				heading: 'Changes and version',
				body: `These terms may change as the demo is built. The checkout records the version accepted with an order. This version is ${TERMS_VERSION}. A change takes effect when the new version is published on this page.`
			},
			{
				heading: 'Not legal advice',
				body: 'These are sample terms for a demonstration and are not legal advice. A real shop must have its terms, privacy notice and consumer information reviewed under the law that applies to it before taking real orders.'
			},
			{
				heading: 'Contact',
				body: 'Questions about these terms: hello@azahar.example.'
			}
		]
	}
};

export const load: PageServerLoad = ({ params }) => {
	const policy = Object.hasOwn(POLICIES, params.slug) ? POLICIES[params.slug] : undefined;

	if (!policy) {
		error(404, 'That page is not here.');
	}

	return {
		slug: params.slug,
		policy
	};
};
