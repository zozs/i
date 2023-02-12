# building stage
FROM rust:1.67 as builder

RUN apt update && apt-get install -y musl-tools
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /usr/src/myapp
COPY . .
RUN cargo install --target=x86_64-unknown-linux-musl --path .

# running stage
FROM gcr.io/distroless/static-debian11
ARG APP=/usr/src/app

COPY --from=builder --chown=nonroot:nonroot /usr/local/cargo/bin/i ${APP}/i

USER nonroot:nonroot
WORKDIR ${APP}

CMD ["/usr/src/app/i"]
