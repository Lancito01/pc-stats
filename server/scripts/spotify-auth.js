// One-time helper that gets the Spotify refresh token the site uses to show what's playing.
//
// 1. Create an app at https://developer.spotify.com/dashboard with the redirect URI
//    http://127.0.0.1:8888/callback (and Web API ticked).
// 2. node scripts/spotify-auth.js <client id> <client secret> [port]
//    The port defaults to 8888. If something else uses it, pick another and add the
//    matching redirect URI (http://127.0.0.1:<port>/callback) to the Spotify app.
// 3. Log in on the page that opens. The token is printed here; put it in SPOTIFY_REFRESH_TOKEN
//    on Vercel, next to SPOTIFY_CLIENT_ID and SPOTIFY_CLIENT_SECRET.
//
// The token can only read what's currently playing (the user-read-currently-playing scope).

import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { createServer } from "node:http";

const [clientId, clientSecret, portArg = "8888"] = process.argv.slice(2);
const port = Number(portArg);
if (!clientId || !clientSecret || !Number.isInteger(port) || port < 1 || port > 65535) {
    console.error("usage: node scripts/spotify-auth.js <client id> <client secret> [port]");
    process.exit(1);
}

const redirectUri = `http://127.0.0.1:${port}/callback`;
const state = randomBytes(16).toString("hex");
const authorizeUrl = `https://accounts.spotify.com/authorize?${new URLSearchParams({
    client_id: clientId,
    response_type: "code",
    redirect_uri: redirectUri,
    scope: "user-read-currently-playing",
    state,
})}`;

// What Spotify's `error` values usually mean for this script
const hints = {
    access_denied: "The login was cancelled. Run the script again and click Agree.",
    invalid_client: "Spotify doesn't recognise the client ID. Copy it again from the app's settings.",
    invalid_request: "Spotify rejected the request. Check that the redirect URI in the app's settings matches exactly.",
};

const reply = (res, status, text) => res.writeHead(status, { "Content-Type": "text/plain" }).end(text);

const server = createServer(async (req, res) => {
    const url = new URL(req.url, redirectUri);
    if (url.pathname !== "/callback") return res.writeHead(404).end();

    const error = url.searchParams.get("error");
    if (error) {
        const hint = hints[error] ?? "Run the script again.";
        console.error(`Spotify sent back an error instead of a login: ${error}\n${hint}`);
        reply(res, 400, `Spotify said "${error}". ${hint}`);
        return server.close();
    }

    // The state check makes sure this callback answers the login this script started.
    // A tab left over from an earlier run doesn't end this one: the right callback may still come.
    if (url.searchParams.get("state") !== state) {
        console.error("Ignored a login from an earlier run of the script. Use the page this run opened.");
        return reply(res, 400, "This page is from an earlier run of the script. Close it and use the one the latest run opened.");
    }
    if (!url.searchParams.get("code")) {
        console.error(`Spotify came back without a login code: ${url.search}`);
        reply(res, 400, "Spotify came back without a login code. Run the script again.");
        return server.close();
    }

    const response = await fetch("https://accounts.spotify.com/api/token", {
        method: "POST",
        headers: {
            Authorization: `Basic ${Buffer.from(`${clientId}:${clientSecret}`).toString("base64")}`,
            "Content-Type": "application/x-www-form-urlencoded",
        },
        body: new URLSearchParams({
            grant_type: "authorization_code",
            code: url.searchParams.get("code"),
            redirect_uri: redirectUri,
        }),
    });
    const data = await response.json();

    if (!response.ok || !data.refresh_token) {
        reply(res, 500, "Spotify didn't hand over a token. The details are in the terminal.");
        console.error("Spotify answered:", data);
        if (data.error === "invalid_client") console.error("The client secret is probably wrong. Copy it again.");
    } else {
        reply(res, 200, "Done. You can close this tab.");
        console.log(`\nSPOTIFY_REFRESH_TOKEN=${data.refresh_token}\n`);
    }
    server.close();
});

server.on("error", (error) => {
    if (error.code !== "EADDRINUSE") throw error;
    const next = port + 1;
    console.error(`Port ${port} is already used by another app. To use ${next} instead:`);
    console.error(`  1. Add http://127.0.0.1:${next}/callback as a redirect URI in your Spotify app's settings.`);
    console.error(`  2. Run: node scripts/spotify-auth.js <client id> <client secret> ${next}`);
    process.exit(1);
});

server.listen(port, "127.0.0.1", () => {
    console.log(`Opening Spotify's login page. If it doesn't open, visit:\n${authorizeUrl}`);
    // `start` is Windows' way to open a URL in the default browser
    spawn("cmd", ["/c", "start", "", authorizeUrl.replaceAll("&", "^&")], { stdio: "ignore", detached: true });
});
