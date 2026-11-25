# Lavemill

Dioxus family app. Rust and Cargo run in Docker.

## Local dev

Start Docker Desktop in Linux container mode. Run from `lavemill-app`.

```sh
docker compose up --build
docker compose logs -f app
docker compose down
```

Use http://localhost:8080/login or http://127.0.0.1:8080/login.
Set `LAVEMILL_PORT` to change the port.
The first build takes a while. Source edits reload automatically.
To-dos use the household time zone, set with `LAVEMILL_TIME_ZONE` (default `Europe/London`).
Shopping is shared across signed-in household members. Refresh to see changes
from other devices. The app deletes items completed at least seven days ago the
next time someone loads Home or reads or updates the shopping list.

`docker compose up` starts the app and the SQLite viewer. Use
`docker compose up app` to start just the app.

## Admin

Create once on the Compose database.

```sh
docker compose run --rm --no-deps app cargo run --locked --no-default-features --features server -- admin init connor Connor Ness
```

Lavemill args go after `--`. Use `admin init USERNAME FIRST_NAME LAST_NAME`.
Quote names with spaces. Enter the password twice at the hidden prompts.

`--rm` cleans up the one-off container. `--no-deps` skips other services.
`--locked` keeps `Cargo.lock` fixed. The feature flags select the server build.

Use **Users** to add accounts, edit details, or reset passwords. A password
reset signs the account out on all devices.

### Plex profiles

Enable the API in Tautulli under **Settings → Web Interface**. Set
`LAVEMILL_TAUTULLI_URL=http://192.168.178.38:8181` and
`LAVEMILL_TAUTULLI_API_KEY` in your local `.env`, then recreate the app
container. The URL must be reachable from that container. Use the base URL
without `/home`. Header authentication requires Tautulli v2.18.0 or later.
In **Users**, choose each person's Plex Home profile and save it. If a profile
is missing, refresh the user list in Tautulli and reload the Users page.

Reset the admin password from the host.

```sh
docker compose run --rm --no-deps app cargo run --locked --no-default-features --features server -- admin reset-password
```

### Steam playing status

Create a [Steam Web API key](https://steamcommunity.com/dev/apikey), set
`LAVEMILL_STEAM_API_KEY` in the server environment or local Compose `.env`,
and recreate the app container. Leave the key blank to disable Steam activity.

When a household member requests sharing, enter their SteamID64 in **Users**
and select **Save Steam profile**. The member needs to set their Steam
**Game details** privacy to **Public**. Games marked private can hide playing
status. Lavemill stores the linked ID and caches current game titles in memory
for 30 seconds. Signed-in household members can see **Playing [game]** on
**Home** when Steam reports a title. To stop sharing, clear the ID and save.

## SQLite

Compose uses `/data/lavemill.sqlite3` on the `household-data` volume.
Outside Compose, `LAVEMILL_DATABASE_PATH` defaults to `data/lavemill.sqlite3`.
`docker compose down` keeps the data. `down -v` deletes the volume.

```sh
docker compose exec app sqlite3 /data/lavemill.sqlite3 .schema
docker compose stop sqlite-web
```

Viewer runs at http://127.0.0.1:8082 after the app is healthy. Override with
`LAVEMILL_DB_VIEWER_PORT`. It's read-only and unauthenticated. Use it with
dev data only.

Add numbered SQL files in `src/migrations/` and register them in `MIGRATIONS`
in `src/database.rs`. Use increasing versions. Leave applied migrations alone.
The runner handles transactions.

To check persistence, use a separate Compose project and port. Write a test
row, recreate the app with `up -d --force-recreate`, and query the row again.

## Checks

```sh
docker compose run --rm --no-deps app cargo fmt --check
docker compose run --rm --no-deps app cargo test --locked --no-default-features --features server
docker compose run --rm --no-deps app cargo check --locked --no-default-features --features web --target wasm32-unknown-unknown
```

Regenerate the lockfile if needed.

```sh
docker compose run --rm --no-deps app cargo generate-lockfile
```

## HTTPS

Set `LAVEMILL_ORIGIN` to the public origin, e.g. `https://lavemill.example.com`.
Point cloudflared at the release server. Keep the SQLite viewer stopped.

## Google Calendar

Enable the Google Calendar API in a Google Cloud project and create a Web
application OAuth client. Register the exact callback at the app origin, for
example `http://127.0.0.1:8080/calendar/google/callback` locally or
`https://lavemill.example.com/calendar/google/callback` in production. Open
Lavemill at that same origin when connecting a calendar.

Set `LAVEMILL_GOOGLE_CLIENT_ID`, `LAVEMILL_GOOGLE_CLIENT_SECRET`, and
`LAVEMILL_GOOGLE_TOKEN_KEY` in the server environment or the ignored local
`.env` file for Compose. Generate the token key once with Python:

```sh
python -c "import secrets; print(secrets.token_urlsafe(32))"
```

Keep the key across deployments and backups. Changing it means members must
reconnect. With all three settings unset, calendar linking is disabled.
For an external OAuth app in Testing, add each Google account as a test user.
Google refresh tokens for Calendar expire after seven days in Testing mode.

Open **Calendar ? Manage calendars** to connect Google and save a calendar selection.
Select **Share with household** to show its current week on Home. Events marked
private appear as Busy. Use **Stop sharing** to remove the calendar from Home
while keeping it connected.

## Outlook Calendar

Register a Microsoft Entra app for personal Microsoft accounts with a **Web**
redirect URI. For local testing on a separate Compose project, set
`LAVEMILL_PORT=8081` and `LAVEMILL_ORIGIN=http://localhost:8081`, and register
`http://localhost:8081/calendar/outlook/callback`. Open Lavemill using that
origin. If testing both providers, also register
`http://localhost:8081/calendar/google/callback` with Google. For production,
register the same Outlook callback path on the configured HTTPS origin.

Grant the delegated Microsoft Graph permission `Calendars.ReadBasic`. Lavemill
requests `offline_access` when a member connects. Set
`LAVEMILL_OUTLOOK_CLIENT_ID`, `LAVEMILL_OUTLOOK_CLIENT_SECRET` (the secret value),
and `LAVEMILL_OUTLOOK_TOKEN_KEY` in the server environment or ignored Compose
`.env` file. Generate a separate token key using the command above and retain
it across deployments and backups.

Under **Calendar ? Manage calendars**, connect Outlook, save a calendar selection, and choose
**Share with household**. Home combines the current week from shared Outlook and
Google calendars. Outlook events marked personal, private, or confidential
appear as Busy.

The **Calendar** page shows shared events in a weekly time grid or a month
grid. Use **Week** and **Month** to change views, and select a day in the
month grid to see all of its events. Home shows the current week's agenda.
