# Coreelec
FROM ubuntu:20.04
RUN apt-get update
RUN DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends curl cmake libopus0 make libpulse0 libasound2 pkg-config ca-certificates gcc libc-dev libasound2-dev
RUN curl --proto '=https' -sSf https://sh.rustup.rs  | sh -s -- -y
