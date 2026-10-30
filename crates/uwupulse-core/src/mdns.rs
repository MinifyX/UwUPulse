//! Bonjour: `_uwupulse._tcp.local.` with TXT `v=1`, `id`, `pk`, so the
//! phone finds the desktop for code comparison and reconnects after an
//! address change.

use mdns_sd::{ServiceDaemon, ServiceInfo};

pub const SERVICE_TYPE: &str = "_uwupulse._tcp.local.";

/// A running advertisement; dropping it withdraws it.
pub struct Advert {
    daemon: ServiceDaemon,
    fullname: String,
}

pub fn advertise(name: &str, id: &str, pk_b64: &str, port: u16) -> Result<Advert, String> {
    let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
    // A dot in the instance name would need escaping on some resolvers.
    let instance: String = name.replace('.', "-");
    let host = format!("uwupulse-{}.local.", &id[..id.len().min(8)]);
    let props = [("v", "1"), ("id", id), ("pk", pk_b64)];
    let info = ServiceInfo::new(SERVICE_TYPE, &instance, &host, "", port, &props[..])
        .map_err(|e| e.to_string())?
        .enable_addr_auto();
    let fullname = info.get_fullname().to_owned();
    daemon.register(info).map_err(|e| e.to_string())?;
    Ok(Advert { daemon, fullname })
}

impl Drop for Advert {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}
