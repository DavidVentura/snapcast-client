Implementation of the client-side of the [Snapcast](https://github.com/badaix/snapcast) [protocol](https://github.com/badaix/snapcast/blob/develop/doc/binary_protocol.md) (snapclient).

The player works as a proof of concept, though I'm not sure how well the synchronization mechanism works.

Only PCM/Opus are implemented, and only File/Pulse/Alsa/Tcp work for output devices.


To use the `TCP` module, (or to avoid having to link to `libpulse`), you can enable the 'simple protocol' module:
```
pactl load-module module-simple-protocol-tcp rate=48000 format=s16le channels=2 playback=true port=12345 listen=127.0.0.1
```
