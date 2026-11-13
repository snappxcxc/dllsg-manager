use std::path::Path;
use std::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn hidden(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    Sm75,
    Sm86,
    Sm89,
    Sm120,
    Unknown,
}

impl Arch {
    pub fn label(self) -> &'static str {
        match self {
            Arch::Sm75 => "Turing (SM75)",
            Arch::Sm86 => "Ampere (SM86)",
            Arch::Sm89 => "Ada (SM89)",
            Arch::Sm120 => "Blackwell (SM120)",
            Arch::Unknown => crate::i18n::t("неизвестно"),
        }
    }

    pub fn is_supported(self) -> bool {
        self != Arch::Unknown
    }
}

#[derive(Debug, Clone)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: String,
    pub driver_raw: String,
    /// Marketing driver version, e.g. 616.86 -> (616, 86)
    pub driver: Option<(u32, u32)>,
    pub arch: Arch,
    pub is_nvidia: bool,
}

impl Default for GpuInfo {
    fn default() -> Self {
        Self {
            name: crate::i18n::t("не определено").into(),
            vendor: String::new(),
            driver_raw: String::new(),
            driver: None,
            arch: Arch::Unknown,
            is_nvidia: false,
        }
    }
}

impl GpuInfo {
    pub fn driver_text(&self) -> String {
        match self.driver {
            Some((a, b)) => format!("{a}.{b:02}"),
            None => crate::i18n::t("неизвестно").to_string(),
        }
    }

    pub fn driver_ok(&self) -> bool {
        match self.driver {
            Some((major, _)) => major >= crate::model::DRIVER_FLOOR,
            None => false,
        }
    }
}

fn arch_from_name(name: &str) -> Arch {
    let n = name.to_ascii_uppercase();
    let is_rtx = n.contains("RTX") || n.contains("GEFORCE");
    if !is_rtx {
        return Arch::Unknown;
    }
    if n.contains("RTX 50") || n.contains("B100") || n.contains("B200") {
        Arch::Sm120
    } else if n.contains("RTX 40") {
        Arch::Sm89
    } else if n.contains("RTX 30") {
        Arch::Sm86
    } else if n.contains("RTX 20") || n.contains("TITAN RTX") {
        Arch::Sm75
    } else if n.contains("GTX 16") || n.contains("GTX 10") {
        Arch::Unknown
    } else {
        Arch::Unknown
    }
}

/// Convert a Windows driver version like 32.0.16.1686 into NVIDIA's 616.86.
pub fn marketing_driver(raw: &str) -> Option<(u32, u32)> {
    let parts: Vec<u32> = raw
        .split('.')
        .filter_map(|p| p.trim().parse::<u32>().ok())
        .collect();
    if parts.len() < 4 {
        return None;
    }
    let driver = (parts[2] % 10) * 100 + parts[3] / 100;
    let minor = parts[3] % 100;
    Some((driver, minor))
}

/// Reads GPU name / driver through the display-adapter class registry key.
pub fn gpu_info() -> GpuInfo {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
        use winreg::RegKey;

        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let class = hklm.open_subkey_with_flags(
            r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}",
            KEY_READ,
        );
        let Ok(class) = class else {
            return GpuInfo::default();
        };

        let mut best: Option<GpuInfo> = None;
        for sub in class.enum_keys().flatten() {
            let Ok(key) = class.open_subkey_with_flags(&sub, KEY_READ) else {
                continue;
            };
            let name: String = key.get_value("DriverDesc").unwrap_or_default();
            let raw: String = key.get_value("DriverVersion").unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let nvidia = name.to_ascii_uppercase().contains("NVIDIA");
            let info = GpuInfo {
                arch: arch_from_name(&name),
                driver: marketing_driver(&raw),
                name: name.clone(),
                vendor: if nvidia { "NVIDIA".into() } else { String::new() },
                driver_raw: raw,
                is_nvidia: nvidia,
            };
            // Prefer a real NVIDIA RTX adapter over virtual displays.
            let better = match &best {
                None => true,
                Some(cur) => {
                    let score = |g: &GpuInfo| {
                        (g.is_nvidia as u32) * 10
                            + (g.arch != Arch::Unknown) as u32 * 5
                            + (g.name.to_ascii_uppercase().contains("RTX")) as u32
                    };
                    score(&info) > score(cur)
                }
            };
            if better {
                best = Some(info);
            }
        }
        best.unwrap_or_default()
    }
    #[cfg(not(windows))]
    {
        GpuInfo::default()
    }
}

/// Lower-cased names of all running processes, using tasklist (no extra deps).
pub fn running_processes() -> Vec<String> {
    let mut cmd = Command::new("tasklist");
    cmd.args(["/FO", "CSV", "/NH"]);
    let out = hidden(&mut cmd).output();
    let Ok(out) = out else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .filter_map(|line| {
            let first = line.find('"')?;
            let rest = &line[first + 1..];
            let end = rest.find('"')?;
            Some(rest[..end].to_ascii_lowercase())
        })
        .collect()
}

pub fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".dlssgm_write_test_{}", std::process::id()));
    match std::fs::write(&probe, b"x") {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// True when the process owning `dir` can write there without elevation;
/// used to explain "run as administrator" hints.
pub fn is_elevated() -> bool {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("net");
        cmd.arg("session");
        match hidden(&mut cmd).output() {
            Ok(out) => out.status.success(),
            Err(_) => false,
        }
    }
    #[cfg(not(windows))]
    {
        true
    }
}

pub fn windows_version() -> String {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
        use winreg::RegKey;
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        if let Ok(key) =
            hklm.open_subkey_with_flags(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", KEY_READ)
        {
            let product: String = key.get_value("ProductName").unwrap_or_default();
            let build: String = key.get_value("CurrentBuild").unwrap_or_default();
            let display: String = key.get_value("DisplayVersion").unwrap_or_default();
            if !product.is_empty() {
                return format!("{product} {display} (build {build})").trim().to_string();
            }
        }
        "Windows".into()
    }
    #[cfg(not(windows))]
    {
        crate::i18n::t("не Windows").into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_conversion() {
        assert_eq!(marketing_driver("32.0.16.1686"), Some((616, 86)));
        assert_eq!(marketing_driver("32.0.15.9186"), Some((591, 86)));
        assert_eq!(marketing_driver("31.0.15.3699"), Some((536, 99)));
        assert_eq!(marketing_driver("oops"), None);
    }

    #[test]
    fn arch_mapping() {
        assert_eq!(arch_from_name("NVIDIA GeForce RTX 3070 Ti"), Arch::Sm86);
        assert_eq!(arch_from_name("NVIDIA GeForce RTX 4090"), Arch::Sm89);
        assert_eq!(arch_from_name("NVIDIA GeForce RTX 5070"), Arch::Sm120);
        assert_eq!(arch_from_name("NVIDIA GeForce RTX 2080 Ti"), Arch::Sm75);
        assert_eq!(arch_from_name("Parsec Virtual Display Adapter"), Arch::Unknown);
    }
}
