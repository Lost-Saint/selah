use iced::Task;

use crate::device::{DeviceWatchEvent, MonitorToggle};
use crate::mixer::ChannelStrip;
use crate::monitor::{ToggleControl, VolumeControl};
use crate::routing::{
    DigitalOutputMode, OutputRouteControl, Route, RouteStatus, RoutingDestination,
    digital_output_mode_available, reset_route,
};

use super::message::{
    ChannelLevelOutcome, ChannelPolarityOutcome, DigitalOutputModeOutcome, FeedbackOutcome,
    Message, MonitorToggleOutcome, RoutingOutcome, SpeakerVolumeOutcome,
};
use super::state::{App, DeviceStatus, WatchStatus, selected_device, toggle_index};
use super::subscription::{feedback_cadence_ms, monitor_due};
use super::tasks::{
    adopt_selection, channel_level_task, channel_polarity_task, digital_output_mode_task,
    finish_scan, monitor_toggle_task, refresh_task, request_scan, routing_task, volume_task,
};

pub(crate) fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::DeviceSelected(location) => {
            app.selected = Some(location);
            adopt_selection(app)
        }
        Message::DeviceWatch(event) => device_watch_event(app, event),
        Message::Refresh => request_scan(app),
        Message::DiscoveryFinished(Ok(report)) if !report.supported.is_empty() => {
            tracing::info!(
                supported = report.supported.len(),
                unsupported = report.unsupported.len(),
                "Audient device scan completed"
            );
            let still_there = app
                .selected
                .as_ref()
                .is_some_and(|wanted| report.supported.iter().any(|d| &d.location == wanted));
            if !still_there {
                app.selected = None;
            }
            app.status = DeviceStatus::Ready(report);
            adopt_selection(app)
        }
        Message::DiscoveryFinished(Ok(report)) if !report.unsupported.is_empty() => {
            tracing::warn!(
                unsupported = report.unsupported.len(),
                "Found an unrecognized Audient interface"
            );
            app.status = DeviceStatus::Unsupported(report);
            app.selected = None;
            app.speaker = VolumeControl::default();
            app.toggles = Default::default();
            app.routes = Vec::new();
            app.digital_output_mode = ToggleControl::default();
            app.channels = Vec::new();
            app.meters = Vec::new();
            app.feedback_notice = None;
            app.feedback_in_flight = false;
            finish_scan(app)
        }
        Message::DiscoveryFinished(Ok(_)) => {
            tracing::info!("No Audient interface detected");
            app.status = DeviceStatus::Empty;
            app.selected = None;
            app.speaker = VolumeControl::default();
            app.toggles = Default::default();
            app.routes = Vec::new();
            app.digital_output_mode = ToggleControl::default();
            app.channels = Vec::new();
            app.meters = Vec::new();
            app.feedback_notice = None;
            app.feedback_in_flight = false;
            finish_scan(app)
        }
        Message::DiscoveryFinished(Err(error)) => {
            tracing::error!(%error, "Audient device scan failed");
            app.status = DeviceStatus::Failed(error);
            app.selected = None;
            app.speaker = VolumeControl::default();
            app.toggles = Default::default();
            app.routes = Vec::new();
            app.digital_output_mode = ToggleControl::default();
            app.channels = Vec::new();
            app.meters = Vec::new();
            app.feedback_notice = None;
            app.feedback_in_flight = false;
            finish_scan(app)
        }
        Message::SpeakerVolumeChanged(level) => request_speaker_volume(app, level),
        Message::SpeakerVolumeFinished(outcome) => finish_speaker_volume(app, outcome),
        Message::InputFilterChanged(group) => {
            app.input_filter.toggle(group);
            Task::none()
        }
        Message::RouteSelected(route) => request_routing(app, route),
        Message::RouteReset(destination) => request_route_reset(app, destination),
        Message::RoutingFinished(outcome) => finish_routing(app, outcome),
        Message::DigitalOutputModeSelected(mode) => request_digital_output_mode(app, mode),
        Message::DigitalOutputModeFinished(outcome) => finish_digital_output_mode(app, outcome),
        Message::MonitorToggleChanged { toggle, on } => request_monitor_toggle(app, toggle, on),
        Message::MonitorToggleFinished(outcome) => finish_monitor_toggle(app, outcome),
        Message::ChannelLevelChanged { channel, level } => {
            request_channel_level(app, channel, level)
        }
        Message::ChannelLevelFinished(outcome) => finish_channel_level(app, outcome),
        Message::ChannelPolarityChanged { channel, flipped } => {
            request_channel_polarity(app, channel, flipped)
        }
        Message::ChannelPolarityFinished(outcome) => finish_channel_polarity(app, outcome),
        Message::FeedbackTick => request_feedback(app),
        Message::FeedbackFinished(outcome) => finish_feedback(app, outcome),
    }
}

