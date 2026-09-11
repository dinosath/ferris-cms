#!/usr/bin/env python3
"""Live acceptance test for computed fields against a running ferriscms server.

Exercises the real end-user path (HTTP -> Content-Type Builder -> DDL -> REST)
on PostgreSQL:

    DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/ferriscms \
    JWT_SECRET=secret BIND_ADDR=127.0.0.1:1337 \
    ./target/debug/ferriscms-server &

    python3 scripts/acceptance_computed_postgres.py

Covers ERP chained computed columns (subtotal / discount_amount / total_amount),
recompute on update, filtering and sorting by computed fields, rejection of
writes to computed fields, and CRM full_name concatenation + success_rate.
Exits non-zero on any assertion failure.
"""

import json, time, urllib.request, urllib.error

BASE = "http://127.0.0.1:1337"
EMAIL = "live-acceptance@test.dev"
PW = "LivePass123!"

def req(method, path, body=None, token=None):
    data = json.dumps(body).encode() if body is not None else None
    r = urllib.request.Request(BASE + path, data=data, method=method)
    if body is not None: r.add_header("Content-Type", "application/json")
    if token: r.add_header("Authorization", "Bearer " + token)
    try:
        with urllib.request.urlopen(r) as resp:
            return resp.status, json.loads(resp.read().decode() or "null")
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read().decode() or "null")

# 1. first admin (register, or login if it already exists)
st, body = req("POST", "/admin/register-admin", {"email": EMAIL, "password": PW, "firstname": "Live", "lastname": "Acceptance"})
if st == 409:
    st, body = req("POST", "/admin/login", {"email": EMAIL, "password": PW})
assert st == 200, (st, body)
token = body["data"]["token"]
print("auth OK (token issued)")

suf = int(time.time())
name = "so%d" % suf
uid = "api::%s.%s" % (name, name)

erp = {"uid": uid, "kind": "collectionType",
  "info": {"singularName": name, "pluralName": name + "s", "displayName": "Sales Order"},
  "attributes": {
    "quantity": {"type": "integer"},
    "unit_price": {"type": "decimal"},
    "discount": {"type": "integer"},
    "subtotal": {"type": "decimal", "computed": True, "expression": "quantity * unit_price", "stored": True, "dependencies": ["quantity","unit_price"]},
    "discount_amount": {"type": "decimal", "computed": True, "expression": "subtotal * discount / 100", "stored": True, "dependencies": ["subtotal","discount"]},
    "total_amount": {"type": "decimal", "computed": True, "expression": "subtotal - discount_amount", "stored": True, "dependencies": ["subtotal","discount_amount"]},
  }}

st, body = req("POST", "/content-type-builder/schema", {"schemas": [erp]}, token)
assert st == 200 and not body.get("error") or body.get("error") is None, (st, body)
print("ERP schema applied (chained computed columns on PostgreSQL)")

def num(x): return float(x)
st, body = req("POST", "/admin/content-manager/collection-types/%s" % uid, {"data": {"quantity": 10, "unit_price": 100, "discount": 10}}, token)
assert st == 200, (st, body)
d = body["data"]
assert num(d["subtotal"]) == 1000, d
assert num(d["discount_amount"]) == 100, d
assert num(d["total_amount"]) == 900, d
doc = d["documentId"]
print("ERP create OK: subtotal=1000 discount=100 total=900 (DB-computed)")

st, body = req("PUT", "/admin/content-manager/collection-types/%s/%s" % (uid, doc), {"data": {"quantity": 20}}, token)
assert st == 200 and num(body["data"]["total_amount"]) == 1800, body
print("ERP update OK: recomputed total=1800")

st, body = req("GET", "/admin/content-manager/collection-types/%s?filters[total_amount][$gte]=1000&pagination[pageSize]=10" % uid, token=token)
assert st == 200 and len(body["data"]) == 1, body
print("ERP filter by computed total_amount OK (%d row)" % len(body["data"]))

st, body = req("GET", "/admin/content-manager/collection-types/%s?sort[0]=total_amount:desc&pagination[pageSize]=10" % uid, token=token)
assert st == 200 and num(body["data"][0]["total_amount"]) == 1800, body
print("ERP sort by computed total_amount OK")

st, body = req("POST", "/admin/content-manager/collection-types/%s" % uid, {"data": {"quantity": 1, "unit_price": 1, "total_amount": 999999}}, token)
assert st == 400, (st, body)
print("ERP write to computed field rejected (400)")

# CRM: concat + integer division
cname = "cu%d" % suf
cuid = "api::%s.%s" % (cname, cname)
crm = {"uid": cuid, "kind": "collectionType",
  "info": {"singularName": cname, "pluralName": cname + "s", "displayName": "Customer"},
  "attributes": {
    "first_name": {"type": "string"}, "last_name": {"type": "string"},
    "deals_won": {"type": "integer"}, "deals_lost": {"type": "integer"},
    "full_name": {"type": "text", "computed": True, "expression": "first_name || ' ' || last_name", "stored": True, "dependencies": ["first_name","last_name"]},
    "success_rate": {"type": "integer", "computed": True, "expression": "(deals_won * 100) / (deals_won + deals_lost)", "stored": True, "dependencies": ["deals_won","deals_lost"]},
  }}
st, body = req("POST", "/content-type-builder/schema", {"schemas": [crm]}, token)
assert st == 200 and body.get("error") is None, (st, body)
print("CRM schema applied (concat + division on PostgreSQL)")

st, body = req("POST", "/admin/content-manager/collection-types/%s" % cuid, {"data": {"first_name": "John", "last_name": "Smith", "deals_won": 8, "deals_lost": 2}}, token)
assert st == 200, (st, body)
d = body["data"]
assert d["full_name"] == "John Smith", d
assert num(d["success_rate"]) == 80, d
print("CRM create OK: full_name='John Smith' success_rate=80 (DB-computed)")

print("\nLIVE ACCEPTANCE PASSED (real server, real PostgreSQL, HTTP)")
