FROM ekidd/rust-musl-builder as build
WORKDIR /usr/src/app

# Cache dependencies
# ADD --chown=rust:rust Cargo.toml Cargo.toml
# RUN mkdir src/
# RUN echo "fn main() {println!(\"if you see this, the build broke\")}" > src/main.rs
# RUN cargo build --release
# RUN rm -f target/release/deps/app

ADD --chown=rust:rust . .

RUN cargo build --release
RUN cargo install --path .

FROM alpine:latest
COPY --from=build /home/rust/.cargo/bin/app /usr/local/bin/app
CMD ["/usr/local/bin/app"]