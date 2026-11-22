//! Pure mapping from an Fn event to what the overlay should say.
//!
//! Nothing here touches hardware, the transport or the window: it turns an
//! already-decoded event into the text, colour and glyph that describe it, so
//! the mapping can be tested without a device.

#![forbid(unsafe_code)]
use himari_protocol::{FanNotice, FnEvent, PlatformCapabilities, PowerMode};

use crate::localization::tr;
use crate::osd_glyph::Glyph;

pub(crate) const ACCENT_INFO: slint::Color = slint::Color::from_rgb_u8(66, 211, 255);
pub(crate) const ACCENT_ERROR: slint::Color = slint::Color::from_rgb_u8(255, 91, 104);

pub(crate) struct OsdView {
    pub(crate) title: String,
    pub(crate) detail: String,
    pub(crate) accent: slint::Color,
    pub(crate) glyph: Glyph,
    /// Level shown as a five-step bar, negative to hide it.
    pub(crate) level: f32,
    /// Keep the glyph turning, used while anti-dust is running.
    pub(crate) spin: bool,
}

impl OsdView {
    pub(crate) fn new(title: String, detail: String, glyph: Glyph) -> Self {
        Self {
            title,
            detail,
            accent: ACCENT_INFO,
            glyph,
            level: -1.0,
            spin: false,
        }
    }

    pub(crate) fn with_level(mut self, level: f32) -> Self {
        self.level = level;
        self
    }

    pub(crate) fn spinning(mut self) -> Self {
        self.spin = true;
        self
    }
}

pub(crate) fn on_off(enabled: bool) -> String {
    if enabled { tr("On") } else { tr("Off") }.to_owned()
}

/// Percentage of the keyboard backlight range. The transport exposes five
/// levels (0 means off) and the vendor OSD shows the same five stops.
pub(crate) fn backlight_text(level: u8) -> String {
    if level == 0 {
        tr("Off").to_owned()
    } else {
        format!("{} {level}/4", tr("Brightness"))
    }
}

/// State-only events: firmware already performed the change, so there is
/// nothing to execute and only the resulting condition is reported.
pub(crate) fn state_view(event: FnEvent, fan: Option<(u32, u32)>) -> Option<OsdView> {
    let view = match event {
        FnEvent::FanNotice(notice) => {
            let detail = match notice {
                FanNotice::AntiDustRunning => tr("Anti-dust running"),
                FanNotice::Turbo => tr("Turbo fan"),
                FanNotice::MaxQ => tr("MaxQ fan profile"),
                FanNotice::Custom => tr("Custom fan profile"),
                FanNotice::Maximum => tr("Maximum fan"),
            }
            .to_owned();
            // Both fans are reported together: the platform runs a CPU and a
            // GPU fan, and one number would hide which one changed.
            let detail = match fan {
                Some((cpu, gpu)) => format!("{detail} · CPU {cpu} / GPU {gpu}"),
                None => detail,
            };
            let view = OsdView::new(tr("Cooling").to_owned(), detail, Glyph::Fan);
            if notice == FanNotice::AntiDustRunning {
                view.spinning()
            } else {
                view
            }
        }
        FnEvent::SilentMode(enabled) => {
            OsdView::new(tr("Silent mode").to_owned(), on_off(enabled), Glyph::Moon)
        }
        FnEvent::MobileRadio(enabled) => {
            OsdView::new(tr("Mobile radio").to_owned(), on_off(enabled), Glyph::Waves)
        }
        FnEvent::CapsLock(enabled) => {
            OsdView::new(tr("Caps Lock").to_owned(), on_off(enabled), Glyph::Key)
        }
        FnEvent::NumLock(enabled) => {
            OsdView::new(tr("Num Lock").to_owned(), on_off(enabled), Glyph::Key)
        }
        FnEvent::ScrollLock(enabled) => {
            OsdView::new(tr("Scroll Lock").to_owned(), on_off(enabled), Glyph::Key)
        }
        FnEvent::FlexiAccess(enabled) => OsdView::new(
            tr("FlexiKey access").to_owned(),
            on_off(enabled),
            Glyph::Key,
        ),
        FnEvent::DiscreteGpuSwitch(enabled) => {
            OsdView::new(tr("Discrete GPU").to_owned(), on_off(enabled), Glyph::Bolt)
        }
        FnEvent::CameraMicrophone(enabled) => OsdView::new(
            tr("Camera and microphone").to_owned(),
            on_off(enabled),
            Glyph::Microphone,
        ),
        FnEvent::Docking(connected) => OsdView::new(
            tr("Docking").to_owned(),
            if connected {
                tr("Connected")
            } else {
                tr("Disconnected")
            }
            .to_owned(),
            Glyph::Dock,
        ),
        _ => return None,
    };
    Some(view)
}

