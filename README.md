Implementation of the client-side of the [Snapcast](https://github.com/badaix/snapcast) [protocol](https://github.com/badaix/snapcast/blob/develop/doc/binary_protocol.md) (snapclient).

The player works as a proof of concept, though it sometimes crashes when using the ALSA backend and adjusting the latency.

Only PCM/Opus are implemented, and only File/Pulse/Alsa/Tcp work for output devices.


To use the `TCP` module, (or to avoid having to link to `libpulse`), you can enable the 'simple protocol' module:
```
pactl load-module module-simple-protocol-tcp rate=48000 format=s16le channels=2 playback=true port=12345 listen=127.0.0.1
```


## Latency

This implementation behaves very similarly as the official one in regards to latency; measured with the scope and an 'audio/video sync test' playback:

0ms added latency on the rs implementation:

![](https://github.com/DavidVentura/snapcast-client/blob/master/images/snapclient-v-rs-0ms-conf-lat.png?raw=true)

5ms added latency on the rs implementation:
![](https://github.com/DavidVentura/snapcast-client/blob/master/images/snapclient-v-rs-5ms-conf-lat.png?raw=true)

8ms added latency on the rs implementation:
![](https://github.com/DavidVentura/snapcast-client/blob/master/images/snapclient-v-rs-8ms-conf-lat.png?raw=true)
