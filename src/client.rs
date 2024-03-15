use crate::proto::ClientHello;
pub struct Client {
    mac: String,
    hostname: String,
}

impl Client {
    pub fn new(mac: String, hostname: String) -> Client {
        Client { mac, hostname }
    }
    pub fn hello(&self) -> Vec<u8> {
        ClientHello {
            Arch: "x86_64",
            ClientName: "CoolClient",
            HostName: &self.hostname,
            ID: &self.mac,
            Instance: 1,
            MAC: &self.mac,
            SnapStreamProtocolVersion: 2,
            Version: "0.17.1",
            OS: "an os",
        }
        .as_buf()
    }
}
