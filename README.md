Implementation of the client-side of the [Snapcast](https://github.com/badaix/snapcast) [protocol](https://github.com/badaix/snapcast/blob/develop/doc/binary_protocol.md) (snapclient).

At a basic level, the player works, though audio packets are not in any way synchronized before playback.

Only PCM/Opus are implemented, and only File/Pulse/Alsa work for output devices.

On my machine, there are small gaps every 11 seconds but I think it's alsa/pulse config.
