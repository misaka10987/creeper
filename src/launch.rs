use std::collections::HashMap;

pub struct LaunchCommand {
    envs: HashMap<String, String>,
    java_flags: Vec<String>,
    game_flags: Vec<String>,
}

impl LaunchCommand {
    pub fn new() -> Self {
        Self {
            envs: HashMap::new(),
            java_flags: vec![],
            game_flags: vec![],
        }
    }
    pub fn add(&mut self, opt: impl LaunchOption) {
        self.envs.extend(opt.envs());
        self.java_flags.extend(opt.java_flags());
        self.game_flags.extend(opt.game_flags());
    }
}

pub trait LaunchOption {
    fn envs(&self) -> HashMap<String, String> {
        HashMap::new()
    }
    fn java_flags(&self) -> Vec<String> {
        vec![]
    }
    fn game_flags(&self) -> Vec<String> {
        vec![]
    }
}
