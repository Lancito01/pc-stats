# Server side

Two Vercel functions that receive updates from PC Stats and serve them to your site, plus an optional Spotify "now playing" source. They're the same files [lancito01.dev](https://github.com/Lancito01/lancito01.dev) runs.

| Route | What it does |
|---|---|
| `POST /api/pc-stats/ingest` | Checks the signature and the payload, then stores the latest update in Redis |
| `GET /api/pc-stats` | The public view: the PC's stats while it's online, and what's playing on Spotify |

## Adding it to a Vercel project

1. Copy `api/` into your project's root, next to any `api/` you already have. Files starting with `_` aren't turned into routes, so `api/_lib/` stays private.
2. The code is ES modules: your `package.json` needs `"type": "module"` (or rename the files to `.mjs`). It runs on Node 18 or newer and has no dependencies.
3. In Vercel, add **Upstash Redis** from the Marketplace (Storage tab). It sets `KV_REST_API_URL` and `KV_REST_API_TOKEN`. A direct Upstash database works too, with `UPSTASH_REDIS_REST_URL` and `UPSTASH_REDIS_REST_TOKEN`.
4. Add `STATS_PUBLIC_KEY`: the JSON key from the app's tray menu (**Copy public key**).
5. If your site is a single-page app with a catch-all rewrite, keep `/api/` out of it, e.g. in `vercel.json`:

   ```json
   { "rewrites": [{ "source": "/((?!api/).*)", "destination": "/index.html" }] }
   ```

6. Deploy, then set the app's `Endpoint` to `https://your-site/api/pc-stats/ingest`.

This is written for Vercel's plain `api/` functions (Vite, static sites and the like). On Next.js, move the two handlers into route handlers; the logic in `api/_lib/` carries over as is. On another host, [PROTOCOL.md](../PROTOCOL.md) has everything a server needs to check.

### Choosing what's public

`PUBLIC_FIELDS` in `api/_lib/pcStats.js` lists what `GET /api/pc-stats` serves. A field that isn't listed is stored but never shown, so you can trim what the site reveals without touching the app.

### Spotify (optional)

1. Create an app at the [Spotify developer dashboard](https://developer.spotify.com/dashboard) with the redirect URI `http://127.0.0.1:8888/callback`.
2. Run `node scripts/spotify-auth.js <client id> <client secret>` and log in. It prints a refresh token that can only read what's currently playing.
3. Set `SPOTIFY_CLIENT_ID`, `SPOTIFY_CLIENT_SECRET` and `SPOTIFY_REFRESH_TOKEN` in Vercel.

## What `GET /api/pc-stats` returns

```json
{
  "at": 1793245680000,
  "pc": {
    "online": true,
    "status": "active",
    "uptime": 384096,
    "cpu": 29, "gpu": 41, "ramUsed": 20.8, "ramTotal": 31.9,
    "clicks": 3412, "keys": 18902, "scrolls": 1207, "mouse": 1284.5,
    "app": "VS Code", "appInfo": "code editor"
  },
  "music": {
    "title": "…", "artist": "…", "image": "https://i.scdn.co/…", "url": "https://open.spotify.com/…",
    "progressMs": 61000, "durationMs": 210000
  }
}
```

- `pc` is `{ "online": false }` when the app hasn't checked in for 2½ minutes. Nothing else about the PC is served then, not even when it was last seen. It's left out entirely when the stats aren't set up, and `null` if Redis couldn't be reached.
- `status` is `"away"` after 5 minutes without keyboard or mouse input.
- `music` is `null` when nothing is playing (paused counts as nothing), and left out when Spotify isn't set up.
- `at` is when the answer was made. Responses are cached at the edge for 15 seconds, so add the time since `at` to `progressMs` to keep a progress bar moving.

lancito01.dev's widget is a working example: [`LiveStrip.jsx`](https://github.com/Lancito01/lancito01.dev/blob/main/src/Components/LiveStrip/LiveStrip.jsx) and [`useLiveStats.js`](https://github.com/Lancito01/lancito01.dev/blob/main/src/lib/useLiveStats.js).

## Testing a deployment

`pc-stats.exe --self-test https://your-site/api/pc-stats/ingest` prints an update signed for that site with a throwaway key, without sending anything. Put its `publicKey` in `STATS_PUBLIC_KEY` on a preview deployment and post the update with the headers from PROTOCOL.md to check the whole path.
