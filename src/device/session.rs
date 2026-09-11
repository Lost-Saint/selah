use std::error::Error;
use std::fmt::{self, Display, Formatter};

use super::{AUDIENT_VENDOR_ID, DetectedDevice, DeviceLocation, DeviceModel};

const USB_CLASS_APPLICATION_SPECIFIC: u8 = 0xfe;
const USB_CLASS_VENDOR_SPECIFIC: u8 = 0xff;

/// A non-audio USB interface suitable for Audient mixer control requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlInterface {
    pub number: u8,
    pub kind: ControlInterfaceKind,
}

/// The USB class used by a selected control interface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlInterfaceKind {
    ApplicationSpecific,
    VendorSpecific,
}

impl Display for ControlInterfaceKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApplicationSpecific => formatter.write_str("application/DFU"),
            Self::VendorSpecific => formatter.write_str("vendor-specific"),
        }
    }
}

/// Exclusive ownership of a safe Audient control interface.
///
/// Dropping the session releases the interface. Prefer [`DeviceSession::close`]
/// when the caller can await explicit release and handle a cleanup error.
pub struct DeviceSession {
    owner: SessionOwner<UsbTransport>,
}

impl DeviceSession {
    /// Opens and claims a non-audio control interface for a supported model.
    ///
    /// This function never detaches a kernel driver and never falls back to an
    /// audio-class interface.
    ///
    /// # Errors
    ///
    /// Returns a typed error if enumeration, opening, or claiming fails, or if
    /// the device has no safe control interface.
    pub async fn open(selected: &DetectedDevice) -> Result<Self, SessionError> {
        Ok(Self {
            owner: SessionOwner::open(UsbTransport, selected).await?,
        })
    }

    /// Returns the model associated with this session.
    #[must_use]
    pub fn model(&self) -> &'static DeviceModel {
        self.owner.model
    }

    /// Returns the claimed non-audio control interface.
    #[must_use]
    pub fn control_interface(&self) -> ControlInterface {
        self.owner.control
    }

    /// Releases the control interface and closes the session.
    ///
    /// # Errors
    ///
    /// Returns an error when the interface cannot be released cleanly.
    pub async fn close(self) -> Result<(), SessionError> {
        self.owner.close().await
    }
}

impl std::fmt::Debug for DeviceSession {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeviceSession")
            .field("model", &self.model().name)
            .field("control", &self.control_interface())
            .finish_non_exhaustive()
    }
}

struct SessionOwner<T>
where
    T: SessionTransport,
{
    transport: T,
    model: &'static DeviceModel,
    control: ControlInterface,
    handle: Option<T::Handle>,
}

impl<T> SessionOwner<T>
where
    T: SessionTransport,
{
    async fn open(transport: T, selected: &DetectedDevice) -> Result<Self, T::Error> {
        let acquired = transport.acquire(selected).await?;

        Ok(Self {
            transport,
            model: selected.model,
            control: acquired.control,
            handle: Some(acquired.handle),
        })
    }

    async fn close(mut self) -> Result<(), T::Error> {
        let Some(handle) = self.handle.take() else {
            return Ok(());
        };

        self.transport.release(handle, self.control).await
    }
}

struct AcquiredInterface<H> {
    control: ControlInterface,
    handle: H,
}

trait SessionTransport: Send + Sync {
    type Handle: Send;
    type Error;

    async fn acquire(
        &self,
        selected: &DetectedDevice,
    ) -> Result<AcquiredInterface<Self::Handle>, Self::Error>;

    async fn release(
        &self,
        handle: Self::Handle,
        control: ControlInterface,
    ) -> Result<(), Self::Error>;
}

struct UsbTransport;

impl SessionTransport for UsbTransport {
    type Handle = nusb::Interface;
    type Error = SessionError;

