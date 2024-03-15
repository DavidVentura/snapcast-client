docker buildx build -t tag . --platform linux/arm/v7 
docker run --platform linux/arm/v7 -it tag bash -c '. $HOME/.cargo/env && cargo build --features alsa,pulse'
