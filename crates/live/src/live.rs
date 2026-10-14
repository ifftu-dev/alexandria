//! [`Live`] — a thin MoQ facade over [`iroh_moq::Moq`].
//!
//! Wraps connect / subscribe / publish so the room actor and the app share one
//! MoQ handle bound to the process-wide iroh [`Endpoint`]. `connect_and_subscribe`
//! never closes the shared session; the room layer retries on it.

use std::time::Duration;

use iroh::{Endpoint, EndpointAddr};
use iroh_moq::{Moq, MoqProtocolHandler, MoqSession};
use moq_lite::{BroadcastConsumer, BroadcastProducer};
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
    /// The MoQ session is shared with everything else between the two peers,
    /// including what we publish to them, so a failed or slow subscription
    /// never closes it: subscribing waits for the peer to *announce* the name,
    /// and a peer that has not announced yet looks exactly like a slow one.
    /// The error goes back to the room, which retries on the same session.
    ///
    /// Only a session whose connection has actually closed is replaced, once,
    /// by a fresh connection (the actor evicts dead sessions on connect).
    pub async fn connect_and_subscribe(
        &self,
        remote: impl Into<EndpointAddr>,
        broadcast_name: &str,
    ) -> Result<(MoqSession, SubscribeBroadcast)> {
        let remote = remote.into();
        let session = self.connect(remote.clone()).await?;
        info!(id=%session.conn().remote_id(), "new peer connected");

        let (session, broadcast) = match Self::subscribe_once(session, broadcast_name).await {
            Ok(subscribed) => subscribed,
            Err((session, err)) if session.conn().close_reason().is_some() => {
                warn!(
                    id = %session.conn().remote_id(),
                    broadcast = %broadcast_name,
                    "connection closed while subscribing, retrying on a fresh connection: {err:#}"
                );
                let session = self.connect(remote).await?;
                Self::subscribe_once(session, broadcast_name)
                    .await
                    .map_err(|(_, err)| err)?
            }
            Err((_, err)) => return Err(err),
        };

        let broadcast = SubscribeBroadcast::new(broadcast_name.to_string(), broadcast).await?;
        Ok((session, broadcast))
    }

    /// One subscribe attempt with a 5s limit. Hands the session back on
    /// failure so the caller can see whether its connection is still alive;
    /// it is never closed here.
    async fn subscribe_once(
        mut session: MoqSession,
        broadcast_name: &str,
    ) -> std::result::Result<(MoqSession, BroadcastConsumer), (MoqSession, n0_error::AnyError)>
    {
        match tokio::time::timeout(Duration::from_secs(5), session.subscribe(broadcast_name)).await
        {
            Ok(Ok(broadcast)) => Ok((session, broadcast)),
            Ok(Err(err)) => Err((session, err.into())),
            Err(_) => {
                let err = n0_error::anyerr!("'{broadcast_name}' was not announced within 5s");
                Err((session, err))
            }
        }
    }

    /// The MoQ [`ProtocolHandler`](iroh::protocol::ProtocolHandler) to register
    /// on the iroh router under [`crate::ALPN`].
    pub fn protocol_handler(&self) -> MoqProtocolHandler {
        self.moq.protocol_handler()
    }

    /// Publish a broadcast under `name`. Returns the publish generation that
    /// [`Self::unpublish`] takes.
    pub async fn publish(&self, name: impl ToString, producer: BroadcastProducer) -> Result<u64> {
        self.moq.publish(name, producer).await
    }

    /// Withdraw the broadcast published as `generation` under `name`, without
    /// waiting. Safe to call from `Drop`.
    pub fn try_unpublish(&self, name: impl ToString, generation: u64) {
        self.moq.try_unpublish(name, generation);
    }

    /// Tear down the MoQ session actor.
    pub fn shutdown(&self) {
        self.moq.shutdown();
    }
}
