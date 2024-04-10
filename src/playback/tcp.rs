use crate::playback::Player;
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};

pub struct Tcp {
    s: TcpStream,
    sample_rate: u16,
}

impl Player for Tcp {
    fn play(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    fn write(&mut self, buf: &mut [i16]) -> anyhow::Result<()> {
        // SAFETY: it's always safe to align i16 to u8
        let (_, converted, _) = unsafe { buf.align_to::<u8>() };
        self.s.write_all(converted)?;
        Ok(())
    }

    fn latency_ms(&self) -> anyhow::Result<u16> {
        // ?
        Ok(0)
    }
    fn set_volume(&mut self, _val: u8) -> anyhow::Result<()> {
        // ?
        Ok(())
    }
    fn sample_rate(&self) -> u16 {
        self.sample_rate
    }
}

impl Tcp {
    pub fn new<A: ToSocketAddrs>(addr: A, rate: usize) -> anyhow::Result<Tcp> {
        let s = TcpStream::connect(addr)?;
        Ok(Tcp {
            s,
            sample_rate: rate as u16,
        })
    }
}
