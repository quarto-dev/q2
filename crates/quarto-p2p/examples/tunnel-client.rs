//! Minimal guest-side driver for `q2 preview --share` sessions.
//!
//! Until `q2 preview --join` lands (live-share plan Phase 3,
//! bd-6y0p1bne), this example is the reference guest: it joins a shared
//! preview session and serves it on a local loopback port.
//!
//! ```text
//! cargo run -p quarto-p2p --example tunnel-client -- <q2preview…> [local-port]
//! ```

use quarto_p2p::{PreviewShareTicket, TunnelClient, TunnelClientConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut args = std::env::args().skip(1);
    let ticket: PreviewShareTicket = args
        .next()
        .ok_or("usage: tunnel-client <q2preview-ticket> [local-port]")?
        .parse()?;
    let port: u16 = match args.next() {
        Some(p) => p.parse()?,
        None => 0,
    };

    let (local, handle) = TunnelClient::bind(
        TunnelClientConfig::default(),
        ticket,
        ([127, 0, 0, 1], port).into(),
    )
    .await?;
    println!("joined shared preview session: http://{local}/");

    // Report status transitions until Ctrl-C.
    let mut status = handle.status();
    loop {
        println!("tunnel status: {:?}", *status.borrow_and_update());
        if status.changed().await.is_err() {
            return Ok(());
        }
    }
}
