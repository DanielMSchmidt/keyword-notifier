FROM rust:latest as build
WORKDIR /usr/src/app

# Cache dependencies
COPY Cargo.toml Cargo.toml
RUN mkdir src/
RUN echo "fn main() {println!(\"if you see this, the build broke\")}" > src/main.rs
RUN cargo build --release
RUN rm -f target/release/deps/app*

COPY . .

RUN cargo build --release
RUN cargo install --path .

FROM alpine:latest
COPY --from=build /usr/local/cargo/bin/app /usr/local/bin/app
CMD ["app"]