/// Handles the operating-system USB monitor without polling.
///
/// Connection changes trigger a fresh descriptor scan; a failed monitor
/// falls back to the manual scan button.
fn device_watch_event(app: &mut App, event: DeviceWatchEvent) -> Task<Message> {
    match event {
        DeviceWatchEvent::Started => {
            tracing::info!("Watching for USB device changes");
            app.watch_status = WatchStatus::Active;
            request_scan(app)
        }
        DeviceWatchEvent::DevicesChanged => request_scan(app),
        DeviceWatchEvent::Failed(error) => {
            tracing::warn!(%error, "Automatic USB device detection is unavailable");
            app.watch_status = WatchStatus::Failed;
            request_scan(app)
        }
    }
}

/// Starts one bounded feedback poll unless one already owns a session.
///
/// Ticks arriving while a poll is in flight are dropped, never queued: at
/// most one session exists per tick, each tick holds it briefly, and the
/// subscription disappears entirely when no supported device is present.
fn request_feedback(app: &mut App) -> Task<Message> {
    let Some(cadence_ms) = feedback_cadence_ms(app) else {
        return Task::none();
    };
    if app.feedback_in_flight {
        return Task::none();
    }
    app.feedback_tick = app.feedback_tick.wrapping_add(1);
    refresh_task(app, monitor_due(app.feedback_tick, cadence_ms))
}

/// Adopts a feedback poll result: hardware values win over local state.
///
/// Monitor fields the device answered become confirmed and move their
/// controls; controls with a send in flight keep their pending value until
/// the send completes. Meter levels replace the previous block only when
/// their length still matches the current strips, so a completion from a
/// replaced device cannot resize or shift live meters. A failed poll blanks
/// the meters — a frozen meter presented as live would be a lie — but keeps
/// the last confirmed monitor values underneath a retry notice.
fn finish_feedback(app: &mut App, outcome: FeedbackOutcome) -> Task<Message> {
    let current = selected_device(app).map(|device| device.location.clone());
    if current.as_ref() != Some(&outcome.location) {
        // A completion from a replaced attachment: it must neither adopt
        // state nor clear a newer poll's in-flight flag. With no device at
        // all the flag cannot belong to a running task, so reset it.
        if current.is_none() {
            app.feedback_in_flight = false;
        }
        return Task::none();
    }
    app.feedback_in_flight = false;
    match outcome.result {
        Ok(snapshot) => {
            app.feedback_notice = None;
            if outcome.read_monitor
                && let Some(monitor) = snapshot.monitor
            {
                adopt_monitor_snapshot(app, &monitor);
            }
            adopt_meter_levels(app, snapshot.meters);
        }
        Err(error) => {
            if app.feedback_notice.is_none() {
                tracing::warn!(%error, "Device feedback is unavailable");
            } else {
                tracing::debug!(%error, "Device feedback poll failed");
            }
            app.meters.fill(None);
            app.feedback_notice = Some(error);
        }
    }
    Task::none()
}

fn adopt_monitor_snapshot(app: &mut App, monitor: &crate::device::MonitorSnapshot) {
    if let Some(level) = monitor.speaker_level
        && app.speaker.apply_confirmed(level)
    {
        tracing::debug!(level, "Speaker level confirmed by hardware");
    }
    for (toggle, on) in &monitor.toggles {
        if app.toggles[toggle_index(*toggle)].apply_confirmed(*on) {
            tracing::debug!(?toggle, on, "Monitor toggle confirmed by hardware");
        }
    }
    if let Some(mode) = monitor.digital_output_mode
        && app.digital_output_mode.apply_confirmed(mode.as_toggle())
    {
        tracing::debug!(
            mode = mode.label(),
            "Digital output mode confirmed by hardware"
        );
    }
}

fn adopt_meter_levels(app: &mut App, meters: Option<Vec<u8>>) {
    match meters {
        Some(levels) if levels.len() == app.meters.len() => {
            for (slot, level) in app.meters.iter_mut().zip(levels) {
                *slot = Some(level);
            }
        }
        Some(_) => {
            tracing::debug!("Ignoring meter block from a replaced device");
        }
        None => {
            app.meters.fill(None);
        }
    }
}

/// Starts a background speaker-volume send, queuing when one is in flight.
///
/// The USB work runs in the returned task, away from Iced's UI thread.
fn request_speaker_volume(app: &mut App, level: f32) -> Task<Message> {
    let Some(device) = selected_device(app) else {
        return Task::none();
    };

    match app.speaker.request(level) {
        Some(send) => volume_task(device, send),
        None => Task::none(),
    }
}

/// Records a background send result and starts the queued level, if any.
fn finish_speaker_volume(app: &mut App, outcome: SpeakerVolumeOutcome) -> Task<Message> {
    let SpeakerVolumeOutcome { level, result } = outcome;
    if !app.speaker.is_in_flight(level) {
        return Task::none();
    }

    match &result {
        Ok(()) => tracing::info!(level, "Speaker volume sent"),
        Err(error) => tracing::warn!(level, %error, "Speaker volume send failed"),
    }

    match app.speaker.finish(level, result) {
        Some(next) => {
            if let Some(device) = selected_device(app) {
                volume_task(device, next)
            } else {
                app.speaker.drop_pending();
                Task::none()
            }
        }
        None => Task::none(),
    }
}

