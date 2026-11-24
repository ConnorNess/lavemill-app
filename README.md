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
To-dos and shopping still reset on refresh.

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

Reset the admin password from the host.

```sh
docker compose run --rm --no-deps app cargo run --locked --no-default-features --features server -- admin reset-password
```

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
