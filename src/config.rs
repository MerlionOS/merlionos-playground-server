use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    /// HTTP server port
    pub port: u16,
    /// Max concurrent QEMU instances
    pub max_instances: usize,
    /// Session timeout in seconds
    pub session_timeout_secs: u64,
    /// Idle timeout in seconds (no input)
    pub idle_timeout_secs: u64,
    /// Path to QEMU binary
    pub qemu_binary: String,
    /// Path to MerlionOS bootimage
    pub kernel_image: String,
    /// QEMU memory allocation per instance
    pub qemu_memory: String,
    /// Allowed CORS origin
    pub cors_origin: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            port: env_or("PORT", "3020").parse().unwrap_or(3020),
            max_instances: env_or("MAX_INSTANCES", "5").parse().unwrap_or(5),
            session_timeout_secs: env_or("SESSION_TIMEOUT", "600").parse().unwrap_or(600),
            idle_timeout_secs: env_or("IDLE_TIMEOUT", "120").parse().unwrap_or(120),
            qemu_binary: env_or("QEMU_BINARY", "qemu-system-x86_64"),
            kernel_image: env_or("KERNEL_IMAGE", "./images/merlionos.bin"),
            qemu_memory: env_or("QEMU_MEMORY", "128M"),
            cors_origin: env_or("CORS_ORIGIN", "*"),
        }
    }
}

fn env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}
