# Protocol

How PC Stats sends updates, for anyone writing a server for it on another platform. [server/](server/) is the reference implementation.

## Request

```
POST /api/pc-stats/ingest HTTP/1.1
Host: example.com
Content-Type: text/plain; charset=utf-8
X-Stats-Timestamp: 1793245680
X-Stats-Signature: G54HsQiRRlrE33aRXiW4nfVj9pu9HiVAXmn9NCtZTDiLpICvdKvhPf4ayqCyjL2+eni8PXF+dlqUhW8X3cLFmA==

{"v":1,"ts":1793245680,"uptime":384096,"idle":12,"cpu":29,...}
```

- **Transport:** HTTPS only; the app refuses plain `http://` endpoints.
- **Body:** sent as `text/plain` so frameworks hand the server the exact bytes that were signed instead of re-encoding parsed JSON. It's UTF-8 JSON, at most 2 KB.
- **`X-Stats-Timestamp`:** Unix time in seconds when the update was made. It equals `ts` in the body.
- **`X-Stats-Signature`:** base64 of a 64-byte ECDSA P-256 / SHA-256 signature in IEEE P1363 form (`r || s`, not DER).

## Signature

The signature covers this message, encoded as UTF-8, with `\n` line breaks:

```
pc-stats/v1
<host>
<timestamp>
<body>
```

- `<host>` is the endpoint's hostname, lowercase, without a port: `example.com`. The server uses the `Host` header the request arrived with. Because the host decides which site receives the request, an update signed for one site can't be replayed against another that trusts the same key.
- `<timestamp>` is the header's value.
- `<body>` is the request body, byte for byte.

The public key is a JWK, which the app shows under **Copy public key**:

```json
{"kty":"EC","crv":"P-256","x":"…","y":"…"}
```

The private key is a non-exportable CNG key named `pc-stats` in the user's Microsoft Software Key Storage Provider.

## Body

Only `v`, `ts` and `interval` are always present; the rest depend on what the user shares.

| Field | Type | Range | Meaning |
|---|---|---|---|
| `v` | integer | `1` | Protocol version |
| `ts` | integer | Unix seconds | Same as `X-Stats-Timestamp` |
| `interval` | integer | 5 – 600 | Seconds until the next update (60 or more while away), so a site can animate between readings |
| `uptime` | integer | 0 – 10 years | Seconds since boot |
| `idle` | integer | 0 – 10 years | Seconds since the last keyboard or mouse input |
| `cpu` | integer | 0 – 100 | CPU %, averaged since the last update |
| `gpu` | integer | 0 – 100 | GPU (3D engine) %, averaged since the last update |
| `ramUsed`, `ramTotal` | number | 0 – 4096 | GB |
| `day` | string | `YYYY-MM-DD` | The PC's local date the counts below belong to |
| `clicks`, `keys`, `scrolls` | integer | 0 – 10⁹ | Totals for `day` |
| `mouse` | number | 0 – 10⁷ | Metres the mouse moved on `day` |
| `app` | string | 1 – 40 characters | Name of the focused app, only if the user listed it |
| `appInfo` | string | 1 – 80 characters | The user's description of that app |

Strings may contain letters, numbers, punctuation, symbols and spaces, but no control characters or line breaks.

## What the server checks, in order

| Check | Response |
|---|---|
| Not `POST` | `405` |
| No public key or storage configured | `503` |
| Body missing, not text, or over 2 KB | `400` |
| Timestamp more than 120 s from the server's clock, or the signature doesn't verify | `401` |
| Body isn't valid JSON, has an unknown field, a value out of range, or a `ts` that differs from the header | `400` |
| `ts` isn't at least 4 s after the last accepted update (replays and floods) | `409` |
| Accepted and stored | `204` |

The signature is checked before the body is parsed. The `ts` comparison and the write happen atomically, so two copies of the same update can't both be accepted.

The app treats `409` as harmless, and shows the others in its tray tooltip and log. It sends every 5 seconds by default (5 to 600) while the user is active, and once a minute after 5 minutes without input, so a server can consider the PC offline after about 2½ minutes of silence.