    async fn acquire(
        &self,
        selected: &DetectedDevice,
    ) -> Result<AcquiredInterface<Self::Handle>, Self::Error> {
        let model = selected.model;
        let mut devices = nusb::list_devices()
            .await
            .map_err(SessionError::Enumerate)?;

        let info = devices
            .find(|device| {
                matches_selection(
                    selected,
                    &DeviceLocation::from_info(device),
                    device.vendor_id(),
                    device.product_id(),
                )
            })
            .ok_or(SessionError::NotFound {
                product_id: model.product_id,
            })?;

        let control = select_control_interface(
            info.interfaces()
                .map(|interface| (interface.interface_number(), interface.class())),
        )
        .ok_or(SessionError::NoSafeControlInterface {
            product_id: model.product_id,
        })?;

        let device = info.open().await.map_err(SessionError::Open)?;
        let interface = device
            .claim_interface(control.number)
            .await
            .map_err(|source| SessionError::Claim {
                interface: control.number,
                source,
            })?;

        Ok(AcquiredInterface {
            control,
            handle: interface,
        })
    }

    async fn release(
        &self,
        interface: Self::Handle,
        control: ControlInterface,
    ) -> Result<(), Self::Error> {
        interface
            .release()
            .await
            .map_err(|source| SessionError::Release {
                interface: control.number,
                source,
            })
    }
}

fn matches_selection(
    selected: &DetectedDevice,
    location: &DeviceLocation,
    vendor_id: u16,
    product_id: u16,
) -> bool {
    selected.location == *location
        && vendor_id == AUDIENT_VENDOR_ID
        && product_id == selected.model.product_id
}

/// Failure to establish or close a safe device session.
#[derive(Clone, Debug)]
pub enum SessionError {
    Enumerate(nusb::Error),
    NotFound { product_id: u16 },
    NoSafeControlInterface { product_id: u16 },
    Open(nusb::Error),
    Claim { interface: u8, source: nusb::Error },
    Release { interface: u8, source: nusb::Error },
}

/// Stable error categories callers can use without depending on `nusb` details.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionErrorKind {
    PermissionDenied,
    Busy,
    Disconnected,
    UnsupportedInterface,
    Other,
}

impl SessionError {
    /// Returns the actionable category for this failure.
    #[must_use]
    pub fn kind(&self) -> SessionErrorKind {
        match self {
            Self::NotFound { .. } => SessionErrorKind::Disconnected,
            Self::NoSafeControlInterface { .. } => SessionErrorKind::UnsupportedInterface,
            Self::Enumerate(source)
            | Self::Open(source)
            | Self::Claim { source, .. }
            | Self::Release { source, .. } => classify_nusb_error(source.kind()),
        }
    }

    /// Gives the user a useful next step for this class of error.
    #[must_use]
    pub fn recovery_hint(&self) -> &'static str {
        match self.kind() {
            SessionErrorKind::Disconnected => "Reconnect the interface and scan again.",
            SessionErrorKind::UnsupportedInterface => {
                "Selah will not claim an audio interface. This model needs hardware investigation."
            }
            SessionErrorKind::PermissionDenied => {
                "Install the Selah udev rule, reconnect the interface, and try again."
            }
            SessionErrorKind::Busy => {
                "Another process owns the control interface. Close it and try again."
            }
            SessionErrorKind::Other => "Reconnect the interface and try again.",
        }
    }
}

fn classify_nusb_error(kind: nusb::ErrorKind) -> SessionErrorKind {
    match kind {
        nusb::ErrorKind::PermissionDenied => SessionErrorKind::PermissionDenied,
        nusb::ErrorKind::Busy => SessionErrorKind::Busy,
        nusb::ErrorKind::Disconnected | nusb::ErrorKind::NotFound => SessionErrorKind::Disconnected,
        _ => SessionErrorKind::Other,
    }
}

impl Display for SessionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Enumerate(source) => write!(formatter, "USB device scan failed: {source}"),
            Self::NotFound { product_id } => {
                write!(
                    formatter,
                    "Audient device {product_id:04x} is no longer present"
                )
            }
            Self::NoSafeControlInterface { product_id } => write!(
                formatter,
                "Audient device {product_id:04x} has no safe control interface"
            ),
            Self::Open(source) => write!(formatter, "could not open the Audient device: {source}"),
            Self::Claim { interface, source } => {
                write!(
                    formatter,
                    "could not claim control interface {interface}: {source}"
                )
            }
            Self::Release { interface, source } => {
                write!(
                    formatter,
                    "could not release control interface {interface}: {source}"
                )
            }
        }
    }
}

