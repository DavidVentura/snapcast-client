use crate::playback::Player;
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};

pub(crate) struct Tcp {
    s: TcpStream,
}

impl Player for Tcp {
    fn play(&self) -> anyhow::Result<()> {
        Ok(())
    }
    fn write(&mut self, buf: &[i16]) -> anyhow::Result<()> {
        // SAFETY: it's always safe to align i16 to u8
        let (_, converted, _) = unsafe { buf.align_to::<u8>() };
        self.s.write_all(converted)?;
        Ok(())
    }

    fn latency_ms(&self) -> anyhow::Result<u16> {
        // ?
        Ok(0)
    }
    fn set_volume(&self, _val: u8) -> anyhow::Result<()> {
        // ?
        Ok(())
    }
}

impl Tcp {
    pub fn new<A: ToSocketAddrs>(addr: A) -> anyhow::Result<Tcp> {
        let s = TcpStream::connect(addr)?;
        Ok(Tcp { s })
    }
}
