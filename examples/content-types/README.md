# CRM and ERP content types

These examples provide a starter domain model for a CRM and an ERP application.
They are grouped with `metadata.namespace` and `metadata.labels.application` so
an eventual first-class Application registry can adopt them without changing
content-type UIDs.

## CRM

- `crm/organization.json`: customers, suppliers, partners, and prospects
- `crm/contact.json`: people associated with organizations
- `crm/customer-group.json`: customer segments used for shared pricing rules
- `crm/customer-group-membership.json`: effective-dated customer membership
- `crm/activity.json`: calls, emails, meetings, notes, and follow-up tasks

## ERP

- `erp/product.json`: products, services, prices, VAT, barcodes, and stock policy
- `erp/warehouse.json`: warehouses, stores, and transit locations
- `erp/inventory-item.json`: per-location stock balances and reorder points
- `erp/inventory-movement.json`: auditable receipts, issues, returns, transfers, and adjustments
- `erp/customer-price-rule.json`: customer-specific and customer-group-specific prices
- `erp/sales-invoice.json`: retail receipts, invoices, credit notes, and payment state
- `erp/sales-invoice-line.json`: itemized fiscal document lines
- `erp/purchase-invoice.json`: supplier invoices, purchase credit notes, and receiving data
- `erp/purchase-invoice-line.json`: itemized supplier invoice lines, lots, and expiry dates
- `erp/payment.json`: cash, card, bank, and provider payment transactions
- `erp/e-invoice-submission.json`: external electronic-invoicing and myDATA submission state

## Import

- `crm/import-bundle.json`: all CRM schemas
- `erp/import-bundle.json`: all ERP schemas plus the CRM `organization` and `customer-group` schemas required by ERP relations

Both bundles use the `ferriscms-content-types` version 1 format and can be imported directly through the content-type builder.

The schemas model business data. Provider integrations, fiscal transmission,
retry handling, notifications, and reconciliation should be implemented as
workflows around these records rather than encoded as content-type fields alone.

For pricing resolution, the application should select active rules matching the
product and quantity, then prefer a direct customer rule over a customer-group
rule, and finally fall back to `product.sale_price`. Within the same scope,
the highest priority rule wins, followed by the most specific quantity break.

`inventory-item` is the current balance for fast reads. Every stock change
should also create an `inventory-movement` record; purchase receipts increase
stock, sales decrease stock, and transfers create an outbound and inbound
movement for their respective warehouses.

The source product page describes capabilities such as cloud invoicing/receipt
issuance, electronic bookkeeping, accountant/bank connectivity, and direct
AADE/myDATA integration. These examples represent the data backbone for those
capabilities; they do not claim to implement the external regulatory services.
