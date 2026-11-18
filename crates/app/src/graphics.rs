//! Select the native Skia Vulkan renderer for all application windows.

pub(crate) use i_slint_renderer_skia::vulkan_diagnostics::{DeviceInfo, FrameEvent, set_observer};

pub(crate) fn report(info: &DeviceInfo) -> String {
    format!(
        "backend=Vulkan renderer=Skia adapter={} type={} vendor={:04x} device={:04x} driver={}",
        info.name, info.device_type, info.vendor_id, info.device_id, info.driver_version
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Renderer;

impl Renderer {
    pub(crate) fn from_arguments(
        arguments: impl IntoIterator<Item = String>,
    ) -> Result<Self, String> {
        for argument in arguments {
            if let Some(value) = argument.strip_prefix("--renderer=")
                && value != "vulkan"
            {
                return Err(format!("unknown renderer: {value}; this build uses Vulkan"));
            }
        }
        Ok(Self)
    }

    pub(crate) fn select(self) -> Result<(), slint::PlatformError> {
        slint::BackendSelector::new()
            .backend_name("winit".into())
            .renderer_name("skia".into())
            .require_vulkan()
            .select()
    }

    pub(crate) const fn label(self) -> &'static str {
        "Vulkan · Skia"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vulkan_is_the_only_supported_renderer() {
        assert_eq!(
            Renderer::from_arguments([]).unwrap().label(),
            "Vulkan · Skia"
        );
        assert!(Renderer::from_arguments(["--renderer=vulkan".into()]).is_ok());
        assert!(Renderer::from_arguments(["--renderer=opengl".into()]).is_err());
        assert!(Renderer::from_arguments(["--renderer=typo".into()]).is_err());
    }
}
