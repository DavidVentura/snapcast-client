.PHONY: install-wireshark-dissector static-server

install-wireshark-dissector:
	mkdir -p ~/.local/lib/wireshark/plugins
	ln -s $(PWD)/dissector.lua ~/.local/lib/wireshark/plugins/SnapCast-dissector.lua

# audiopus_sys links libopus dynamically and builds it with the host gcc, which
# leaves a static musl binary calling into an unresolved libopus.so; force the
# bundled libopus static and built with musl-gcc so it links into the binary
static-server:
	CC=musl-gcc OPUS_STATIC=1 \
		cargo build -p snapcast-server --bin snapcast-server --target x86_64-unknown-linux-musl

