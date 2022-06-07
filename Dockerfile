FROM rust:latest as build

RUN rustup target add x86_64-unknown-linux-musl
RUN apt update && apt install -y musl-tools musl-dev build-essential
RUN update-ca-certificates


WORKDIR /app
COPY . /app

RUN cargo build --release --verbose
#RUN RUN make -f Makefile release

FROM debian:buster-slim
COPY --from=build /app/target/release/feature_probe_server .

CMD [ "./feature_probe_server" ]
