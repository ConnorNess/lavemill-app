# Lavemill

A little family app built with Dioxus. Rust and Cargo run in Docker, so there's
nothing extra to install locally.

## SQLite foundation

### Database viewer

Start the app and the SQLite viewer together:

```sh
docker compose up
```

Open http://localhost:8082 to browse tables and run SELECT queries. The viewer
starts once the app compiles, initializes the database, and serves its home page.
If the initial build fails, the app exits and Compose leaves the viewer stopped.
The viewer uses a pinned upstream sqlite-web image, opens the database read-only,
and is available only on this machine.
Set `LAVEMILL_DB_VIEWER_PORT` in your shell to override port 8082.

```sh
docker compose logs sqlite-web
docker compose stop sqlite-web
```

Stopping the viewer leaves the app running. Use `docker compose up app` to start
only the app. The app pages still use mock data, so the viewer shows only the
empty `schema_migrations` table. The shared volume is writable so SQLite can create
WAL coordination files, but the viewer's database connection rejects edits. No
authentication is configured; keep this development tool bound to localhost.

### Storage and migrations

The server initializes SQLite before serving requests. Compose sets
`LAVEMILL_DATABASE_PATH=/data/lavemill.sqlite3` on the `household-data` volume;
outside Compose the default is `data/lavemill.sqlite3`. Missing parent directories
are created. Invalid paths or failed migrations stop startup with a path and error.
The current pages still use mock data and reset on refresh.

Connections enable foreign keys, WAL mode and a five-second busy timeout.
Only the server builds SQLite. The development image includes the `sqlite3` CLI:

```sh
docker compose exec app sqlite3 /data/lavemill.sqlite3 .schema
docker compose exec app cargo test --locked --no-default-features --features server
```

The initial schema contains only `schema_migrations`. To add a feature schema,
add a numbered SQL file under `src/migrations/` and append its positive version
and `include_str!` SQL to `MIGRATIONS` in `src/database.rs`. Keep versions strictly
increasing and never edit an applied migration. SQL must not contain transaction
control: the runner wraps all pending migrations and ledger entries in one
immediate transaction. Unknown or non-prefix database histories fail startup.

For an isolated persistence check, use a separate Compose project and unused ports
(e.g. set `LAVEMILL_PORT=8081` and `LAVEMILL_DB_VIEWER_PORT=8083` in your shell).
Build/start it with `docker compose -p lavemill-sqlite-check up --build -d`. Wait
for compilation and confirm its home page loads before running:

```sh
docker compose -p lavemill-sqlite-check exec app sqlite3 /data/lavemill.sqlite3 "CREATE TABLE persistence_check(value TEXT); INSERT INTO persistence_check VALUES ('survived');"
docker compose -p lavemill-sqlite-check up -d --force-recreate
docker compose -p lavemill-sqlite-check exec app sqlite3 /data/lavemill.sqlite3 "SELECT value FROM persistence_check;"
docker compose -p lavemill-sqlite-check exec app sqlite3 /data/lavemill.sqlite3 "DROP TABLE persistence_check;"
docker compose -p lavemill-sqlite-check down
```

The query must return `survived`. `down` retains data; `down -v` deletes that
project's volumes. Database files are ignored by Git. Backup/restore procedures
and production deployment will be added in a separate task.

Start Docker Desktop with Linux containers enabled. Run everything below from
`lavemill-app`.

## First run

```sh
docker compose build
docker compose up
```

Open the app at http://localhost:8080 and the SQLite viewer at http://localhost:8082
once the app finishes compiling and initializes the database. The first build
takes a while. Source edits are picked up while the app is running.

`Cargo.lock` is included. If it ever needs generating from scratch:

```sh
docker compose run --rm --no-deps app cargo generate-lockfile
```

## Docker commands

```sh
# Start
docker compose up

# Or run in the background
docker compose up -d

# Logs
docker compose logs -f app

# Stop
docker compose down

# Rebuild after changing the Dockerfile
docker compose up --build
```

## Cargo commands

```sh
# Check formatting
docker compose run --rm --no-deps app cargo fmt --check

# Check the server
docker compose run --rm --no-deps app cargo check --locked --no-default-features --features server

# Check the web client
docker compose run --rm --no-deps app cargo check --locked --no-default-features --features web --target wasm32-unknown-unknown
```