/// Title used both for the normal OSD and the failure OSD.
pub(crate) fn event_title(event: FnEvent) -> &'static str {
    match event {
        FnEvent::BacklightStep(_) | FnEvent::BacklightToggle | FnEvent::BacklightCycle => {
            tr("Keyboard backlight")
        }
        FnEvent::FnLockToggle => tr("Fn Lock"),
        FnEvent::FanNotice(_) | FnEvent::NoiselessFanToggle => tr("Cooling"),
        FnEvent::PowerModeCycle => tr("Power mode"),
        FnEvent::ControlCenterToggle => tr("HIMARI"),
        FnEvent::Docking(_) => tr("Docking"),
        FnEvent::NumLock(_) => tr("Num Lock"),
        FnEvent::CapsLock(_) => tr("Caps Lock"),
        FnEvent::ScrollLock(_) => tr("Scroll Lock"),
        FnEvent::SilentMode(_) => tr("Silent mode"),
        FnEvent::MobileRadio(_) => tr("Mobile radio"),
        FnEvent::Camera(_) => tr("Camera"),
        FnEvent::CameraMicrophone(_) => tr("Camera and microphone"),
        FnEvent::TouchpadToggle { .. } => tr("Touchpad"),
        FnEvent::ComboSwap(_) => tr("Win / Fn swap"),
        FnEvent::FlexiAccess(_) => tr("FlexiKey access"),
        FnEvent::DiscreteGpuSwitch(_) => tr("Discrete GPU"),
        FnEvent::GpuTurbo => tr("GPU Turbo"),
    }
}

/// Execute one Fn action. `Ok(None)` means the event has no user-visible
/// result, so nothing is presented.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PowerTier {
    /// A platform power profile, applied through the DCHU power packet.
    Profile(PowerMode),
    /// Quiet profile with the discrete GPU powered down. The vendor console
    /// calls this out as a separate tier because it is the only one that
    /// changes what the discrete GPU consumes.
    QuietWithoutDiscreteGpu,
}

impl PowerTier {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Profile(mode) => crate::localization::localized_power_label(mode),
            Self::QuietWithoutDiscreteGpu => tr("Quiet + discrete GPU off"),
        }
    }

    pub(crate) const fn profile(self) -> PowerMode {
        match self {
            Self::Profile(mode) => mode,
            Self::QuietWithoutDiscreteGpu => PowerMode::Quiet,
        }
    }

    /// Glyph that identifies this tier on the overlay.
    pub(crate) const fn glyph(self) -> Glyph {
        match self {
            Self::QuietWithoutDiscreteGpu => Glyph::Moon,
            Self::Profile(mode) => match mode {
                PowerMode::Performance => Glyph::Bolt,
                PowerMode::Entertainment => Glyph::Film,
                PowerMode::PowerSaving => Glyph::Leaf,
                PowerMode::Quiet | PowerMode::QuietPlus => Glyph::Moon,
                _ => Glyph::Gauge,
            },
        }
    }

    pub(crate) const fn powers_down_discrete_gpu(self) -> bool {
        matches!(self, Self::QuietWithoutDiscreteGpu)
    }
}

/// Stops the Fn key cycles through, gated by what the platform reports.
pub(crate) fn power_tiers(capabilities: &PlatformCapabilities) -> Vec<PowerTier> {
    let mut tiers = Vec::new();
    for mode in [
        PowerMode::Performance,
        PowerMode::Entertainment,
        PowerMode::PowerSaving,
        PowerMode::Quiet,
    ] {
        if capabilities.supports_power_mode(mode) {
            tiers.push(PowerTier::Profile(mode));
        }
    }
    // The GPU tier needs no profile support: it keeps the quiet profile when
    // the platform offers one and otherwise stays on whatever profile is
    // active, but it always powers the discrete GPU down.
    if capabilities.nvidia_power_off && !tiers.iter().any(|tier| tier.powers_down_discrete_gpu()) {
        tiers.push(PowerTier::QuietWithoutDiscreteGpu);
    }
    tiers
}

