use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::Stream;

use super::DiscoveryError;

/// A lifecycle event from the operating system's USB monitor.
#[derive(Clone, Debug)]
pub enum DeviceWatchEvent {
    Started,
    DevicesChanged,
    Failed(DiscoveryError),
}

/// Watches USB connection changes without polling or opening any device.
pub fn watch_events() -> impl Stream<Item = DeviceWatchEvent> {
    DeviceWatch {
        state: match nusb::watch_devices() {
            Ok(watch) => WatchState::Active {
                watch,
                announce_start: true,
            },
            Err(error) => WatchState::Failed(Some(DiscoveryError::watch(error))),
        },
    }
}

struct DeviceWatch {
    state: WatchState,
}

enum WatchState {
    Active {
        watch: nusb::hotplug::HotplugWatch,
        announce_start: bool,
    },
    Failed(Option<DiscoveryError>),
}

impl Stream for DeviceWatch {
    type Item = DeviceWatchEvent;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match &mut self.state {
            WatchState::Active { announce_start, .. } if *announce_start => {
                *announce_start = false;
                Poll::Ready(Some(DeviceWatchEvent::Started))
            }
            WatchState::Active { watch, .. } => Pin::new(watch)
                .poll_next(context)
                .map(|event| event.map(|_| DeviceWatchEvent::DevicesChanged)),
            WatchState::Failed(error) => error.take().map_or(Poll::Pending, |error| {
                Poll::Ready(Some(DeviceWatchEvent::Failed(error)))
            }),
        }
    }
}
