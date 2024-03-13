use crate::playback::Player;
use std::io::Write;

pub(crate) struct File {
    f: std::fs::File,
}
impl Player for File {
    fn play(&self) -> anyhow::Result<()> {
        Ok(())
    }
    fn write(&mut self, buf: &[i16]) -> anyhow::Result<()> {
        // SAFETY: it's always safe to align i16 to u8
        let (_, converted, _) = unsafe { buf.align_to::<u8>() };
        self.f.write_all(converted)?;
        Ok(())
    }
}

impl File {
    pub fn new(p: &std::path::Path) -> anyhow::Result<File> {
        Ok(File {
            f: std::fs::File::create(p)?,
        })
    }
}
