use std::env::consts::{ARCH, OS};

use mc_launchermeta::version::rule::Os;

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

pub fn check_class(class: &str) -> bool {
    match class {
        "natives-linux" => OS == "linux",
        "natives-windows" => OS == "windows",
        "natives-macos" => OS == "macos",
        c => todo!("unknown classifier {c}"),
    }
}