fn route_control(
    app: &mut App,
    destination: RoutingDestination,
) -> Option<&mut OutputRouteControl> {
    app.routes
        .iter_mut()
        .find(|output| output.destination == destination)
}

/// Starts a named output route; ignored while that output has one in flight.
///
/// The USB work runs in the returned task, away from Iced's UI thread.
fn request_routing(app: &mut App, route: Route) -> Task<Message> {
    let Some(device) = selected_device(app) else {
        return Task::none();
    };
    let Some(output) = route_control(app, route.destination) else {
        return Task::none();
    };
    // Re-selecting the dropdown's current value is a no-op, unless the last
    // send failed and the same choice is an explicit retry.
    if output.control.requested() == Some(route.source)
        && !matches!(output.control.status(), RouteStatus::Failed { .. })
    {
        return Task::none();
    }

    match output.control.request(route.source) {
        Some(_) => routing_task(device, route),
        None => Task::none(),
    }
}

fn request_route_reset(app: &mut App, destination: RoutingDestination) -> Task<Message> {
    let Some(device) = selected_device(app) else {
        return Task::none();
    };
    let Ok(route) = reset_route(device.model, destination) else {
        return Task::none();
    };
    request_routing(app, route)
}

/// Records a routing send result; stale completions after a rescan are ignored.
fn finish_routing(app: &mut App, outcome: RoutingOutcome) -> Task<Message> {
    let RoutingOutcome { route, result } = outcome;
    let Some(output) = route_control(app, route.destination) else {
        return Task::none();
    };
    if !output.control.is_in_flight(route.source) {
        return Task::none();
    }

    match &result {
        Ok(()) => tracing::info!(
            destination = %route.destination.label(),
            source = route.source.label(),
            "Output route sent"
        ),
        Err(error) => tracing::warn!(
            destination = %route.destination.label(),
            source = route.source.label(),
            %error,
            "Output route send failed"
        ),
    }

    let _ = output.control.finish(route.source, result);
    Task::none()
}

fn request_digital_output_mode(app: &mut App, mode: DigitalOutputMode) -> Task<Message> {
    let Some(device) = selected_device(app) else {
        return Task::none();
    };
    if !digital_output_mode_available(&device) {
        return Task::none();
    }
    match app.digital_output_mode.request(mode.as_toggle()) {
        Some(_) => digital_output_mode_task(device, mode),
        None => Task::none(),
    }
}

fn finish_digital_output_mode(app: &mut App, outcome: DigitalOutputModeOutcome) -> Task<Message> {
    let DigitalOutputModeOutcome { mode, result } = outcome;
    let value = mode.as_toggle();
    if !app.digital_output_mode.is_in_flight(value) {
        return Task::none();
    }
    match &result {
        Ok(()) => tracing::info!(mode = mode.label(), "Digital output mode sent"),
        Err(error) => tracing::warn!(mode = mode.label(), %error, "Digital output mode failed"),
    }
    let _ = app.digital_output_mode.finish(value, result);
    Task::none()
}

/// Starts a monitor-toggle send; ignored while that toggle has one in flight.
///
/// The USB work runs in the returned task, away from Iced's UI thread.
fn request_monitor_toggle(app: &mut App, toggle: MonitorToggle, on: bool) -> Task<Message> {
    let Some(device) = selected_device(app) else {
        return Task::none();
    };

    match app.toggles[toggle_index(toggle)].request(on) {
        Some(send) => monitor_toggle_task(device, toggle, send),
        None => Task::none(),
    }
}

/// Records a toggle send result; stale completions after a rescan are ignored.
fn finish_monitor_toggle(app: &mut App, outcome: MonitorToggleOutcome) -> Task<Message> {
    let MonitorToggleOutcome { toggle, on, result } = outcome;
    let control = &mut app.toggles[toggle_index(toggle)];
    if !control.is_in_flight(on) {
        return Task::none();
    }

    match &result {
        Ok(()) => tracing::info!(?toggle, on, "Monitor toggle sent"),
        Err(error) => tracing::warn!(?toggle, on, %error, "Monitor toggle send failed"),
    }

    let _ = control.finish(on, result);
    Task::none()
}

/// Looks up the strip for a channel, ignoring completions for strips that
/// no longer exist after a rescan or model change.
fn channel_strip(app: &mut App, channel: u8) -> Option<&mut ChannelStrip> {
    app.channels.get_mut(channel as usize)
}

/// Starts a background channel-level send, queuing when one is in flight.
///
/// The USB work runs in the returned task, away from Iced's UI thread.
fn request_channel_level(app: &mut App, channel: u8, level: f32) -> Task<Message> {
    let Some(device) = selected_device(app) else {
        return Task::none();
    };
    let Some(strip) = channel_strip(app, channel) else {
        return Task::none();
    };

    match strip.level.request(level) {
        Some(send) => channel_level_task(device, channel, send),
        None => Task::none(),
    }
}

