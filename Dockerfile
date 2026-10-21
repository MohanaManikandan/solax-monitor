# The screens. Node builds ui/dist, which the server embeds at compile time.
FROM node:26-alpine AS ui
WORKDIR /build/ui
COPY ui/package.json ui/package-lock.json ./
RUN npm ci
COPY crates/solax-api/bindings /build/crates/solax-api/bindings
COPY ui ./
RUN npm run build

# Build stage. musl-dev is what the bundled SQLite needs to compile.
FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /build
# The dependencies first, against stand-in sources, as a layer of their own:
# a change to the code then rebuilds only this workspace's two crates, not
# everything they depend on.
COPY Cargo.toml Cargo.lock ./
COPY crates/solax-api/Cargo.toml crates/solax-api/Cargo.toml
COPY server/Cargo.toml server/Cargo.toml
RUN mkdir -p crates/solax-api/src server/src \
 && echo "" > crates/solax-api/src/lib.rs \
 && echo "fn main() {}" > server/src/main.rs \
 && cargo build --release -p solax-server \
 && rm -rf crates/solax-api/src server/src
COPY crates crates
COPY server server
COPY --from=ui /build/ui/dist ui/dist
# The real sources keep their own, older times, so cargo would take the
# stand-ins built a moment ago as current and ship them. Touched, they build.
RUN touch crates/solax-api/src/lib.rs server/src/main.rs \
 && cargo build --release -p solax-server

# Runtime stage. No X server, no Python, no fonts: the UI is a web page now.
FROM alpine:3.22
RUN adduser -D -u 10001 solax && mkdir -p /app/solax-data && chown solax /app/solax-data
WORKDIR /app
COPY --from=build /build/target/release/solax-server /usr/local/bin/solax-server

# An unprivileged default for a bare docker run. compose overrides this with
# the host user, because config.toml is mode 0600 and only its owner can read
# it. Running bare, pass -u "$(id -u):$(id -g)" for the same reason.
USER solax
EXPOSE 8080
CMD ["solax-server"]
