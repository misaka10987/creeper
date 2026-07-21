use std::{collections::HashMap, iter::once, path::Path};

use crate::{
    Checksum, Creeper, Install, VERSION,
    util::skip_two,
    vanilla::{RuleChecker, java_module_path},
};

use mc_launchermeta::version as mc_version;
use serde::{Deserialize, Serialize};

/// The (extended) Minecraft launcher `version.json` metadata.
///
/// This is a superset of `mc_launchermeta::version::Version`,
/// and is made compatible with the format used by NeoForge.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct McVersionExt {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inherits_from: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<mc_version::Arguments>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub minecraft_arguments: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_index: Option<mc_version::AssetIndex>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub assets: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub compliance_level: Option<u8>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub downloads: Option<mc_version::Downloads>,

    pub id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub java_version: Option<mc_version::JavaVersion>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub libraries: Vec<mc_version::library::Library>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<mc_version::logging::Logging>,

    pub main_class: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_launcher_version: Option<u8>,

    pub release_time: String,

    pub time: String,

    #[serde(rename = "type")]
    pub kind: mc_launchermeta::VersionKind,
}

impl From<mc_version::Version> for McVersionExt {
    fn from(value: mc_version::Version) -> Self {
        Self {
            inherits_from: None,
            arguments: value.arguments,
            minecraft_arguments: value.minecraft_arguments,
            asset_index: Some(value.asset_index),
            assets: Some(value.assets),
            compliance_level: value.compliance_level,
            downloads: Some(value.downloads),
            id: value.id,
            java_version: value.java_version,
            libraries: value.libraries,
            logging: value.logging,
            main_class: value.main_class,
            minimum_launcher_version: Some(value.minimum_launcher_version),
            release_time: value.release_time,
            time: value.time,
            kind: value.kind,
        }
    }
}

impl Creeper {
    fn vanilla_args_install(&self, args: &mc_version::Arguments, version_name: &str) -> Install {
        let rule = RuleChecker::default();

        let version_type = format!("creeper {VERSION}");

        let vars = [
            ("version_name", version_name),
            ("game_directory", "."),
            ("version_type", &version_type),
            ("natives_directory", "./.creeper/native"),
            ("launcher_name", "creeper"),
            ("launcher_version", VERSION),
            ("library_directory", "./.creeper/lib"),
        ]
        .into_iter()
        .collect::<HashMap<_, _>>();

        let java_flag = args
            .jvm
            .iter()
            .filter_map(|a| a.rules.iter().all(rule.checker()).then_some(&a.values))
            .flatten();

        let java_flag = skip_two(
            |a| ["--class-path", "-cp", "--module-path", "-p"].contains(&a.as_str()),
            java_flag,
        );

        let java_flag = java_flag
            .iter()
            .map(|x| shellexpand::env_with_context_no_errors(x, |k| vars.get(k)).to_string())
            .collect();

        let mc_flag = args
            .game
            .iter()
            .filter_map(|a| a.rules.iter().all(rule.checker()).then_some(&a.values))
            .flatten();

        let mc_flag = skip_two(
            |a| {
                [
                    "--username",
                    "--assetsDir",
                    "--assetIndex",
                    "--uuid",
                    "--accessToken",
                    "--userType",
                ]
                .contains(&a.as_str())
            },
            mc_flag,
        );

        let mc_flag = mc_flag
            .iter()
            .map(|x| shellexpand::env_with_context_no_errors(x, |k| vars.get(k)).to_string())
            .collect();

        Install {
            java_flag,
            mc_flag,
            ..Default::default()
        }
    }

    pub async fn mc_version_install(&self, version: McVersionExt) -> anyhow::Result<Install> {
        let mut install = Install::default();

        let rule = RuleChecker::default();

        if let Some(downloads) = version.downloads {
            let client = self
                .download(
                    format!("{}.jar", version.id),
                    downloads.client.url,
                    Some(downloads.client.size),
                    once(Checksum::sha1(downloads.client.sha1)),
                )
                .await?;

            install.extend(once(Install {
                mc_jar: Some(client),
                ..Default::default()
            }));
        }

        let lib = self.vanilla_lib(version.libraries).await?;

        let java_args = version
            .arguments
            .iter()
            .flat_map(|x| &x.jvm)
            .filter_map(|a| a.rules.iter().all(rule.checker()).then_some(&a.values))
            .flatten();

        let mut java_lib_mod = HashMap::new();

        for p in java_module_path(java_args.map(|a| a.as_str()))? {
            let path = Path::new(p);

            if let Some(art) = lib.get(path) {
                java_lib_mod.insert(path.into(), art.clone());
            }
        }

        if let Some(asset_index) = version.asset_index {
            let asset_index = self.download_asset_index(asset_index).await?;

            let asset = self.vanilla_asset_install(asset_index).await?;

            install.extend(once(asset));
        }

        if let Some(args) = version.arguments {
            let arg = self.vanilla_args_install(&args, &version.id);

            install.extend(once(arg));
        }

        install.extend(once(Install {
            java_lib_class: lib,
            java_lib_mod,
            java_main_class: Some(version.main_class),
            ..Default::default()
        }));

        Ok(install)
    }
}
