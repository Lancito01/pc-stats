# PC Stats

A tiny Windows tray app that shares a few live stats from your PC with your website: whether you're at it, how hard it's working, and how much you've clicked and typed today. When the app isn't running, your site shows nothing about the PC at all.

You can see it live in the "right now" section of [lancito01.dev](https://lancito01.dev).

It's written in Rust against the Windows APIs directly: one ~270 KB exe, about 7 MB of RAM, and next to no CPU. This repo has the app, the [server side](server/) for a Vercel site, and the [protocol](PROTOCOL.md) for writing your own.

## What gets sent

About once a minute, only what's switched on in `pc-stats.ini`:

| Setting | Sent | Shown on the site as |
|---|---|---|
| `Uptime` | seconds since boot, seconds since the last input | "on for 4d 10h", active or away |
| `Load` | CPU and GPU % (averaged over the minute), RAM used/total | three small bars |
| `Input` | clicks, key presses and scrolls today | totals |
| `MouseDistance` | how far the mouse moved today | "1.3 km of mouse" |
| `Apps` | the focused app's name and description, **only if it's listed under `[Apps]`** | "in VS Code: code editor" |

**Show what gets sent** in the tray menu opens the last update exactly as it was sent.

### What it never reads or sends

- **Which keys you press.** Key presses are counted through Raw Input. The key code is only used to tell a new press from a held key repeating, then thrown away.
- **Window titles, URLs, file names or anything on screen.** For apps it only looks at the focused exe's file name, and only reports it if it's on your list.
- **Your IP, PC name, username or location.** The server only stores the fields above.

## How it's protected

- **Only your PC can post stats.** Every update is signed with a P-256 key that Windows creates on first run and marks *non-exportable*. It never exists as a file, can't be copied off the PC, and belongs to your Windows account. Your site only knows the public half, so someone who reads this code, the exe, or even your hosting settings still can't fake an update.
- **Updates can't be replayed or redirected.** Each signature covers the update's timestamp and your site's hostname. The server rejects anything more than 2 minutes off or older than the last accepted update, and an update signed for one site is useless on another.
- **The server accepts nothing unexpected.** It checks the signature before reading the update, then rejects unknown fields, out-of-range numbers, labels with control characters, and bodies over 2 KB.
- **Nothing to connect to.** The app only makes outgoing HTTPS requests through Windows' own WinHTTP. It doesn't listen on any port, take commands, or download or update itself.
- **It goes dark when you want.** *Pause sharing* or quitting stops all updates, and within about 3 minutes your site stops showing the PC. When the PC is off, the site doesn't say when it was last seen.

Being open source doesn't weaken any of this: none of the protection depends on the code being secret.

### What it can't protect against

- **Anyone can poll your site.** Someone who checks your site's public stats every minute can build their own record of when your PC is on or idle. That's what live stats means; switch off `Uptime` (or pause) if that matters.
- **Malware running as you on the PC.** It could use the key to sign fake updates, just as it could do anything else you can. If that happens, fake stats are the least of the problems.
- **Raw Input sees all physical input**, as other apps can. Like any input-counting app, some antivirus may flag it.

## Setup

You need Windows 10 or 11, and a site that can run the [server side](server/). It's ready-made for Vercel; [PROTOCOL.md](PROTOCOL.md) covers other hosts.

1. **Set up the server.** Follow [server/README.md](server/README.md): copy its `api/` folder into your project, add a Redis database, and deploy.
2. **Build the app:** `cargo build --release` (Rust with the MSVC toolchain). Move `target\release\pc-stats.exe` into its own folder; its settings, log and today's counts are saved next to it.
3. **Run it.** On first run it opens `pc-stats.ini`. Set `Endpoint` to your site's ingest address, e.g. `https://example.com/api/pc-stats/ingest`, and save.
4. **Connect the key.** Right-click the tray dot (green while sharing), pick **Copy public key**, and set it as `STATS_PUBLIC_KEY` on your site. On Vercel that's an environment variable, followed by a redeploy.
5. **Check the tray tooltip.** It should say *Last update sent at …*. **Start with Windows** is in the tray menu.

For a "now playing" from Spotify alongside the stats, see the Spotify section in [server/README.md](server/README.md). That part runs on the server, so it works even when the PC is off.

### If updates don't go through

The tray tooltip and `pc-stats.log` say why:

- **"Not sharing yet"**: `Endpoint` is empty or not an `https://` address.
- **"rejected this PC's key"**: `STATS_PUBLIC_KEY` doesn't match this PC's key. Copy it again.
- **"isn't set up for stats yet"**: the server is missing Redis or `STATS_PUBLIC_KEY`.
- **"couldn't reach the site"**: no connection, or something in front of the site blocking the request. Cloudflare's free *Bot Fight Mode*, for example, can't be told to skip one path. If that happens, point `Endpoint` at the host's own address (on Vercel, the project's production `.vercel.app` domain) to go around it.

## Settings

`pc-stats.ini` is created next to the exe on first run and re-read before every update, so edits apply within a minute. Each line is commented. The `[Apps]` section is `exe name = name to show | what it is`:

```ini
[Apps]
code.exe=VS Code | code editor
obs64.exe=OBS | recording or streaming
```

`MouseDpi` should match your mouse's DPI so the distance comes out right.

## Removing it

Quit from the tray (untick **Start with Windows** first) and delete the folder. To delete the signing key too:

```
certutil -user -csp "Microsoft Software Key Storage Provider" -delkey pc-stats
```

## Testing

- `pc-stats.exe --self-test [endpoint]` samples the PC for two seconds and prints an update signed for that endpoint's host with a throwaway key. It doesn't send anything or touch your real key.
- `pc-stats.exe --public-key` prints your real public key.
- `cargo test` runs the unit tests.
