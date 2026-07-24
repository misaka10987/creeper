use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail};
use chrono::Utc;
use creeper_maven_coord::MavenCoord;
use neoforge::install::Processor;
use strfmt::Format;
use tokio::{
    fs::{create_dir_all, metadata, remove_dir_all, try_exists, write},
    process::Command,
};
use tracing::{debug, error, info, instrument, trace};
use walkdir::WalkDir;

use crate::{
    Artifact, Creeper, jar::jar_main_class, neoforge::fmt::maven_coord_format,
    path::creeper_cache_dir,
};

pub struct InstallContainer {
    path: PathBuf,
    lib: Creeper,
    lib_file: HashMap<PathBuf, Artifact>,
    var: HashMap<String, String>,
}

impl InstallContainer {
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub async fn init(&self) -> anyhow::Result<()> {
        if try_exists(self.path()).await? {
            bail!(
                "cannot initialize install container on existing path {}",
                self.path().display()
            );
        }

        create_dir_all(self.path()).await?;

        Ok(())
    }

    pub async fn deinit(&self) -> anyhow::Result<()> {
        remove_dir_all(self.path()).await?;
        Ok(())
    }

    pub fn lib_dir(&self) -> PathBuf {
        self.path.join("libraries")
    }

    pub fn add_lib_file(&mut self, file: impl IntoIterator<Item = (PathBuf, Artifact)>) {
        self.lib_file.extend(file);
    }

    pub fn add_var(&mut self, var: impl IntoIterator<Item = (String, String)>) {
        self.var.extend(var);
    }

    pub async fn deploy_lib(&self) -> anyhow::Result<()> {
        self.lib
            .batch_retrieve_artifact_to(self.lib_file.clone(), self.lib_dir())
            .await?;

        Ok(())
    }

    #[instrument(skip(self, proc), fields(proc = proc.jar))]
    pub async fn run(&self, proc: &Processor) -> anyhow::Result<()> {
        info!("running in {}", self.path().display());

        let jar = proc.jar.parse::<MavenCoord>()?;

        let jar = self
            .lib_file
            .get(&jar.path())
            .ok_or(anyhow!("processor jar {jar} not found in container"))?;

        let jar = self.lib.retrieve_artifact(jar).await?;

        let main_class = jar_main_class(&jar).await?;

        let mut cp = vec![jar.display().to_string()];

        for c in &proc.classpath {
            let coord = c.parse::<MavenCoord>()?;

            let file = self.lib_file.get(&coord.path()).ok_or(anyhow!(
                "processor {proc} require classpath {coord}, which is not found in container"
            ))?;

            let file = self.lib.retrieve_artifact(file).await?;

            cp.push(file.display().to_string());
        }

        let mut cmd = Command::new("java");

        cmd.arg("--class-path").arg(cp.join(":"));
        cmd.arg(main_class);

        for arg in &proc.args {
            let arg = arg.format(&self.var)?;
            let arg = maven_coord_format(&arg, self.lib_dir())?;
            cmd.arg(arg);
        }

        debug!("running command {:?}", cmd.as_std());

        let output = cmd.output().await?;

        if !output.status.success() {
            error!("command failed");

            let now = Utc::now();

            let cmd = format!("{:?}", cmd.as_std());

            let hash = &blake3::hash(cmd.as_bytes()).to_hex()[..8];

            let name = format!("{}-{hash}", now.to_rfc3339());

            let path = creeper_cache_dir()?.join("log").join(name);

            create_dir_all(&path).await?;

            write(path.join("stdout.txt"), output.stdout).await?;
            write(path.join("stderr.txt"), output.stderr).await?;
            write(path.join("command.sh"), cmd).await?;

            error!("log saved to {}", path.display());

            if let Some(code) = output.status.code() {
                bail!("processor exited with error {code}: {proc}");
            }

            bail!("processor exited with error: {proc}");
        }

        Ok(())
    }

    pub async fn collect_lib_file(
        &self,
        exclude: impl IntoIterator<Item = &Path>,
    ) -> anyhow::Result<HashMap<PathBuf, Artifact>> {
        let exclude = self
            .lib_file
            .keys()
            .map(|k| k.as_path())
            .chain(exclude.into_iter().map(|k| k.as_ref()))
            .collect::<HashSet<_>>();

        let mut map = HashMap::new();

        for i in WalkDir::new(self.lib_dir()) {
            let entry = i?;

            let path = entry.path();

            let meta = metadata(path).await?;

            if meta.is_dir() {
                continue;
            }

            if meta.is_symlink() {
                trace!("skipping symlink {}", path.display());
                continue;
            }

            let relative = path.strip_prefix(self.lib_dir()).unwrap();

            if exclude.contains(relative) {
                trace!("excluding {}", path.display());
                continue;
            }

            trace!("found {}", path.display());

            let art = self.lib.store_artifact(path).await?;

            map.insert(relative.to_path_buf(), art);
        }

        Ok(map)
    }
}

impl Creeper {
    pub(crate) fn new_install_container(&self, path: PathBuf) -> InstallContainer {
        InstallContainer {
            path,
            lib: self.clone(),
            lib_file: HashMap::new(),
            var: HashMap::new(),
        }
    }
}
