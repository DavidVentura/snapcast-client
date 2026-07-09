use crate::proto::Base;

/// What the driver must read next off the socket to advance a protocol machine.
pub enum Action {
    ReadHeader,
    ReadPacket(u32),
}

/// Bytes the driver read off the socket, fed back into a protocol machine.
pub enum Event<'a> {
    HeaderReceived(&'a [u8]),
    PacketReceived(&'a [u8]),
}

/// The snapcast two-phase framing shared by the client and server machines: a
/// fixed 26-byte [`Base`] header announces the payload length, then that many
/// payload bytes follow. Holds the parsed header while its payload is read.
pub(crate) struct Framing {
    state: FramingState,
}

enum FramingState {
    ReadingHeader,
    ReadingPacket(Base),
}

impl Framing {
    pub(crate) fn new() -> Framing {
        Framing {
            state: FramingState::ReadingHeader,
        }
    }

    pub(crate) fn next_action(&self) -> Action {
        match &self.state {
            FramingState::ReadingHeader => Action::ReadHeader,
            FramingState::ReadingPacket(base) => Action::ReadPacket(base.size),
        }
    }

    pub(crate) fn on_header(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.state = FramingState::ReadingPacket(Base::try_from(bytes)?);
        Ok(())
    }

    /// Consume the header parsed by the preceding [`Framing::on_header`], leaving
    /// the framing ready for the next header. Panics if no header is pending,
    /// which can only happen if the driver feeds a packet out of order.
    pub(crate) fn take_base(&mut self) -> Base {
        match std::mem::replace(&mut self.state, FramingState::ReadingHeader) {
            FramingState::ReadingPacket(base) => base,
            FramingState::ReadingHeader => panic!("PacketReceived without a pending header"),
        }
    }
}
