mod guard;
mod prompt;

use std::{fmt::Display, marker::PhantomData, str::FromStr, sync::atomic::AtomicBool};

use inquire::validator::{StringValidator, Validation};
use tokio::sync::Mutex;
use tracing::Metadata;
use tracing_subscriber::filter::{FilterFn, filter_fn};

use crate::Creeper;

struct Hooks {
    start: Vec<Box<dyn FnMut() + Send>>,
    end: Vec<Box<dyn FnMut() + Send>>,
}

impl Hooks {
    pub fn new() -> Self {
        Self {
            start: vec![],
            end: vec![],
        }
    }
}

pub struct InquireManager {
    active: AtomicBool,
    hooks: Mutex<Hooks>,
}

impl InquireManager {
    pub fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
            hooks: Mutex::new(Hooks::new()),
        }
    }
}

impl Creeper {
    pub fn is_inquire_active(&self) -> bool {
        self.inquire
            .active
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

pub type Filter = FilterFn<fn(&Metadata<'_>) -> bool>;

pub fn make_filter(f: fn(&Metadata<'_>) -> bool) -> Filter {
    filter_fn(f)
}

pub const fn parse_validator<T>() -> impl StringValidator
where
    T: FromStr,
    <T as FromStr>::Err: Display,
{
    struct Validator<T>(PhantomData<T>);

    impl<T> Clone for Validator<T> {
        fn clone(&self) -> Self {
            Self(self.0.clone())
        }
    }

    impl<T> StringValidator for Validator<T>
    where
        T: FromStr,
        <T as FromStr>::Err: Display,
    {
        fn validate(
            &self,
            input: &str,
        ) -> Result<inquire::validator::Validation, inquire::CustomUserError> {
            let valid = match input.parse::<T>() {
                Ok(_) => Validation::Valid,
                Err(e) => Validation::Invalid(e.to_string().into()),
            };
            Ok(valid)
        }
    }

    Validator::<T>(PhantomData)
}