/// Records a channel-level result and starts the queued level, if any.
fn finish_channel_level(app: &mut App, outcome: ChannelLevelOutcome) -> Task<Message> {
    let ChannelLevelOutcome {
        channel,
        level,
        result,
    } = outcome;
    let Some(strip) = channel_strip(app, channel) else {
        return Task::none();
    };
    if !strip.level.is_in_flight(level) {
        return Task::none();
    }

    match &result {
        Ok(()) => tracing::info!(channel, level, "Channel level sent"),
        Err(error) => tracing::warn!(channel, level, %error, "Channel level send failed"),
    }

    let next = strip.level.finish(level, result);
    match next {
        Some(next_level) => {
            if let Some(device) = selected_device(app) {
                channel_level_task(device, channel, next_level)
            } else if let Some(strip) = channel_strip(app, channel) {
                strip.level.drop_pending();
                Task::none()
            } else {
                Task::none()
            }
        }
        None => Task::none(),
    }
}

/// Starts a channel-polarity send; ignored while one is in flight.
///
/// The USB work runs in the returned task, away from Iced's UI thread.
fn request_channel_polarity(app: &mut App, channel: u8, flipped: bool) -> Task<Message> {
    let Some(device) = selected_device(app) else {
        return Task::none();
    };
    let Some(strip) = channel_strip(app, channel) else {
        return Task::none();
    };

    match strip.polarity.request(flipped) {
        Some(send) => channel_polarity_task(device, channel, send),
        None => Task::none(),
    }
}

/// Records a channel-polarity result; stale completions are ignored.
fn finish_channel_polarity(app: &mut App, outcome: ChannelPolarityOutcome) -> Task<Message> {
    let ChannelPolarityOutcome {
        channel,
        flipped,
        result,
    } = outcome;
    let Some(strip) = channel_strip(app, channel) else {
        return Task::none();
    };
    if !strip.polarity.is_in_flight(flipped) {
        return Task::none();
    }

    match &result {
        Ok(()) => tracing::info!(channel, flipped, "Channel polarity sent"),
        Err(error) => tracing::warn!(channel, flipped, %error, "Channel polarity send failed"),
    }

    let _ = strip.polarity.finish(flipped, result);
    Task::none()
}

#[cfg(test)]
mod tests {
    use super::{route_control, update};
    use crate::app::message::{
        ChannelLevelOutcome, ChannelPolarityOutcome, DigitalOutputModeOutcome, FeedbackOutcome,
        Message, MonitorToggleOutcome, RoutingOutcome, SpeakerVolumeOutcome,
    };
    use crate::app::state::{
        App, DeviceStatus, fresh_meters_for, fresh_routes_for, selected_device, toggle_index,
    };
    use crate::app::subscription::{
        METER_CADENCE_MS, MONITOR_CADENCE_MS, feedback_cadence_ms, monitor_due,
    };
    use crate::app::tasks::{finish_scan, request_scan};
    use crate::device::{
        ControlInterface, ControlInterfaceKind, DetectedDevice, DeviceLocation, DiscoveryReport,
        FeedbackSnapshot, MonitorSnapshot, MonitorToggle,
    };
    use crate::monitor::{ToggleStatus, VolumeStatus};
    use crate::routing::{
        DigitalOutputMode, Route, RouteStatus, RoutingDestination, RoutingSource,
    };

    #[test]
    fn scan_requests_are_coalesced_while_a_scan_is_running() {
        let (mut app, _startup) = App::new();

        let _first_scan = request_scan(&mut app);
        let _queued_scan = request_scan(&mut app);
        let _duplicate_scan = request_scan(&mut app);

        assert!(app.scan_in_flight);
        assert!(app.rescan_requested);

        let _restart = finish_scan(&mut app);
        assert!(app.scan_in_flight);
        assert!(!app.rescan_requested);

        let _finished = finish_scan(&mut app);
        assert!(!app.scan_in_flight);
        assert!(!app.rescan_requested);
    }

    #[test]
    fn volume_change_without_a_device_is_ignored() {
        let (mut app, _startup) = App::new();

        let _ignored = update(&mut app, Message::SpeakerVolumeChanged(0.5));

        assert_level_eq(app.speaker.position(), 0.0);
        assert_eq!(app.speaker.status(), VolumeStatus::Unknown);
    }

    #[test]
    fn stale_completion_after_a_rescan_is_ignored() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _send = update(&mut app, Message::SpeakerVolumeChanged(0.2));
        let _rescan = update(
            &mut app,
            Message::DiscoveryFinished(Ok(DiscoveryReport::default())),
        );
        let _stale = update(
            &mut app,
            Message::SpeakerVolumeFinished(SpeakerVolumeOutcome {
                level: 0.2,
                result: Ok(()),
            }),
        );

