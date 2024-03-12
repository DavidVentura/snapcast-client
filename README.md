Attempt at an implementation of the client-side of the [Snapcast](https://github.com/badaix/snapcast) [protocol](https://github.com/badaix/snapcast/blob/develop/doc/binary_protocol.md)

At a basic level, the protocol works, audio packets are not in any way synchronized before playback.

Only PCM audio plays back correctly via pulseaudio, but there are small gaps in playback
