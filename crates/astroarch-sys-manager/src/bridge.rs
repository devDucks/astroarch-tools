//! CXX-Qt Bridge: exposes the status of system services and management
//! actions to QML under the name `ServiceManager`

use core::pin::Pin;
use cxx_qt::Threading;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]

        // === Status properties ===
        #[qproperty(QString, bluetooth_status, cxx_name = "bluetoothStatus")]
        #[qproperty(QString, vsftpd_status, cxx_name = "vsftpdStatus")]
        #[qproperty(QString, gpsd_status, cxx_name = "gpsdStatus")]
        #[qproperty(QString, rpi_status, cxx_name = "rpiStatus")]
        #[qproperty(bool, bluetooth_installed, cxx_name = "bluetoothInstalled")]
        #[qproperty(bool, vsftpd_installed, cxx_name = "vsftpdInstalled")]
        #[qproperty(bool, gpsd_installed, cxx_name = "gpsdInstalled")]

        type ServiceManager = super::ServiceManagerRust;

        #[qinvokable]
        fn start_monitoring(self: Pin<&mut ServiceManager>);

        #[qinvokable]
        fn toggle_service(self: Pin<&mut ServiceManager>, plugin_path: &QString, cmd_on: &QString, cmd_off: &QString, is_active: bool);

        #[qinvokable]
        fn remove_service(self: Pin<&mut ServiceManager>, plugin_path: &QString, cmd_remove: &QString);

        #[qinvokable]
        fn run_rpi_optimization(self: Pin<&mut ServiceManager>);
    }

    impl cxx_qt::Threading for ServiceManager {}
}

#[derive(Default)]
pub struct ServiceManagerRust {
    // === Status fields ===
    pub bluetooth_status: cxx_qt_lib::QString,
    pub vsftpd_status: cxx_qt_lib::QString,
    pub gpsd_status: cxx_qt_lib::QString,
    pub rpi_status: cxx_qt_lib::QString,
    pub bluetooth_installed: bool,
    pub vsftpd_installed: bool,
    pub gpsd_installed: bool,

}

impl qobject::ServiceManager {
    pub fn toggle_service(
        self: Pin<&mut Self>,
        plugin_path: &cxx_qt_lib::QString,
        cmd_on: &cxx_qt_lib::QString,
        cmd_off: &cxx_qt_lib::QString,
        is_active: bool,
    ) {
        let path = plugin_path.to_string();
        let cmd = if is_active { cmd_off.to_string() } else { cmd_on.to_string() };
        Self::run_zsh_command(path, cmd);
    }

    pub fn remove_service(
        self: Pin<&mut Self>,
        plugin_path: &cxx_qt_lib::QString,
        cmd_remove: &cxx_qt_lib::QString,
    ) {
        Self::run_zsh_command(plugin_path.to_string(), cmd_remove.to_string());
    }

    pub fn run_rpi_optimization(self: Pin<&mut Self>) {
        Self::run_zsh_command(
            "/home/astronaut/.astroarch/plugins/power_max_current/power_max_current.plugin.zsh".to_string(),
                              "power_max_current".to_string(),
        );
    }

    fn run_zsh_command(plugin_path: String, command: String) {
        std::thread::spawn(move || {
            let zsh_cmd = format!("source {} && {}", plugin_path, command);
            let _ = std::process::Command::new("konsole")
            .arg("-e")
            .arg("zsh")
            .arg("-c")
            .arg(zsh_cmd)
            .spawn();
        });
    }

    pub fn start_monitoring(self: Pin<&mut Self>) {
        let qt_thread = self.qt_thread();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                loop {
                    let bt = check_system_service("bluetooth.service").await;
                    let ftp = check_system_service("vsftpd.service").await;
                    let gps = check_system_service("gpsd.service").await;

                    let rpi_active = check_rpi_usb_max_current().await;
                    let rpi_status_str = if rpi_active { "active" } else { "inactive" };

                    // Installation Checks
                    let bt_installed = is_system_service_installed("bluetooth").await;
                    let ftp_installed = is_system_service_installed("vsftpd").await;
                    let gps_installed = is_system_service_installed("gpsd").await;

                    let bt_q = cxx_qt_lib::QString::from(bt);
                    let ftp_q = cxx_qt_lib::QString::from(ftp);
                    let gps_q = cxx_qt_lib::QString::from(gps);
                    let rpi_status_q = cxx_qt_lib::QString::from(rpi_status_str);

                    // The QObject is gone (its page closed): the monitoring ends
                    let sent = qt_thread.queue(move |mut qobject| {
                        qobject.as_mut().set_bluetooth_status(bt_q);
                        qobject.as_mut().set_vsftpd_status(ftp_q);
                        qobject.as_mut().set_gpsd_status(gps_q);
                        qobject.as_mut().set_rpi_status(rpi_status_q);

                        // Updating the “installed” properties
                        qobject.as_mut().set_bluetooth_installed(bt_installed);
                        qobject.as_mut().set_vsftpd_installed(ftp_installed);
                        qobject.as_mut().set_gpsd_installed(gps_installed);
                    });
                    if sent.is_err() {
                        break;
                    }

                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            });
        });
    }
}

// ====================== SYSTEM SERVICES ======================

async fn check_system_service(name: &str) -> String {
    let active = is_system_service_active(name).await;
    let enabled = is_system_service_enabled(name).await;

    if active { "active".to_string() }
    else if enabled { "enabled".to_string() }
    else { "inactive".to_string() }
}

async fn is_system_service_active(name: &str) -> bool {
    let output = tokio::process::Command::new("systemctl")
    .args(["is-active", "--quiet", name])
    .output()
    .await;
    output.map(|o| o.status.success()).unwrap_or(false)
}

async fn is_system_service_enabled(name: &str) -> bool {
    let output = tokio::process::Command::new("systemctl")
    .args(["is-enabled", "--quiet", name])
    .output()
    .await;
    output.map(|o| o.status.success()).unwrap_or(false)
}

// ====================== HARDWARE DETECTION ======================

async fn check_rpi_usb_max_current() -> bool {
    let output = tokio::process::Command::new("grep")
    .arg("-q")
    .arg("^usb_max_current_enable=1")
    .arg("/boot/config.txt")
    .output()
    .await;
    output.map(|o| o.status.success()).unwrap_or(false)
}

// ====================== DETECTION SYSTEM ======================

/// Check whether a system service is installed
async fn is_system_service_installed(service_name: &str) -> bool {
    let output = tokio::process::Command::new("systemctl")
    .args(["list-unit-files", &format!("{}.service", service_name)])
    .output()
    .await;

    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout.contains(service_name)
        },
        Err(_) => false,
    }
}