        // The rescan reset the control; the late completion must not invent
        // an acknowledged send for the new (empty) device set.
        assert!(matches!(app.status, DeviceStatus::Empty));
        assert_eq!(app.speaker.status(), VolumeStatus::Unknown);
    }

    #[test]
    fn speaker_volume_moves_from_pending_to_applied() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _send = update(&mut app, Message::SpeakerVolumeChanged(0.2));
        assert_level_eq(app.speaker.position(), 0.2);
        assert_eq!(
            app.speaker.status(),
            VolumeStatus::Sending {
                sending: 0.2,
                queued: None,
            }
        );

        let _done = update(
            &mut app,
            Message::SpeakerVolumeFinished(SpeakerVolumeOutcome {
                level: 0.2,
                result: Ok(()),
            }),
        );
        assert_eq!(app.speaker.status(), VolumeStatus::Sent { level: 0.2 });
    }

    #[test]
    fn speaker_volume_failure_keeps_last_sent_and_reports_the_error() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _first = update(&mut app, Message::SpeakerVolumeChanged(0.2));
        let _applied = update(
            &mut app,
            Message::SpeakerVolumeFinished(SpeakerVolumeOutcome {
                level: 0.2,
                result: Ok(()),
            }),
        );

        let _retry = update(&mut app, Message::SpeakerVolumeChanged(0.3));
        assert_eq!(
            app.speaker.status(),
            VolumeStatus::Sending {
                sending: 0.3,
                queued: None,
            }
        );
        let _failed = update(
            &mut app,
            Message::SpeakerVolumeFinished(SpeakerVolumeOutcome {
                level: 0.3,
                result: Err("no device".to_owned()),
            }),
        );

        // The failed level is not presented as confirmed: the previous
        // acknowledged send is preserved and the error stays visible.
        assert_level_eq(app.speaker.position(), 0.3);
        assert_eq!(
            app.speaker.status(),
            VolumeStatus::Failed {
                error: "no device".to_owned(),
                last_sent: Some(0.2),
            }
        );
    }

    #[test]
    fn routing_request_without_a_device_is_ignored() {
        let (mut app, _startup) = App::new();
        let route = headphone_route(RoutingSource::MainMix);
        let _ignored = update(&mut app, Message::RouteSelected(route));
        assert!(app.routes.is_empty());
    }

    #[test]
    fn routing_moves_from_unknown_through_sending_to_sent() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.routes = fresh_routes_for(&selected_device(&app).unwrap());
        let route = headphone_route(RoutingSource::MainMix);

        let _send = update(&mut app, Message::RouteSelected(route));
        assert_eq!(
            route_control(&mut app, route.destination)
                .unwrap()
                .control
                .status(),
            RouteStatus::Sending {
                source: route.source
            }
        );

        let _done = update(
            &mut app,
            Message::RoutingFinished(RoutingOutcome {
                route,
                result: Ok(()),
            }),
        );
        assert_eq!(
            route_control(&mut app, route.destination)
                .unwrap()
                .control
                .status(),
            RouteStatus::Sent {
                source: route.source
            }
        );
    }

    #[test]
    fn reselecting_the_current_source_is_a_no_op_unless_it_failed() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.routes = fresh_routes_for(&selected_device(&app).unwrap());
        let route = headphone_route(RoutingSource::MainMix);

        let _send = update(&mut app, Message::RouteSelected(route));
        let _done = update(
            &mut app,
            Message::RoutingFinished(RoutingOutcome {
                route,
                result: Ok(()),
            }),
        );

        // Dropdown re-selection of the accepted value sends nothing new.
        let _reselect = update(&mut app, Message::RouteSelected(route));
        assert_eq!(
            route_control(&mut app, route.destination)
                .unwrap()
                .control
                .status(),
            RouteStatus::Sent {
                source: route.source
            }
        );

        // But the same choice after a failure is an explicit retry.
        let _failed = update(
            &mut app,
            Message::RouteSelected(headphone_route(RoutingSource::CueB)),
        );
        let _failed_done = update(
            &mut app,
            Message::RoutingFinished(RoutingOutcome {
                route: headphone_route(RoutingSource::CueB),
                result: Err("no device".to_owned()),
            }),
        );
        let _retry = update(
            &mut app,
            Message::RouteSelected(headphone_route(RoutingSource::CueB)),
        );
        assert_eq!(
            route_control(&mut app, route.destination)
                .unwrap()
                .control
                .status(),
            RouteStatus::Sending {
                source: RoutingSource::CueB
            }
        );
    }

    #[test]
    fn routing_failure_reports_the_error() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.routes = fresh_routes_for(&selected_device(&app).unwrap());
        let route = headphone_route(RoutingSource::CueB);

        let _send = update(&mut app, Message::RouteSelected(route));
        let _failed = update(
            &mut app,
            Message::RoutingFinished(RoutingOutcome {
                route,
                result: Err("no device".to_owned()),
            }),
        );
        assert_eq!(
            route_control(&mut app, route.destination)
                .unwrap()
                .control
                .status(),
            RouteStatus::Failed {
                error: "no device".to_owned(),
                last_sent: None,
            }
        );
    }

    #[test]
    fn reset_sends_the_documented_default() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.routes = fresh_routes_for(&selected_device(&app).unwrap());
        let destination = RoutingDestination::Outputs3And4;

        let _send = update(&mut app, Message::RouteReset(destination));
        assert_eq!(
            route_control(&mut app, destination)
                .unwrap()
                .control
                .status(),
            RouteStatus::Sending {
                source: RoutingSource::DawMix
            }
        );
    }

    #[test]
    fn digital_output_mode_is_pending_then_sent_without_claiming_readback() {
        let (mut app, _startup) = App::new();
        let mut report = report_with_control();
        report.supported[0].model = crate::device::supported_device(0x000d).unwrap();
        app.status = DeviceStatus::Ready(report);

        let _send = update(
            &mut app,
            Message::DigitalOutputModeSelected(DigitalOutputMode::Adat),
        );
        assert_eq!(
            app.digital_output_mode.status(),
            ToggleStatus::Sending { sending: true }
        );

        let _done = update(
            &mut app,
            Message::DigitalOutputModeFinished(DigitalOutputModeOutcome {
                mode: DigitalOutputMode::Adat,
                result: Ok(()),
            }),
        );
        assert_eq!(
            app.digital_output_mode.status(),
            ToggleStatus::Sent { on: true }
        );
    }

    #[test]
    fn monitor_toggle_moves_from_unknown_through_sending_to_sent() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _send = update(
            &mut app,
            Message::MonitorToggleChanged {
                toggle: MonitorToggle::Dim,
                on: true,
            },
        );
        assert_eq!(
            app.toggles[toggle_index(MonitorToggle::Dim)].status(),
            ToggleStatus::Sending { sending: true }
        );

        let _done = update(
            &mut app,
            Message::MonitorToggleFinished(MonitorToggleOutcome {
                toggle: MonitorToggle::Dim,
                on: true,
                result: Ok(()),
            }),
        );
        assert_eq!(
            app.toggles[toggle_index(MonitorToggle::Dim)].status(),
            ToggleStatus::Sent { on: true }
        );
    }

    fn assert_level_eq(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < f32::EPSILON,
            "expected slider level {expected}, got {actual}"
        );
    }

    #[test]
    fn channels_are_rebuilt_from_the_discovered_model() {
        use crate::monitor::{ToggleStatus, VolumeStatus};

        let (mut app, _startup) = App::new();
        assert!(app.channels.is_empty());

        let _ready = update(
            &mut app,
            Message::DiscoveryFinished(Ok(report_with_control())),
        );
        // iD14 MKII: 2 microphones + 8 digital inputs.
        assert_eq!(app.channels.len(), 10);
        assert_eq!(app.channels[0].level.status(), VolumeStatus::Unknown);
        assert_eq!(app.channels[0].polarity.status(), ToggleStatus::Unknown);
    }

    #[test]
    fn channel_level_moves_from_pending_to_applied() {
        use crate::monitor::VolumeStatus;

        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.channels = fresh_ready_channels();

        let _send = update(
            &mut app,
            Message::ChannelLevelChanged {
                channel: 2,
                level: 0.6,
            },
        );
        assert_level_eq(app.channels[2].level.position(), 0.6);
        assert_eq!(
            app.channels[2].level.status(),
            VolumeStatus::Sending {
                sending: 0.6,
                queued: None,
            }
        );

        let _done = update(
            &mut app,
            Message::ChannelLevelFinished(ChannelLevelOutcome {
                channel: 2,
                level: 0.6,
                result: Ok(()),
            }),
        );
        assert_eq!(
            app.channels[2].level.status(),
            VolumeStatus::Sent { level: 0.6 }
        );
    }

    #[test]
    fn channel_polarity_moves_from_pending_to_applied() {
        use crate::monitor::ToggleStatus;

        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.channels = fresh_ready_channels();

        let _send = update(
            &mut app,
            Message::ChannelPolarityChanged {
                channel: 1,
                flipped: true,
            },
        );
        assert_eq!(
            app.channels[1].polarity.status(),
            ToggleStatus::Sending { sending: true }
        );

        let _done = update(
            &mut app,
            Message::ChannelPolarityFinished(ChannelPolarityOutcome {
                channel: 1,
                flipped: true,
                result: Ok(()),
            }),
        );
        assert_eq!(
            app.channels[1].polarity.status(),
            ToggleStatus::Sent { on: true }
        );
    }

    fn fresh_ready_channels() -> Vec<crate::mixer::ChannelStrip> {
        std::iter::repeat_with(crate::mixer::ChannelStrip::default)
            .take(10)
            .collect()
    }

    #[test]
    fn feedback_cadence_needs_a_supported_device_with_a_control_interface() {
        let (mut app, _startup) = App::new();
        assert_eq!(feedback_cadence_ms(&app), None);

        app.status = DeviceStatus::Empty;
        assert_eq!(feedback_cadence_ms(&app), None);

        // Ready but no safe control interface: nothing may be polled.
        let mut report = report_with_control();
        report.supported[0].control_interface = None;
        app.status = DeviceStatus::Ready(report);
        assert_eq!(feedback_cadence_ms(&app), None);
    }

    #[test]
    fn feedback_cadence_prefers_meters_then_monitor_readback() {
        let (mut app, _startup) = App::new();

        // iD14 MKII: strips and meters share one gate.
        app.status = DeviceStatus::Ready(report_with_control());
        assert_eq!(feedback_cadence_ms(&app), Some(METER_CADENCE_MS));

        // iD4: monitor volumes read back, but no mixer means no meters.
        let mut report = report_with_control();
        report.supported[0].model = crate::device::supported_device(0x0003).unwrap();
        app.status = DeviceStatus::Ready(report);
        assert_eq!(feedback_cadence_ms(&app), Some(MONITOR_CADENCE_MS));
    }

    #[test]
    fn monitor_refresh_folds_into_the_meter_cadence() {
        assert!(monitor_due(10, METER_CADENCE_MS));
        assert!(monitor_due(20, METER_CADENCE_MS));
        assert!(!monitor_due(5, METER_CADENCE_MS));
        assert!(!monitor_due(11, METER_CADENCE_MS));
        // On the slow cadence every tick carries the monitor snapshot.
        assert!(monitor_due(1, MONITOR_CADENCE_MS));
        assert!(monitor_due(7, MONITOR_CADENCE_MS));
    }

    #[test]
    fn feedback_tick_is_dropped_while_a_poll_owns_the_session() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.feedback_in_flight = true;

        let _dropped = update(&mut app, Message::FeedbackTick);
        assert!(app.feedback_in_flight);
        assert_eq!(app.feedback_tick, 0);
    }

    #[test]
    fn connect_refresh_adopts_hardware_state_as_confirmed() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.channels = fresh_ready_channels();
        app.meters = fresh_meters_for(&selected_device(&app).unwrap());
        assert_eq!(app.speaker.status(), VolumeStatus::Unknown);

        let _done = update(
            &mut app,
            Message::FeedbackFinished(feedback_ok(
                MonitorSnapshot {
                    speaker_level: Some(0.7),
                    toggles: vec![(MonitorToggle::Dim, true)],
                    digital_output_mode: None,
                },
                Some(vec![0x40; 10]),
            )),
        );

        // Hardware wins: the slider moves to the device value and the
        // status names it confirmed, not sent.
        assert_level_eq(app.speaker.position(), 0.7);
        assert_eq!(app.speaker.status(), VolumeStatus::Confirmed { level: 0.7 });
        assert_eq!(
            app.toggles[toggle_index(MonitorToggle::Dim)].status(),
            ToggleStatus::Confirmed { on: true }
        );
        assert_eq!(app.meters, vec![Some(0x40); 10]);
        assert!(app.feedback_notice.is_none());
        assert!(!app.feedback_in_flight);
    }

    #[test]
    fn pending_send_is_not_overwritten_by_readback() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _send = update(&mut app, Message::SpeakerVolumeChanged(0.2));
        let _refresh = update(
            &mut app,
            Message::FeedbackFinished(feedback_ok(
                MonitorSnapshot {
                    speaker_level: Some(0.9),
                    toggles: Vec::new(),
                    digital_output_mode: None,
                },
                None,
            )),
        );

        // The local operation wins transiently; the hardware value is not
        // adopted until the send completes and the next poll confirms it.
        assert_level_eq(app.speaker.position(), 0.2);
        assert_eq!(
            app.speaker.status(),
            VolumeStatus::Sending {
                sending: 0.2,
                queued: None,
            }
        );

        let _done = update(
            &mut app,
            Message::SpeakerVolumeFinished(SpeakerVolumeOutcome {
                level: 0.2,
                result: Ok(()),
            }),
        );
        assert_eq!(app.speaker.status(), VolumeStatus::Sent { level: 0.2 });
    }

    #[test]
    fn failed_poll_blanks_meters_but_keeps_confirmed_monitor() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.channels = fresh_ready_channels();
        app.meters = fresh_meters_for(&selected_device(&app).unwrap());

        let _confirmed = update(
            &mut app,
            Message::FeedbackFinished(feedback_ok(
                MonitorSnapshot {
                    speaker_level: Some(0.5),
                    toggles: Vec::new(),
                    digital_output_mode: None,
                },
                Some(vec![0x20; 10]),
            )),
        );
        assert_eq!(app.speaker.status(), VolumeStatus::Confirmed { level: 0.5 });

        // A frozen meter presented as live would be a lie: meters go back
        // to unknown while the last confirmed monitor value stands.
        let _failed = update(
            &mut app,
            Message::FeedbackFinished(FeedbackOutcome {
                location: report_location(),
                read_monitor: false,
                result: Err("stalled".to_owned()),
            }),
        );
        assert!(app.meters.iter().all(Option::is_none));
        assert_eq!(app.speaker.status(), VolumeStatus::Confirmed { level: 0.5 });
        assert_eq!(app.feedback_notice.as_deref(), Some("stalled"));
    }

    #[test]
    fn stale_feedback_from_a_replaced_device_is_ignored() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.feedback_in_flight = true;

        let _stale = update(
            &mut app,
            Message::FeedbackFinished(FeedbackOutcome {
                location: DeviceLocation {
                    bus: "9".to_owned(),
                    address: 9,
                },
                read_monitor: true,
                result: Ok(FeedbackSnapshot {
                    monitor: Some(MonitorSnapshot {
                        speaker_level: Some(0.1),
                        toggles: Vec::new(),
                        digital_output_mode: None,
                    }),
                    meters: None,
                }),
            }),
        );

        // Neither adopted nor cleared: the newer poll still owns the flag.
        assert_eq!(app.speaker.status(), VolumeStatus::Unknown);
        assert!(app.feedback_in_flight);
    }

    #[test]
    fn reconnect_resets_confirmed_state_and_meters_to_unknown() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.channels = fresh_ready_channels();
        app.meters = fresh_meters_for(&selected_device(&app).unwrap());

        let _confirmed = update(
            &mut app,
            Message::FeedbackFinished(feedback_ok(
                MonitorSnapshot {
                    speaker_level: Some(0.6),
                    toggles: Vec::new(),
                    digital_output_mode: None,
                },
                Some(vec![0x10; 10]),
            )),
        );
        assert_eq!(app.speaker.status(), VolumeStatus::Confirmed { level: 0.6 });

        // Disconnect drops everything: old values are never restored as
        // confirmed before readback succeeds again.
        let _gone = update(
            &mut app,
            Message::DiscoveryFinished(Ok(DiscoveryReport::default())),
        );
        assert!(matches!(app.status, DeviceStatus::Empty));
        assert_eq!(app.speaker.status(), VolumeStatus::Unknown);
        assert!(app.meters.is_empty());
        assert!(app.feedback_notice.is_none());
        assert!(!app.feedback_in_flight);
    }

    #[test]
    fn second_device_can_be_selected_explicitly() {
        let (mut app, _startup) = App::new();
        let mk = |addr: u8| DetectedDevice {
            location: DeviceLocation {
                bus: "3".to_owned(),
                address: addr,
            },
            model: crate::device::supported_device(0x0008).unwrap(),
            reported_name: None,
            control_interface: Some(ControlInterface {
                number: 4,
                kind: ControlInterfaceKind::ApplicationSpecific,
            }),
        };
        app.status = DeviceStatus::Ready(DiscoveryReport {
            supported: vec![mk(16), mk(17)],
            unsupported: vec![],
        });
        app.selected = None;
        let first = selected_device(&app).unwrap().location.address;
        assert_eq!(first, 16);
        let _pick = update(
            &mut app,
            Message::DeviceSelected(DeviceLocation {
                bus: "3".to_owned(),
                address: 17,
            }),
        );
        assert_eq!(selected_device(&app).unwrap().location.address, 17);
    }

    #[test]
    fn picked_model_drives_channel_and_meter_counts() {
        let (mut app, _startup) = App::new();
        let mk = |product_id: u16, address: u8| DetectedDevice {
            location: DeviceLocation {
                bus: "3".to_owned(),
                address,
            },
            model: crate::device::supported_device(product_id).unwrap(),
            reported_name: None,
            control_interface: Some(ControlInterface {
                number: 4,
                kind: ControlInterfaceKind::ApplicationSpecific,
            }),
        };
        let report = || DiscoveryReport {
            supported: vec![mk(0x0008, 16), mk(0x0003, 17)],
            unsupported: vec![],
        };
        let first_model = crate::device::supported_device(0x0008).unwrap();
        let second_model = crate::device::supported_device(0x0003).unwrap();
        let first_strips = crate::mixer::mixer_channel_count(first_model) as usize;
        let first_meters = crate::mixer::meter_channel_count(first_model) as usize;
        let second_strips = crate::mixer::mixer_channel_count(second_model) as usize;
        let second_meters = crate::mixer::meter_channel_count(second_model) as usize;
        assert_ne!(first_strips, second_strips);

        let _found = update(&mut app, Message::DiscoveryFinished(Ok(report())));
        assert_eq!(selected_device(&app).unwrap().location.address, 16);
        assert_eq!(app.channels.len(), first_strips);
        assert_eq!(app.meters.len(), first_meters);

        let _pick = update(
            &mut app,
            Message::DeviceSelected(DeviceLocation {
                bus: "3".to_owned(),
                address: 17,
            }),
        );
        assert_eq!(selected_device(&app).unwrap().location.address, 17);
        assert_eq!(app.channels.len(), second_strips);
        assert_eq!(app.meters.len(), second_meters);
    }

    fn feedback_ok(monitor: MonitorSnapshot, meters: Option<Vec<u8>>) -> FeedbackOutcome {
        FeedbackOutcome {
            location: report_location(),
            read_monitor: true,
            result: Ok(FeedbackSnapshot {
                monitor: Some(monitor),
                meters,
            }),
        }
    }

    fn report_location() -> DeviceLocation {
        DeviceLocation {
            bus: "1".to_owned(),
            address: 2,
        }
    }

    fn headphone_route(source: RoutingSource) -> Route {
        Route {
            destination: RoutingDestination::Headphones,
            source,
        }
    }

    fn report_with_control() -> DiscoveryReport {
        DiscoveryReport {
            supported: vec![DetectedDevice {
                location: DeviceLocation {
                    bus: "1".to_owned(),
                    address: 2,
                },
                model: crate::device::supported_device(0x0008).unwrap(),
                reported_name: None,
                control_interface: Some(ControlInterface {
                    number: 4,
                    kind: ControlInterfaceKind::ApplicationSpecific,
                }),
            }],
            unsupported: vec![],
        }
    }
}
