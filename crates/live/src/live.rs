//! [`Live`] — a thin MoQ facade over [`iroh_moq::Moq`].
//!
//! Wraps connect / subscribe / publish so the room actor and the app share one
//! MoQ handle bound to the process-wide iroh [`Endpoint`]. `connect_and_subscribe`
//! carries the reconnect-on-subscribe-timeout behaviour the room layer relies on.

use std::time::Duration;

use iroh::{Endpoint, EndpointAddr};
use iroh_moq::{Moq, MoqProtocolHandler, MoqSession};
use moq_lite::BroadcastProducer;
use moq_media::subscribe::SubscribeBroadcast;
use n0_error::Result;
use tracing::{info, warn};

/// Clonable MoQ handle bound to a shared iroh [`Endpoint`].
#[derive(Clone)]
pub struct Live {
    pub moq: Moq,
}

impl Live {
    /// Build a MoQ facade over the given endpoint.
    pub fn new(endpoint: Endpoint) -> Self {
        Self {
            moq: Moq::new(endpoint),
        }
    }

    /// Open (or reuse) a MoQ session to `remote`.
    pub async fn connect(&self, remote: impl Into<EndpointAddr>) -> Result<MoqSession> {
        self.moq.connect(remote).await
    }

    /// Connect to `remote` and subscribe to a named broadcast.
    ///
    /// A cached MoQ session can be half-dead: the QUIC connection survives but
    /// the peer no longer serves the track, so `subscribe` hangs. Guard each
    /// attempt with a 5s timeout; on the first timeout, drop the stale session
    /// and retry once on a fresh connection before giving up.
    pub async fn connect_and_subscribe(
        &self,
        remote: impl Into<EndpointAddr>,
        broadcast_name: &str,
    ) -> Result<(MoqSession, SubscribeBroadcast)> {
        let remote = remote.into();
        let mut session = self.connect(remote.clone()).await?;
        info!(id=%session.conn().remote_id(), "new peer connected");

        let broadcast = match tokio::time::timeout(
            Duration::from_secs(5),
            session.subscribe(broadcast_name),
        )
        .await
        {
            Ok(Ok(broadcast)) => broadcast,
            Ok(Err(err)) => {
                session.close(0, b"subscribe failed");
                return Err(err.into());
            }
            Err(_) => {
                warn!(
                    id = %session.conn().remote_id(),
                    broadcast = %broadcast_name,
                    "subscribe timed out on cached session, closing and retrying with a fresh connection"
                );
                session.close(0, b"subscribe timeout");

                let mut session = self.connect(remote).await?;
                info!(
                    id = %session.conn().remote_id(),
                    broadcast = %broadcast_name,
                    "reconnected peer after subscribe timeout"
                );
                let broadcast =
                    tokio::time::timeout(Duration::from_secs(5), session.subscribe(broadcast_name))
                        .await
                        .map_err(|_| {
                            session.close(0, b"subscribe retry timeout");
                            n0_error::anyerr!(
                                "subscribe to '{broadcast_name}' timed out after reconnect"
                            )
                        })?
                        .inspect_err(|_err| {
                            session.close(0, b"subscribe retry failed");
                        })?;
                let broadcast =
                    SubscribeBroadcast::new(broadcast_name.to_string(), broadcast).await?;
                return Ok((session, broadcast));
            }
        };

        let broadcast = SubscribeBroadcast::new(broadcast_name.to_string(), broadcast).await?;
        Ok((session, broadcast))
    }

    /// The MoQ [`ProtocolHandler`](iroh::protocol::ProtocolHandler) to register
    /// on the iroh router under [`crate::ALPN`].
    pub fn protocol_handler(&self) -> MoqProtocolHandler {
        self.moq.protocol_handler()
    }

    /// Publish a broadcast under `name`.
    pub async fn publish(&self, name: impl ToString, producer: BroadcastProducer) -> Result<()> {
        self.moq.publish(name, producer).await
    }

    /// Tear down the MoQ session actor.
    pub fn shutdown(&self) {
        self.moq.shutdown();
    }
}
