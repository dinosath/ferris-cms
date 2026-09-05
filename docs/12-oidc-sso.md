# OIDC / SSO authentication (admin panel)

`ferriscms` supports signing administrators in through an external **OpenID
Connect** identity provider (Keycloak, Okta, Entra ID, Google, Auth0, ...) using
the **Authorization Code flow with PKCE**. This lets an operator log into the
admin panel with their corporate account instead of (or in addition to) a local
username/password.

The feature is **opt-in**: it is entirely disabled unless the server is started
with OIDC configuration present.

## Configuration (environment variables)

OIDC is configured through environment variables at server start. Because the
server already reads most runtime config from env vars (and the Helm chart maps
arbitrary env vars through `values.env`), no extra files are needed.

| Variable | Required | Description |
|----------|----------|-------------|
| `OIDC_ISSUER` | yes | Issuer URL of the IdP, e.g. `https://accounts.google.com` or `https://keycloak.example.com/realms/myrealm`. Used for OIDC Discovery. |
| `OIDC_CLIENT_ID` | yes | The client (application) id registered at the IdP. |
| `OIDC_CLIENT_SECRET` | yes | The client secret for confidential clients. |
| `OIDC_REDIRECT_URI` | optional | The callback URL registered with the IdP. Defaults to `http://localhost:1337/admin/oidc/callback`. Must point to this server's `/admin/oidc/callback`. |
| `OIDC_SCOPES` | optional | Space-separated scopes. `openid` is always implied. Default: `openid profile email`. |
| `OIDC_AUTO_PROVISION` | optional | `true`/`1` to auto-create a Super Admin for a first-time SSO user (matched by email). Default `false` (an SSO user must already have an admin account). |

> **Security note:** with `OIDC_AUTO_PROVISION=true`, *any* user authenticated by
> the configured IdP with an email that has no local admin yet is granted the
> **Super Admin** role. Only enable this when the IdP is trusted to gate admin
> access (e.g. an org-restricted realm/application).

OIDC is considered **enabled** when `OIDC_ISSUER`, `OIDC_CLIENT_ID` and
`OIDC_CLIENT_SECRET` are all set and non-empty.

## Flow

1. The login screen calls `GET /admin/oidc/status` to learn whether SSO is
   enabled (and the issuer). When it is, the user is offered a
   "Continue with SSO" action that opens `GET /admin/oidc/authorize`.
2. `GET /admin/oidc/authorize` performs OIDC Discovery against the issuer,
   generates a `state` + PKCE `code_verifier` + `nonce`, remembers them
   server-side, and **redirects** the browser to the IdP authorization endpoint.
3. The user authenticates at the IdP and is redirected back to
   `GET /admin/oidc/callback?code=...&state=...`.
4. `GET /admin/oidc/callback` exchanges the code at the IdP token endpoint,
   verifies the returned ID token (signature against the IdP's JWKS, issuer,
   audience, expiry, nonce, `at_hash`), and matches the identity to an admin by
   email:
   - an existing **active** admin with that email is signed in;
   - otherwise, if `OIDC_AUTO_PROVISION=true`, a Super Admin is created;
   - otherwise the request is rejected (`403`).
5. On success the callback **redirects the browser back to the SPA** with the
   session token in the URL fragment: `/#oidc_token=<jwt>`. The SPA reads it,
   stores the token in `localStorage`, clears the fragment and opens the admin
   panel. The token is never sent back to the server as part of a reload.

## Admin API surface

All three routes are unauthenticated (they are the SSO entry points).

| Method | Route | Behaviour |
|--------|-------|-----------|
| `GET` | `/admin/oidc/status` | `{"data":{"enabled":bool,"issuer":string\|null}}` |
| `GET` | `/admin/oidc/authorize` | `302` redirect to the IdP, or an error when disabled |
| `GET` | `/admin/oidc/callback` | `code`+`state` exchange → `302` to `/#oidc_token=<jwt>` (the SPA logs the user in), or an error |

### Example (status)

```
curl http://localhost:1337/admin/oidc/status
# {"data":{"enabled":true,"issuer":"https://idp.example.com/realms/app"}}
```

The `authorize`/`callback` pair is a browser (Authorization Code + PKCE) flow:
`/admin/oidc/authorize` 302s to the IdP, and after the user signs in the IdP
redirects back to `/admin/oidc/callback`, which 302s the browser to the SPA at
`/#oidc_token=<jwt>`. The admin UI reads that token and starts a session.

## Mapping rules

The SSO identity is mapped to a local admin **by email** (from the ID token's
`email` claim, lowercased). Provisioned SSO accounts have a random, unusable
local password, so password login stays impossible for them; SSO is the only way
in. A blocked or deactivated admin cannot sign in via SSO.

## Multi-replica note

The in-flight PKCE `state` is held in server memory. For correct behaviour with
`replicaCount > 1`, SSO callbacks should reach the same instance that issued the
authorization (typical with a single service/ingress) — or the `state` store can
later be moved to the database.