#[cfg(test)]
mod tier_tests {
    use super::*;
    use himari_protocol::PlatformCapabilities;

    /// The GPU tier must appear whenever the platform can power the discrete
    /// GPU down, because it is the only tier that changes what the discrete GPU
    /// consumes.
    /// The four vendor power tiers carry four distinct glyphs; sharing one
    /// gauge was a real regression before this test existed.
    #[test]
    fn every_power_tier_keeps_its_own_glyph() {
        assert_eq!(
            PowerTier::Profile(PowerMode::Performance).glyph(),
            Glyph::Bolt
        );
        assert_eq!(
            PowerTier::Profile(PowerMode::Entertainment).glyph(),
            Glyph::Film
        );
        assert_eq!(
            PowerTier::Profile(PowerMode::PowerSaving).glyph(),
            Glyph::Leaf
        );
        assert_eq!(PowerTier::Profile(PowerMode::Quiet).glyph(), Glyph::Moon);
        assert_eq!(PowerTier::QuietWithoutDiscreteGpu.glyph(), Glyph::Moon);
        let glyphs = [
            PowerTier::Profile(PowerMode::Performance).glyph(),
            PowerTier::Profile(PowerMode::Entertainment).glyph(),
            PowerTier::Profile(PowerMode::PowerSaving).glyph(),
        ];
        assert_eq!(glyphs[0].id(), 4);
        assert_eq!(glyphs[1].id(), 15);
        assert_eq!(glyphs[2].id(), 16);
    }

    #[test]
    fn gpu_tier_is_offered_when_the_platform_supports_it() {
        let caps = PlatformCapabilities {
            performance: true,
            nvidia_power_off: true,
            ..Default::default()
        };
        let tiers = power_tiers(&caps);
        assert!(tiers.iter().any(|tier| tier.powers_down_discrete_gpu()));
    }

    /// Without the capability the cycle must not pretend the tier exists.
    #[test]
    fn gpu_tier_is_absent_without_the_capability() {
        let caps = PlatformCapabilities {
            performance: true,
            ..Default::default()
        };
        let tiers = power_tiers(&caps);
        assert!(!tiers.iter().any(|tier| tier.powers_down_discrete_gpu()));
        assert!(tiers.contains(&PowerTier::Profile(PowerMode::Performance)));
    }

