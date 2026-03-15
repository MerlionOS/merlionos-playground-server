use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tracing::{info, error};

use crate::config::Config;

/// A running QEMU instance with serial I/O channels.
pub struct QemuInstance {
    child: Child,
    /// Send bytes to QEMU's serial stdin
    pub stdin_tx: mpsc::Sender<u8>,
    /// Receive bytes from QEMU's serial stdout
    pub stdout_rx: mpsc::Receiver<u8>,
}

impl QemuInstance {
    /// Spawn a new QEMU process with serial on stdio.
    pub async fn spawn(config: &Config) -> Result<Self, String> {
        let mut child = Command::new(&config.qemu_binary)
            .args([
                "-drive", &format!("format=raw,file={}", config.kernel_image),
                "-m", &config.qemu_memory,
                "-serial", "stdio",
                "-display", "none",
                "-no-reboot",
                "-no-shutdown",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("failed to spawn QEMU: {e}"))?;

        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;

        // Channel for sending bytes to QEMU stdin
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<u8>(1024);
        // Channel for receiving bytes from QEMU stdout
        let (stdout_tx, stdout_rx) = mpsc::channel::<u8>(4096);

        // Stdin forwarder
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(byte) = stdin_rx.recv().await {
                if stdin.write_all(&[byte]).await.is_err() {
                    break;
                }
                let _ = stdin.flush().await;
            }
        });

        // Stdout reader
        tokio::spawn(async move {
            let mut stdout = stdout;
            let mut buf = [0u8; 256];
            loop {
                match stdout.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        for &byte in &buf[..n] {
                            if stdout_tx.send(byte).await.is_err() {
                                return;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        info!("QEMU instance spawned");
        Ok(Self { child, stdin_tx, stdout_rx })
    }

    /// Kill the QEMU process.
    pub async fn kill(&mut self) {
        if let Err(e) = self.child.kill().await {
            error!("Failed to kill QEMU: {e}");
        }
        info!("QEMU instance killed");
    }

    /// Check if QEMU is still running.
    pub fn is_running(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }
}
