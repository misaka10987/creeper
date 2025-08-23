use std::{
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};
use serde_inline_default::serde_inline_default;
use walkdir::WalkDir;

use stop::stop;

use crate::{InstConfig, VERSION, launch::LaunchOption};

#[serde_inline_default]
#[derive(Clone, Serialize, Deserialize)]
pub struct Java {
    /// Path to the java executable.
    pub path: PathBuf,
    /// Maximum memory allocated.
    pub memory: usize,
    /// Whether to add JVM optimization arguments.
    #[serde(rename = "vm-opt-args")]
    #[serde_inline_default(true)]
    pub vm_opt_args: bool,
    /// Additional flags passed to java runtime.
    #[serde(default)]
    pub flags: Vec<String>,
}

impl Java {
    pub fn cmd(&self) -> Command {
        let mut cmd = Command::new(&self.path);
        cmd.arg(format!("-Xmx{}", self.memory));
        if self.vm_opt_args {
            cmd.args([
                "-XX:+UnlockExperimentalVMOptions",
                "-XX:+UseG1GC",
                "-XX:G1NewSizePercent=20",
                "-XX:G1ReservePercent=20",
                "-XX:MaxGCPauseMillis=50",
                "-XX:G1HeapRegionSize=32m",
                "-XX:-UseAdaptiveSizePolicy",
                "-XX:-OmitStackTraceInFastThrow",
                "-XX:-DontCompileHugeMethods",
            ]);
        }
        cmd.args(self.flags.clone());
        cmd
    }
}

impl InstConfig {
    pub fn java_class_path(&self) -> Vec<String> {
        let lib = WalkDir::new(&self.mc.lib);
        lib.into_iter()
            .map(|e| e.unwrap_or_else(stop!()))
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "jar"))
            .map(|e| e.path().display().to_string())
            .collect()
    }

    pub fn launch(&self, dir: impl AsRef<Path>) -> Command {
        let dir = dir.as_ref();
        let mut java = self.java.cmd();
        java.env("INST_NAME", &self.name);
        java.env("INST_ID", &self.name);
        java.env("INST_DIR", dir);
        java.env("INST_MC_DIR", dir);
        java.env("INST_JAVA", &self.java.path);
        java.args(self.java_flags(dir));
        java.arg("net.minecraft.client.main.Main");
        java.args(self.game_flags(dir));
        java
    }

    pub fn java_flags(&self, dir: impl AsRef<Path>) -> Vec<String> {
        let dir = dir.as_ref();

        let mut flags = vec![];

        // common flags
        flags.extend([
            "-Dfile.encoding=UTF-8".into(),
            "-Dstdout.encoding=UTF-8".into(),
            "-Dstderr.encoding=UTF-8".into(),
            "-Duser.home=null".into(),
            "-Djava.rmi.server.useCodebaseOnly=true".into(),
            "-Dcom.sun.jndi.rmi.object.trustURLCodebase=false".into(),
            "-Dcom.sun.jndi.cosnaming.object.trustURLCodebase=false".into(),
            "-Dlog4j2.formatMsgNoLookups=true".into(),
        ]);

        flags.extend([
            format!(
                "-Dlog4j.configurationFile={}",
                dir.join("log4j2.xml").display()
            ),
            format!(
                "-Dminecraft.client.jar={}",
                dir.join("minecraft.jar").display()
            ),
        ]);

        // LWJGL path
        flags.push(format!("-Djava.library.path={}", self.mc.lwjgl.display()));

        // launcher identifiers for java runtime
        flags.extend([
            "-Dminecraft.launcher.brand=creeper".into(),
            format!("-Dminecraft.launcher.version={}", VERSION),
        ]);

        // class paths
        let mut cp = self
            .mc
            .java_libs
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>();
        cp.push(dir.join("minecraft.jar").display().to_string());
        flags.extend(["-cp".into(), cp.join(":")]);

        flags
    }

    pub fn game_flags(&self, dir: impl AsRef<Path>) -> Vec<String> {
        let dir = dir.as_ref();

        let mut flags = vec![];

        flags.extend(["--version".into(), self.name.clone()]);

        // launcher identifiers for game
        flags.extend(["--versionType".into(), format!("creeper {}", VERSION)]);

        // dirs
        flags.extend([
            "--gameDir".into(),
            dir.display().to_string(),
            "--assetsDir".into(),
            self.mc.asset.display().to_string(),
            "--assetIndex".into(),
            format!("{}.{}", self.mc.version.major, self.mc.version.minor),
        ]);

        // window size
        flags.extend([
            "--width".into(),
            format!("{}", self.mc.width),
            "--height".into(),
            format!("{}", self.mc.height),
        ]);

        flags.extend(self.user.game_flags());

        flags.extend(self.mc.game_flags.clone());

        flags
    }
}