    /// Quiet is only listed when the platform accepts it, but the GPU tier is
    /// listed independently of that flag.
    #[test]
    fn quiet_profile_and_gpu_tier_are_gated_separately() {
        let caps = PlatformCapabilities {
            power_saving: true,
            nvidia_power_off: true,
            ..Default::default()
        };
        let tiers = power_tiers(&caps);
        assert!(!tiers.contains(&PowerTier::Profile(PowerMode::Quiet)));
        assert!(tiers.iter().any(|tier| tier.powers_down_discrete_gpu()));
        assert_eq!(tiers.len(), 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use himari_protocol::Adjustment;

    /// Every decoded event must have a title, otherwise the failure OSD would
    /// present an empty first line.
    /// Every firmware state notification keeps the glyph that identifies its
    /// control; a shared fallback icon would make the card ambiguous.
    #[test]
    fn every_state_event_keeps_its_glyph() {
        use crate::osd_glyph::Glyph;
        let cases: [(FnEvent, Glyph); 12] = [
            (FnEvent::FanNotice(FanNotice::Turbo), Glyph::Fan),
            (FnEvent::FanNotice(FanNotice::AntiDustRunning), Glyph::Fan),
            (FnEvent::SilentMode(true), Glyph::Moon),
            (FnEvent::SilentMode(false), Glyph::Moon),
            (FnEvent::MobileRadio(true), Glyph::Waves),
            (FnEvent::CapsLock(true), Glyph::Key),
            (FnEvent::NumLock(true), Glyph::Key),
            (FnEvent::ScrollLock(true), Glyph::Key),
            (FnEvent::FlexiAccess(true), Glyph::Key),
            (FnEvent::DiscreteGpuSwitch(true), Glyph::Bolt),
            (FnEvent::CameraMicrophone(true), Glyph::Microphone),
            (FnEvent::Docking(true), Glyph::Dock),
        ];
        for (event, glyph) in cases {
            let view = state_view(event, None).expect("state event must present");
            assert_eq!(view.glyph, glyph, "glyph drifted for {event:?}");
        }
        // Anti-dust is the only state event whose glyph keeps turning.
        let dust = state_view(FnEvent::FanNotice(FanNotice::AntiDustRunning), None).unwrap();
        assert!(dust.spin);
        let turbo = state_view(FnEvent::FanNotice(FanNotice::Turbo), None).unwrap();
        assert!(!turbo.spin);
    }

    #[test]
    fn every_decoded_event_has_a_title() {
        let events = [
            FnEvent::BacklightStep(Adjustment::Down),
            FnEvent::BacklightStep(Adjustment::Up),
            FnEvent::BacklightToggle,
            FnEvent::BacklightCycle,
            FnEvent::FnLockToggle,
            FnEvent::FanNotice(FanNotice::AntiDustRunning),
            FnEvent::FanNotice(FanNotice::Turbo),
            FnEvent::FanNotice(FanNotice::MaxQ),
            FnEvent::FanNotice(FanNotice::Custom),
            FnEvent::FanNotice(FanNotice::Maximum),
            FnEvent::NoiselessFanToggle,
            FnEvent::PowerModeCycle,
            FnEvent::ControlCenterToggle,
            FnEvent::Docking(true),
            FnEvent::Docking(false),
            FnEvent::NumLock(true),
            FnEvent::NumLock(false),
            FnEvent::CapsLock(true),
            FnEvent::CapsLock(false),
            FnEvent::ScrollLock(true),
            FnEvent::ScrollLock(false),
            FnEvent::SilentMode(true),
            FnEvent::SilentMode(false),
            FnEvent::MobileRadio(true),
            FnEvent::MobileRadio(false),
            FnEvent::Camera(true),
            FnEvent::Camera(false),
            FnEvent::CameraMicrophone(true),
            FnEvent::CameraMicrophone(false),
            FnEvent::TouchpadToggle {
                legacy_enabled: true,
            },
            FnEvent::TouchpadToggle {
                legacy_enabled: false,
            },
        ];
        for event in events {
            assert!(!event_title(event).is_empty(), "missing title: {event:?}");
        }
    }

    /// Firmware-driven notifications must paint without touching hardware, and
    /// they must carry an explicit condition rather than a generic toggle.
    #[test]
    fn state_only_events_report_a_condition() {
        for event in [
            FnEvent::CapsLock(true),
            FnEvent::NumLock(false),
            FnEvent::ScrollLock(true),
            FnEvent::SilentMode(false),
            FnEvent::MobileRadio(true),
            FnEvent::Docking(true),
            FnEvent::Docking(false),
            FnEvent::FanNotice(FanNotice::Turbo),
        ] {
            let view = state_view(event, None).expect("state event must present");
            assert!(!view.detail.is_empty(), "missing detail: {event:?}");
            assert!(!view.title.is_empty(), "missing title: {event:?}");
        }
    }

    /// Action events are executed instead of being rendered from state.
    #[test]
    fn action_events_are_not_state_only() {
        for event in [
            FnEvent::BacklightToggle,
            FnEvent::BacklightCycle,
            FnEvent::FnLockToggle,
            FnEvent::PowerModeCycle,
            FnEvent::TouchpadToggle {
                legacy_enabled: true,
            },
        ] {
            assert!(
                state_view(event, None).is_none(),
                "unexpected state view: {event:?}"
            );
        }
    }

    #[test]
    fn backlight_text_covers_off_and_every_level() {
        assert_eq!(backlight_text(0), tr("Off"));
        for level in 1..=4 {
            assert_eq!(
                backlight_text(level),
                format!("{} {level}/4", tr("Brightness"))
            );
        }
    }
}
