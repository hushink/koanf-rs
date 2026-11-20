//! Watch events and subscription guards.

use std::fmt;
use std::sync::Arc;

use crate::config::ConfigInner;
use crate::diff::ChangeSet;
use crate::error::Error;

#[cfg(feature = "async")]
use std::pin::Pin;
#[cfg(feature = "async")]
use std::task::{Context, Poll};

#[cfg(feature = "async")]
use futures_channel::mpsc::UnboundedReceiver;
#[cfg(feature = "async")]
use futures_core::Stream;

/// An event delivered to watch subscribers.
///
/// Cloning an event is cheap: the snapshot and change set are reference
/// counted, and errors are shared through an `Arc`.
#[derive(Clone, Debug)]
pub enum WatchEvent {
    /// At least one source changed and the merged configuration differs.
    Changed {
        /// The new immutable snapshot.
        snapshot: crate::config::Snapshot,
        /// The changes between the previous and new snapshot.
        changes: ChangeSet,
    },
    /// A source could not be reloaded. The previous snapshot remains active.
    Error {
        /// The reload or watch error.
        error: Arc<Error>,
    },
}

/// A subscription guard returned by `Config::watch` and
/// `Config::watch_channel`.
///
/// Dropping the guard unregisters the subscription. The last guard (together
/// with the last `Config` handle) stops the underlying source watchers.
pub struct WatchGuard {
    inner: Arc<ConfigInner>,
    id: u64,
}

impl WatchGuard {
    pub(crate) fn new(inner: Arc<ConfigInner>, id: u64) -> Self {
        Self { inner, id }
    }
}

impl Drop for WatchGuard {
    fn drop(&mut self) {
        self.inner.remove_subscription(self.id);
    }
}

impl fmt::Debug for WatchGuard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WatchGuard")
            .field("id", &self.id)
            .finish()
    }
}

/// An asynchronous stream of [`WatchEvent`] values.
///
/// This is available with the `async` feature. It is runtime-agnostic: use it
/// with any executor that can drive a [`futures_core::Stream`], or call
/// [`WatchStream::recv`] directly.
#[cfg(feature = "async")]
pub struct WatchStream {
    receiver: UnboundedReceiver<WatchEvent>,
}

#[cfg(feature = "async")]
impl WatchStream {
    pub(crate) fn new(receiver: UnboundedReceiver<WatchEvent>) -> Self {
        Self { receiver }
    }

    /// Waits for the next event, returning `None` once the subscription ends.
    pub async fn recv(&mut self) -> Option<WatchEvent> {
        std::future::poll_fn(|context| Pin::new(&mut self.receiver).poll_next(context)).await
    }
}

#[cfg(feature = "async")]
impl Stream for WatchStream {
    type Item = WatchEvent;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.get_mut().receiver).poll_next(context)
    }
}

#[cfg(feature = "async")]
impl fmt::Debug for WatchStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WatchStream")
            .finish_non_exhaustive()
    }
}
