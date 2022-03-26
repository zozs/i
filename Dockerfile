# building stage
FROM rust:1.59 as builder

WORKDIR /usr/src/myapp
COPY . .
RUN cargo install --path .

# running stage
FROM debian:11-slim
ARG APP=/usr/src/app

RUN apt-get update \
    && apt-get install -y ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/*

ENV APP_USER=appuser
RUN groupadd $APP_USER \
    && useradd -g $APP_USER $APP_USER \
    && mkdir -p ${APP}

COPY --from=builder /usr/local/cargo/bin/i ${APP}/i

RUN chown -R $APP_USER:$APP_USER ${APP}

USER $APP_USER
WORKDIR ${APP}

CMD ["./i"]
