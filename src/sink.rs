/// Trait for sending or handling events.
///
/// Used as the backbone for a subscriber.
pub trait Sink<T> {
    /// Send an event
    fn send(&self, event: T);
    /// Returns true if the sink is still connected
    fn is_connected(&self) -> bool;
}

#[cfg(feature = "flume")]
impl<T> Sink<T> for flume::Sender<T> {
    fn send(&self, event: T) {
        let _ = self.send(event);
    }

    fn is_connected(&self) -> bool {
        !self.is_disconnected()
    }
}

#[cfg(feature = "tokio")]
impl<T> Sink<T> for tokio::sync::mpsc::UnboundedSender<T> {
    fn send(&self, event: T) {
        let _ = self.send(event);
    }

    fn is_connected(&self) -> bool {
        !self.is_closed()
    }
}

#[cfg(feature = "tokio")]
impl<T> Sink<T> for alloc::sync::Weak<tokio::sync::Notify> {
    fn send(&self, _: T) {
        if let Some(notify) = self.upgrade() {
            notify.notify_one()
        }
    }

    fn is_connected(&self) -> bool {
        self.strong_count() > 0
    }
}
