use anyhow::{ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace crate")
        .to_path_buf()
}
pub fn default_install() -> PathBuf {
    std::env::var_os("VRISING_DATA")
        .map(PathBuf::from)
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .or_else(|| {
            std::fs::read(repo().join("target/launcher-config.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<String>(&b).ok())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| repo().parent().expect("installation parent").to_path_buf())
}
pub fn verify(install: &Path) -> Result<()> {
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("../../../reference.lock.json"))?;
    for row in reference["files"].as_array().context("reference files")? {
        let name = row["path"].as_str().context("reference path")?;
        let path = install.join(name);
        let mut file =
            File::open(&path).with_context(|| format!("Не найден файл {}", path.display()))?;
        ensure!(
            file.metadata()?.len() == row["size"].as_u64().context("reference size")?,
            "Размер {name} не соответствует целевой версии"
        );
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        ensure!(
            format!("{:x}", hash.finalize()) == row["sha256"].as_str().context("reference hash")?,
            "Хеш {name} не соответствует V Rising 1.1.13.0-r99712-b17"
        );
    }
    Ok(())
}
pub fn launch(install: &Path) -> Result<String> {
    verify(install)?;
    let root = repo();
    let script = root.join("tools/run_player_prototype.ps1");
    ensure!(
        script.is_file(),
        "Не найден скрипт запуска в исходном checkout"
    );
    let log = root.join("target/launcher-prototype.log");
    std::fs::create_dir_all(root.join("target"))?;
    let output = File::create(&log)?;
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(script)
        .arg("-Data")
        .arg(install.join("VRising_Data"))
        .current_dir(&root)
        .stdout(Stdio::from(output.try_clone()?))
        .stderr(Stdio::from(output));
    let layout = root.join("target/client-layout.json");
    if layout.is_file() {
        command.arg("-Layout").arg(layout);
    }
    if let Some(python) = std::env::var_os("RRISING_PYTHON") {
        command.arg("-Python").arg(python);
    }
    if let Some(user) = std::env::var_os("USERPROFILE") {
        let cargo = PathBuf::from(user).join(".cargo/bin");
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![cargo];
        paths.extend(std::env::split_paths(&path));
        command.env("PATH", std::env::join_paths(paths)?);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let status = command
        .spawn()
        .context("Не удалось запустить PowerShell")?
        .wait()?;
    ensure!(
        status.success(),
        "Запуск завершился с кодом {:?}. Журнал: {}",
        status.code(),
        log.display()
    );
    Ok(format!("Прототип закрыт. Журнал: {}", log.display()))
}

pub fn remember_install(path: &Path) -> Result<()> {
    std::fs::create_dir_all(repo().join("target"))?;
    std::fs::write(
        repo().join("target/launcher-config.json"),
        serde_json::to_vec(&path.to_string_lossy())?,
    )?;
    Ok(())
}
