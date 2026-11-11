# Azahar design direction

Azahar is a demo skincare house. The products, prices, story and photographs are sample
content for this build, not a real business. No testimonials, certifications, clinical
results or performance statistics are shown.

## Design reads

Storefront: a direct-to-consumer botanical skincare house for thoughtful buyers, in a warm
editorial language, dial ENERGY 2 / RHYTHM 2 / MOTION 2.

Dashboard: a working financial instrument for the same house, in an expressive dark
botanical language, dial ENERGY 3 / RHYTHM 3 / MOTION 2.

MOTION 2 means one entrance per page, one or two dealt moments, and micro-interactions on
controls and product cards. No scroll-linked card fades, no parallax on text, no endless
loops.

## Palette

Dealt by the taste skill for a product page in the wellness trade. The first palette is
used as printed.

- Ground `#f6f0eb`, panels `#ebe0d9`
- Ink `#2c2325`, muted `#6a5d5e`
- Accent `#9a3c4c`, accent ink `#ffffff`
- One accent only: the primary button, the active filter, one word in the statement.

Why it fits: pale plaster and warm near-black are the wall and the shelf of an apothecary.
The deep rose is the one saturated note, the colour of the petals and the label band, and
it appears once per screen at most.

Contrast, measured with the antislop contrast checker:

- ink on ground 13.52:1
- muted on ground 5.56:1
- white on accent 6.72:1
- accent on ground 5.95:1

### Store theme

The store ships a working light and dark mode. Light is the plaster-and-rose default.
Dark is the same house at night:

- Ground `#171311`, panels `#221b18`
- Ink `#f4ece5`, muted `#b9aca4`
- Accent `#e0a0a8`, accent ink `#241316`
- Contrast: ink 15.80:1, muted 8.35:1, accent 8.58:1, accent ink 8.28:1.

The choice is saved in `localStorage`, follows `prefers-color-scheme` on the first visit,
and a first-party `theme-init.js` applies it before paint so the page does not flash light.
The toggle is a 44px circular button in the header, with a moon for light and a sun for dark.

## Type

Dealt pairing: Schibsted Grotesk for display and text, with JetBrains Mono for figures,
order numbers and small technical labels.

- Display weight 400, tracking `-0.035em`, hero `-0.042em`.
- Text weight 400, labels and buttons 500.
- Sentence case everywhere. No large monospace headings, no wide-tracked uppercase labels.
- The wordmark is the name set as text in Schibsted Grotesk 600. No emblem.

Why: Schibsted Grotesk has the sturdy, plain-spoken voice of a label printed by a small
press. It is not a luxury serif costume and it does not read as a generic startup sans.
Mono is reserved for numbers, where fixed widths help scanning.

## Layout

- One section per screen at desktop. The hero is alone on the first screen.
- A 12 column wrap, 1280px wide, equal left and right edges, 24px gutters.
- Product cards are 4:5 photographs; the photograph is the object, not a panel.
- Radii: 2px for inputs, 4px for cards, 6px for buttons and dialogs. No pill shapes.
- No glass, no glow, no background grid. One shadow level, used only for the cart toast
  and the admin modal.
- Whitespace separates sections and sets the rhythm. Section padding is 120px desktop,
  72px tablet, 48px phone.

Why: the storefront is a shelf, not a dashboard. The grid keeps the product photographs
aligned and lets the one editorial moment break the rhythm.

## Photography

Real photographs from Pexels, used under the Pexels license, and credited in one footer
line. The product bottles are blank stand-ins. They are not Azahar's own product shots.
The report lists the shot list for a real shoot.

Treatment: one warm grade across the set, soft window light, plaster, linen, amber glass,
matte ceramic, rose petals, olive oil, oranges. No people passed off as customers or staff.

## Motion

- Hero entrance once on load: the head row settles 8px, the photograph unmasks upward.
- Hover: linked cards lift 2px, photographs zoom 1.02, buttons step 1px and press back.
- Micro: header bag count ticks, add-to-bag toast slides in, cart quantities update in place.
- Theme toggle: a 640ms circular clip-path reveal from the centre of the toggle button to the
  farthest corner. The View Transitions API snapshots the old and new themes, so the real
  elements are revealed as the circle grows, without a reload. Browsers without the API get
  a solid ground wipe. Reduced motion switches instantly.
- Dealt moment one, home: a statement whose words move from muted to ink as it crosses the
  middle of the viewport. Purpose: pace the reading of the house line.
- Dealt moment two, home: three pinned chapters about the grove, the press and the jar. The
  right frame crossfades as each chapter reaches its mark. Purpose: connect the shelf to
  the material.
- Dashboard: chart path draws once, figures count up once, preset switch crossfades the
  tokens over 240ms, sidebar active marker slides. Purpose: show that the data and the
  theme changed, not decoration.

All of it rests under `prefers-reduced-motion: reduce`. Nothing moves longer than 5
seconds, so no pause control is needed.

## Dashboard

The screen's job: answer "what needs attention today, and how did the house do this
month?" The revenue chart and the attention rail are the focal points. Four equal stat
cards are not used.

- Shell: compact left rail, wordmark, five destinations, preset switch at the foot.
- Overview: a wide revenue panel with one large figure and a real previous-period delta; a
  narrow rail with orders to pack, low stock and new customers; top products; order mix;
  recent orders with real status controls.
- Orders: status tabs, search, table, detail drawer, status transition control.
- Products: table with stock and price visible, create and edit drawer, archive with a
  confirmation dialog.
- Customers: email, orders, spend, last order. No invented avatars.
- Demo data is labelled "Demo dataset" in the header. The numbers come from the seeded
  orders in Turso, not from random values in the client.

### Colour presets

The preset changes every token in the admin, including chart series, focus ring and
selection. It is stored per admin user in Turso and mirrored to local storage for an
instant, flash-free load. All four pass AA for body text and 3:1 for chart strokes against
their grounds.

| Preset | Ground | Surface | Ink | Muted | Accent | Chart series |
|---|---|---|---|---|---|---|
| Plaster | `#f6f0eb` | `#ebe0d9` | `#2c2325` | `#6a5d5e` | `#9a3c4c` | rose, pine, ochre, stone |
| Cellar | `#151a16` | `#1d241e` | `#f1ece1` | `#a3ab9f` | `#d8a44c` | saffron, sage, clay, bone |
| Terracotta | `#f2e5d8` | `#e5d1bf` | `#2b1d18` | `#6e554a` | `#a94e2b` | clay, pine, ochre, umber |
| Lagoon | `#0e1d23` | `#152b33` | `#e9f1ef` | `#9ab0b1` | `#5fb7b2` | aqua, saffron, clay, mist |

Why presets: the owner reads the same numbers in different light, and the preset is a real
preference, not a decoration. It is saved, not lost on reload.

## Voice

Plain, tactile, specific. Short sentences. Materials and actions, not promises. No
buzzwords, no em dashes, no exclamation marks. Buttons name the action and the object:
"Add the oil", "Place the order", "Read the ingredients". The one hard fact on the first
screen is the size of the shelf: eight formulas.

## Assumptions

- Working name: Azahar. It is a demo brand, set as a text wordmark.
- Prices are in Algerian dinar, stored as centimes.
- Payment is cash on delivery. No card gateway is connected.
- Contact is `hello@azahar.example`. No phone number, street number or registration number.
- The photographs are licensed stand-ins, not the client's own product shots.
