# Auth fixtures (spec 0002)

JSON bodies served by `wiremock` in the auth tests. Tokens, codes and account
data are fakes (`FAKE-ACCESS`, `FAKE-REFRESH`, `FAKE-DEVICE-CODE`, user `1`,
country `FI`, user code `ABCDE`).

**Recorded shape**: copied from the live-API probe in the spec's "Facts vs.
assumptions" (keys, types and nesting as Tidal sent them, values faked):

| Fixture | Live response it mirrors |
|---|---|
| `device_authorization.json` | `200` from `POST /device_authorization` |
| `token_pending.json` | `400` `authorization_pending` from `POST /token` |
| `token_granted.json` | `200` device-code grant from `POST /token` |
| `token_refreshed.json` | `200` refresh from `POST /token` (no `refresh_token`) |
| `refresh_invalid_grant.json` | `400` `invalid_grant` for a bad refresh token |
| `api_unauthorized.json` | `401` from an API call with a bad bearer |

**Derived** from a recorded shape by removing or changing one field, to
exercise an edge case: `device_authorization_no_complete.json` (no
`verificationUriComplete`), `token_granted_no_user.json`,
`token_granted_empty_country.json`, `token_granted_no_user_id.json`,
`token_refreshed_country_changed.json` (`SE`), `token_refreshed_no_user.json`,
`token_refreshed_rotated.json` (carries a new `refresh_token`; Tidal was not
seen rotating it).

**Assumed** (never seen live; RFC 8628 / RFC 6749 error codes written in the
same `{"error", "error_description", "status", "sub_status"}` shape as
`token_pending.json`): `token_slow_down.json`, `token_expired.json`,
`token_denied.json`, `refresh_invalid_client.json`,
`refresh_unauthorized_client.json` (AC19: Tidal rejecting the PKCE client).

**Synthetic**, for AC15 only (a server body that echoes the tokens):
`echoing_error.json`, `echoing_malformed_grant.json`.
