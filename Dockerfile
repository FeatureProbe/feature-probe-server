FROM rust:latest as build

WORKDIR /app
COPY . /app
COPY id_rsa /root/.ssh/id_rsa
RUN ssh-keyscan github.com >> /root/.ssh/known_hosts

RUN make -f Makefile release

FROM debian:buster-slim
COPY --from=build /app/target/release/feature_probe_server .

CMD [ "./feature_probe_server" ]