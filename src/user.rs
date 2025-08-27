use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::launch::LaunchOption;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub enum UserType {
    MSA,
}

impl Display for UserType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let display = match self {
            UserType::MSA => "msa",
        };
        write!(f, "{display}")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct User {
    pub name: String,
    pub uuid: String,
    pub token: String,
    #[serde(rename = "type")]
    pub user_type: UserType,
}

impl LaunchOption for User {
    fn game_flags(&self) -> Vec<String> {
        vec![
            "--username".into(),
            self.name.clone(),
            "--uuid".into(),
            self.uuid.clone(),
            "--accessToken".into(),
            self.token.clone(),
            "--userType".into(),
            self.user_type.to_string(),
        ]
    }
}
