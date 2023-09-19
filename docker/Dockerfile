# docker build --build-arg GITHUB_AUTH_TOKEN=... -t .../bootstrap-rr:latest .

FROM rust:1.71-buster as builder

RUN rustup update nightly-2023-06-01 && rustup default nightly-2023-06-01

RUN mkdir -p -m 0700 ~/.ssh && ssh-keyscan github.com >> ~/.ssh/known_hosts

ENV CARGO_NET_GIT_FETCH_WITH_CLI=true

ARG GITHUB_AUTH_TOKEN
RUN git config --global url."https://${GITHUB_AUTH_TOKEN}:@github.com/".insteadOf "https://github.com/" && \
    cargo install --git https://github.com/vlad9486/openmina-poc.git --branch docker openmina-bootstrap-sandbox openmina-gossipsub-sandbox --locked && \
    rm ~/.gitconfig

FROM debian:buster

RUN apt-get update && apt-get install -y libssl-dev

COPY --from=builder /usr/local/cargo/bin/openmina-bootstrap-sandbox \
    /usr/local/bin/openmina-bootstrap-sandbox
COPY --from=builder /usr/local/cargo/bin/openmina-gossipsub-sandbox \
    /usr/local/bin/openmina-gossipsub-sandbox

ENV MINA_RECORD_PATH=/tmp/mina-record
COPY ./mina-record.tar.gz /tmp/mina-record.tar.gz
RUN cd /tmp && tar xf mina-record.tar.gz && rm mina-record.tar.gz
