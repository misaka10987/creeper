mod guard;
mod prompt;

use std::{fmt::Display, marker::PhantomData, str::FromStr};

use inquire::validator::{StringValidator, Validation};
use tokio::sync::Mutex;
use tracing::Metadata;
use tracing_subscriber::filter::{FilterFn, filter_fn};

struct InquireManagerInner {
    start_hooks: Vec<Box<dyn FnMut() + Send>>,
    end_hooks: Vec<Box<dyn FnMut() + Send>>,
}

pub struct InquireManager {
    inner: Mutex<InquireManagerInner>,
}

impl InquireManager {
    pub fn new() -> Self {
        let inner = InquireManagerInner {
            start_hooks: vec![],
            end_hooks: vec![],
        };

        Self {
            inner: Mutex::new(inner),
        }
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
