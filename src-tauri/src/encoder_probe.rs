//! Vulkan encoder probes use the same device argument as the final encode.
use anyhow::{Context, Result};
use ash::vk;
use std::{
    ffi::CStr,
    io::{Read, Seek, SeekFrom},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub(crate) const DEFAULT_DEVICE: &str = "vulkan=vk";

pub(crate) fn device_args(spec: &str) -> [&str; 4] {
    ["-init_hw_device", spec, "-filter_hw_device", "vk"]
}

/// FFmpeg supports selecting a Vulkan device by name. Prefer discrete devices
/// and do not report a software ICD as a hardware encoder.
pub(crate) fn prores_devices() -> Result<Vec<String>> {
    unsafe {
        let entry = ash::Entry::load().context("load Vulkan loader")?;
        let instance = entry
            .create_instance(&vk::InstanceCreateInfo::default(), None)
            .context("create Vulkan instance")?;
        let result = instance.enumerate_physical_devices().map(|devices| {
            let mut names: Vec<_> = devices
                .into_iter()
                .filter_map(|device| {
                    let props = instance.get_physical_device_properties(device);
                    let priority = match props.device_type {
                        vk::PhysicalDeviceType::DISCRETE_GPU => 0,
                        vk::PhysicalDeviceType::INTEGRATED_GPU => 1,
                        vk::PhysicalDeviceType::VIRTUAL_GPU => 2,
                        _ => return None,
                    };
                    let name = CStr::from_ptr(props.device_name.as_ptr()).to_string_lossy();
                    // A comma terminates the name in FFmpeg's device syntax.
                    let name = name.split(',').next().unwrap_or_default().trim();
                    (!name.is_empty() && name.parse::<u32>().is_err())
                        .then(|| (priority, format!("vulkan=vk:{name}")))
                })
                .collect();
            names.sort();
            names.dedup();
            names.into_iter().map(|(_, arg)| arg).collect()
        });
        instance.destroy_instance(None);
        result.context("enumerate Vulkan devices")
    }
}

/// Keep driver stalls from wedging the encoder dropdown. Capture stderr on
/// disk so FFmpeg can complete even when it emits more than a pipe buffer.
pub(crate) fn run(command: &mut Command, timeout: Duration) -> Result<()> {
    let mut log = tempfile::tempfile()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log.try_clone()?)
        .spawn()
        .context("launch FFmpeg encoder probe")?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(20)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                result?;
                break None;
            }
        }
    };
    log.seek(SeekFrom::Start(0))?;
    let mut details = Vec::new();
    log.take(16 * 1024).read_to_end(&mut details)?;
    let details = String::from_utf8_lossy(&details);
    let details = details.trim();
    match status {
        None => anyhow::bail!("probe timed out after {}s: {details}", timeout.as_secs()),
        Some(status) if !status.success() || !details.is_empty() => {
            anyhow::bail!("sample encode failed ({status}): {details}");
        }
        Some(_) => Ok(()),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn collects_ffmpeg_error_and_accepts_clean_success() {
        run(
            Command::new("sh").args(["-c", "exit 0"]),
            Duration::from_secs(1),
        )
        .unwrap();
        let err = run(
            Command::new("sh").args(["-c", "printf 'missing extension' >&2; exit 1"]),
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(err.to_string().contains("missing extension"));
    }

    #[test]
    fn rejects_driver_errors_and_stalled_probes() {
        let err = run(
            Command::new("sh").args(["-c", "printf 'device error' >&2"]),
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(err.to_string().contains("device error"));
        let err = run(
            &mut Command::new("sleep").arg("30"),
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(err.to_string().contains("timed out"));
    }
}