impl Error for SessionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Enumerate(source)
            | Self::Open(source)
            | Self::Claim { source, .. }
            | Self::Release { source, .. } => Some(source),
            Self::NotFound { .. } | Self::NoSafeControlInterface { .. } => None,
        }
    }
}

pub(crate) fn select_control_interface(
    interfaces: impl IntoIterator<Item = (u8, u8)>,
) -> Option<ControlInterface> {
    let mut vendor_specific = None;

    for (number, class) in interfaces {
        match class {
            USB_CLASS_APPLICATION_SPECIFIC => {
                return Some(ControlInterface {
                    number,
                    kind: ControlInterfaceKind::ApplicationSpecific,
                });
            }
            USB_CLASS_VENDOR_SPECIFIC if vendor_specific.is_none() => {
                vendor_specific = Some(ControlInterface {
                    number,
                    kind: ControlInterfaceKind::VendorSpecific,
                });
            }
            _ => {}
        }
    }

    vendor_specific
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use futures_lite::future::block_on;

    use super::{
        AcquiredInterface, ControlInterface, ControlInterfaceKind, DetectedDevice, DeviceLocation,
        SessionErrorKind, SessionOwner, SessionTransport, classify_nusb_error,
        select_control_interface,
    };

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum MockError {
        Acquire,
        Release,
    }

    #[derive(Default)]
    struct MockState {
        acquisitions: usize,
        releases: usize,
        handles_dropped: usize,
    }

    struct MockTransport {
        state: Arc<Mutex<MockState>>,
        acquire_error: bool,
        release_error: bool,
    }

    struct MockHandle(Arc<Mutex<MockState>>);

    impl Drop for MockHandle {
        fn drop(&mut self) {
            self.0.lock().unwrap().handles_dropped += 1;
        }
    }

    #[allow(
        clippy::unused_async_trait_impl,
        reason = "the synchronous mock preserves the asynchronous transport contract"
    )]
    impl SessionTransport for MockTransport {
        type Handle = MockHandle;
        type Error = MockError;

        async fn acquire(
            &self,
            _selected: &DetectedDevice,
        ) -> Result<AcquiredInterface<Self::Handle>, Self::Error> {
            self.state.lock().unwrap().acquisitions += 1;

            if self.acquire_error {
                return Err(MockError::Acquire);
            }

            Ok(AcquiredInterface {
                control: ControlInterface {
                    number: 4,
                    kind: ControlInterfaceKind::ApplicationSpecific,
                },
                handle: MockHandle(Arc::clone(&self.state)),
            })
        }

        async fn release(
            &self,
            _handle: Self::Handle,
            _control: ControlInterface,
        ) -> Result<(), Self::Error> {
            self.state.lock().unwrap().releases += 1;

            if self.release_error {
                Err(MockError::Release)
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn acquisition_failure_does_not_attempt_cleanup() {
        let state = Arc::new(Mutex::new(MockState::default()));
        let result = block_on(SessionOwner::open(
            mock_transport(&state, true, false),
            &selected_device(),
        ));

        assert!(matches!(result, Err(MockError::Acquire)));
        let state = state.lock().unwrap();
        assert_eq!(state.acquisitions, 1);
        assert_eq!(state.releases, 0);
        assert_eq!(state.handles_dropped, 0);
    }

    #[test]
    fn explicit_close_releases_and_drops_the_handle() {
        let state = Arc::new(Mutex::new(MockState::default()));
        let owner = block_on(SessionOwner::open(
            mock_transport(&state, false, false),
            &selected_device(),
        ))
        .unwrap();

        assert_eq!(block_on(owner.close()), Ok(()));
        let state = state.lock().unwrap();
        assert_eq!(state.releases, 1);
        assert_eq!(state.handles_dropped, 1);
    }

    #[test]
    fn cleanup_failure_is_returned_after_dropping_the_handle() {
        let state = Arc::new(Mutex::new(MockState::default()));
        let owner = block_on(SessionOwner::open(
            mock_transport(&state, false, true),
            &selected_device(),
        ))
        .unwrap();

        assert_eq!(block_on(owner.close()), Err(MockError::Release));
        let state = state.lock().unwrap();
        assert_eq!(state.releases, 1);
        assert_eq!(state.handles_dropped, 1);
    }

    #[test]
    fn dropping_an_open_session_drops_its_handle() {
        let state = Arc::new(Mutex::new(MockState::default()));
        let owner = block_on(SessionOwner::open(
            mock_transport(&state, false, false),
            &selected_device(),
        ))
        .unwrap();

        drop(owner);
        let state = state.lock().unwrap();
        assert_eq!(state.releases, 0);
        assert_eq!(state.handles_dropped, 1);
    }

    fn mock_transport(
        state: &Arc<Mutex<MockState>>,
        acquire_error: bool,
        release_error: bool,
    ) -> MockTransport {
        MockTransport {
            state: Arc::clone(state),
            acquire_error,
            release_error,
        }
    }

    fn selected_device() -> DetectedDevice {
        DetectedDevice {
            location: DeviceLocation {
                bus: "1".to_owned(),
                address: 2,
            },
            model: crate::device::supported_device(0x000d).unwrap(),
            reported_name: None,
            control_interface: None,
        }
    }

    #[test]
    fn classifies_actionable_usb_errors() {
        assert_eq!(
            classify_nusb_error(nusb::ErrorKind::PermissionDenied),
            SessionErrorKind::PermissionDenied
        );
        assert_eq!(
            classify_nusb_error(nusb::ErrorKind::Busy),
            SessionErrorKind::Busy
        );
        assert_eq!(
            classify_nusb_error(nusb::ErrorKind::Disconnected),
            SessionErrorKind::Disconnected
        );
        assert_eq!(
            classify_nusb_error(nusb::ErrorKind::NotFound),
            SessionErrorKind::Disconnected
        );
        assert_eq!(
            classify_nusb_error(nusb::ErrorKind::Other),
            SessionErrorKind::Other
        );
    }

    #[test]
    fn opening_requires_the_selected_attachment_and_model() {
        use super::{AUDIENT_VENDOR_ID, DetectedDevice, DeviceLocation, matches_selection};
        let location = DeviceLocation {
            bus: "1".to_owned(),
            address: 2,
        };
        let selected = DetectedDevice {
            location: location.clone(),
            model: crate::device::supported_device(0x000d).unwrap(),
            reported_name: None,
            control_interface: None,
        };
        assert!(matches_selection(
            &selected,
            &location,
            AUDIENT_VENDOR_ID,
            0x000d
        ));
        let other_address = DeviceLocation {
            address: 3,
            ..location.clone()
        };
        let other_bus = DeviceLocation {
            bus: "2".to_owned(),
            ..location.clone()
        };
        assert!(!matches_selection(
            &selected,
            &other_address,
            AUDIENT_VENDOR_ID,
            0x000d
        ));
        assert!(!matches_selection(
            &selected,
            &other_bus,
            AUDIENT_VENDOR_ID,
            0x000d
        ));
        assert!(!matches_selection(&selected, &location, 0x1234, 0x000d));
        assert!(!matches_selection(
            &selected,
            &location,
            AUDIENT_VENDOR_ID,
            0x0008
        ));
    }

    #[test]
    fn prefers_application_specific_interface() {
        let selected = select_control_interface([(0, 0x01), (3, 0xff), (4, 0xfe)]);

        assert_eq!(
            selected,
            Some(ControlInterface {
                number: 4,
                kind: ControlInterfaceKind::ApplicationSpecific,
            })
        );
    }

    #[test]
    fn uses_vendor_specific_interface_when_dfu_is_absent() {
        let selected = select_control_interface([(0, 0x01), (2, 0xff)]);

        assert_eq!(
            selected,
            Some(ControlInterface {
                number: 2,
                kind: ControlInterfaceKind::VendorSpecific,
            })
        );
    }

    #[test]
    fn refuses_to_select_an_audio_interface() {
        assert_eq!(select_control_interface([(0, 0x01), (1, 0x01)]), None);
    }
}
