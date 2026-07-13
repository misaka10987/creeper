use std::{
    collections::HashMap,
    env::consts::{ARCH, OS},
};

use mc_launchermeta::version::rule::{Os, Rule};

#[derive(Default)]
pub struct RuleChecker {
    feature: HashMap<String, bool>,
}

impl RuleChecker {
    pub fn checker(&self) -> impl Fn(&Rule) -> bool {
        move |rule| self.check(rule)
    }

    pub fn check(&self, rule: &Rule) -> bool {
        let os = rule.os.as_ref().is_none_or(Self::check_os);

        let feature = rule.features.iter().all(|(k, v)| {
            let enable = self.feature.get(k).unwrap_or(&false);

            enable == v
        });

        let apply = os && feature;

        match rule.action {
            mc_launchermeta::version::rule::RuleAction::Allow => apply,
            mc_launchermeta::version::rule::RuleAction::Disallow => !apply,
        }
    }

    pub fn check_os(os: &Os) -> bool {
        let name = os.name.as_ref().is_none_or(|x| match x {
            mc_launchermeta::version::rule::OsName::Windows => OS == "windows",
            mc_launchermeta::version::rule::OsName::Osx => OS == "macos",
            mc_launchermeta::version::rule::OsName::Linux => OS == "linux",
        });

        let arch = os.arch.as_ref().is_none_or(|x| match x {
            mc_launchermeta::version::rule::OsArch::X86 => ARCH == "x86" || ARCH == "x86_64",
        });

        let version = os
            .version
            .as_ref()
            .is_none_or(|_| todo!("does not support checking OS version"));

        name && arch && version
    }
}
