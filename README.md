# Lavemill

A little family app built with Dioxus. Rust and Cargo run in Docker, so there's
nothing extra to install locally.

Start Docker Desktop with Linux containers enabled. Run everything below from
`lavemill-app`.

## First run

```sh
docker compose build
docker compose up
```

Open http://localhost:8080 once the app finishes compiling. The first build takes
a while. Source edits are picked up while it's running.

`Cargo.lock` is included. If it ever needs generating from scratch:

```sh
docker compose run --rm --no-deps app cargo generate-lockfile
```

## Day to day

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

Ctrl+C also stops it when running in the foreground. Caches and data survive a
normal shutdown.

## Cargo commands

```sh
# Check formatting
docker compose run --rm --no-deps app cargo fmt --check

# Check the server
docker compose run --rm --no-deps app cargo check --locked --no-default-features --features server

# Check the web client
docker compose run --rm --no-deps app cargo check --locked --no-default-features --features web --target wasm32-unknown-unknown
```
