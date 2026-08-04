use anyhow::{bail, Result};
use std::ffi::OsString;
use std::process::Command;
use tracing::info;

pub const SERVICE_NAME: &str = "netvan-api";
pub const SERVICE_DISPLAY: &str = "Netvan API";
pub const DEFAULT_URL: &str = "http://127.0.0.1:8000";

pub fn install() -> Result<()> {
    let exe = std::env::current_exe()?;
    let bin = exe.display().to_string();
    let status = Command::new("sc")
        .args([
            "create",
            SERVICE_NAME,
            &format!("binPath= \"{bin}\" run"),
            "start= auto",
            &format!("DisplayName= {SERVICE_DISPLAY}"),
        ])
        .status()?;
    if !status.success() {
        bail!("sc create failed (may need elevation)");
    }
    let _ = Command::new("sc")
        .args([
            "description",
            SERVICE_NAME,
            "Local Netvan collectors + HTTP/WebSocket API for netvan-webui",
        ])
        .status();
    let _ = Command::new("sc")
        .args([
            "failure",
            SERVICE_NAME,
            "reset= 86400",
            "actions= restart/5000/restart/5000/restart/5000",
        ])
        .status();
    info!("installed {SERVICE_NAME}");
    println!("Installed Windows service '{SERVICE_NAME}' ({SERVICE_DISPLAY})");
    println!("Listening URL when running: {DEFAULT_URL}");
    Ok(())
}

pub fn uninstall() -> Result<()> {
    let _ = stop();
    let status = Command::new("sc").args(["delete", SERVICE_NAME]).status()?;
    if !status.success() {
        bail!("sc delete failed (may need elevation)");
    }
    info!("uninstalled {SERVICE_NAME}");
    println!("Uninstalled Windows service '{SERVICE_NAME}'");
    Ok(())
}

pub fn start() -> Result<()> {
    let status = Command::new("sc").args(["start", SERVICE_NAME]).status()?;
    if !status.success() {
        bail!("sc start failed (may need elevation)");
    }
    println!("Started '{SERVICE_NAME}' → {DEFAULT_URL}");
    Ok(())
}

pub fn stop() -> Result<()> {
    let status = Command::new("sc").args(["stop", SERVICE_NAME]).status()?;
    if !status.success() {
        // ignore if already stopped
    }
    println!("Stopped '{SERVICE_NAME}'");
    Ok(())
}

pub fn status() -> Result<()> {
    let output = Command::new("sc").args(["query", SERVICE_NAME]).output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let err = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        println!("Service: {SERVICE_NAME}");
        println!("Installed: no");
        println!("State: not installed");
        println!("URL: {DEFAULT_URL} (when running)");
        if !err.trim().is_empty() {
            println!("Detail: {}", err.trim());
        }
        return Ok(());
    }

    let state = text
        .lines()
        .find_map(|line| {
            let t = line.trim();
            if t.starts_with("STATE") {
                // e.g. STATE              : 4  RUNNING
                Some(
                    t.split(':')
                        .nth(1)
                        .unwrap_or("")
                        .split_whitespace()
                        .last()
                        .unwrap_or("unknown")
                        .to_string(),
                )
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".into());

    println!("Service: {SERVICE_NAME}");
    println!("Display: {SERVICE_DISPLAY}");
    println!("Installed: yes");
    println!("State: {state}");
    println!("URL: {DEFAULT_URL}");
    Ok(())
}

#[cfg(windows)]
#[allow(dead_code)]
pub fn run_as_service() -> Result<()> {
    use std::time::Duration;
    use windows_service::service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::{define_windows_service, service_dispatcher};

    define_windows_service!(ffi_service_main, service_main);

    fn service_main(_args: Vec<OsString>) {
        let event_handler = move |control| match control {
            ServiceControl::Stop | ServiceControl::Interrogate => {
                ServiceControlHandlerResult::NoError
            }
            _ => ServiceControlHandlerResult::NotImplemented,
        };
        let status_handle =
            service_control_handler::register(SERVICE_NAME, event_handler).expect("register");
        status_handle
            .set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: ServiceState::Running,
                controls_accepted: ServiceControlAccept::STOP,
                exit_code: ServiceExitCode::Win32(0),
                checkpoint: 0,
                wait_hint: Duration::default(),
                process_id: None,
            })
            .ok();
        let rt = tokio::runtime::Runtime::new().expect("rt");
        let _ = rt.block_on(crate::http_server::run());
    }

    service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
    Ok(())
}
