# Permissions

Three roles exist: `customer`, `employee` and `admin`. The role lives on the user row and is
checked on the server; hiding a button in the admin UI is never the control.

| Surface | Customer | Employee | Admin |
|---|---|---|---|
| Storefront, own account, own orders | Yes | Yes | Yes |
| Admin overview and metrics | No | Read | Read |
| Admin products | No | Read | Read, create, edit, archive |
| Record a counter sale | No | Yes | Yes |
| Change stock directly | No | No | Yes |
| Product image upload | No | No | Yes |
| Admin orders | No | Read, advance status, create shipments, refresh tracking | Full |
| Cancel an order | No | No | Yes |
| Admin customers | No | Read | Read |
| Admin personal colour style | No | Yes | Yes |
| Team and role assignment | No | No | Yes |
| Storefront theme editor | No | No | Yes |
| Plugin list, settings and enable/disable | No | No | Yes |
| Plugin admin routes | No | No | Yes |
| Code mode and AI analysis | No | No | Yes |

## Server mapping

- Every `/api/admin/*` route starts with `requireUser` and `requireStaff`.
- `requireStaff` accepts `admin` and `employee`.
- Routes that change catalogue, storefront, plugins or settings add `requireAdmin`:
  product create/update/archive, image upload, theme reads and writes, plugin reads,
  settings, state changes and plugin routes.
- `POST /api/admin/sales` is available to staff, so an employee at the counter can take a cash sale. It moves stock and runs plugin hooks.
- `PATCH /api/admin/products/{id}` (including the stock field) is admin-only, so an employee cannot silently change the catalogue.
- `PATCH /api/admin/orders/{id}/status` is available to staff, but `cancelled` is rejected
  for employees because cancellation restocks products and releases IMEIs.
- `GET`/`PUT /api/admin/preferences` is personal and available to both staff roles.
- `GET /api/admin/team` and `PATCH /api/admin/team/{id}` are admin-only. The team screen can
  promote a customer to employee or administrator, or demote a staff account. An
  administrator cannot remove their own role, and the last administrator cannot be demoted.
  Changing a role takes effect on the next request because the session user is reloaded from
  the database each time.
- Plugin admin routes remain `requireAdmin`; employees cannot call them even when the plugin
  is enabled.

## UI mapping

- The admin shell allows both staff roles. Employees see Overview, Orders, Products and
  Customers; Storefront, Team and Plugins are hidden and show an "Administrators only" panel
  if reached directly.
- The products table is read-only for employees: no New product, Edit or Archive controls.
- The order status dropdown omits cancellation for employees; shipment and tracking remain.
- The plugin detail screen and the storefront editor are admin-only.
- The seeded demo employee is `staff@azahar.example` / `azahar-dev-staff-2026`. The seeder
  only creates it when the user table is empty; existing deployments can set the role on a
  user row to `employee`.
