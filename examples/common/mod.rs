use anyhow::{Context as _, Result};
use pin_project_lite::pin_project;
use std::pin::Pin;
use std::str::FromStr;
use std::task::{Context, Poll};
use tracing::warn;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Layer};
use tracing_tree::HierarchicalLayer;
use twilight_model::id::Id;

pub fn init_tracing() -> Result<()> {
    let fmt = match dotenvy::var("GRAMOPHONE_TRACING_TREE").ok() {
        Some(value) if value.to_lowercase() == "true" =>  {
            HierarchicalLayer::default()
                .with_indent_lines(true)
                .with_indent_amount(6)
                .with_targets(true)
                .with_thread_names(true)
                .with_ansi(false)
                .boxed()
        },
        value => {
            if value.is_some_and(|v| v.to_lowercase() != "false" || v.is_empty()) {
                warn!("unexpected value of `GRAMOPHONE_TRACING_TREE` (should be `true`, `false` or nothing),\
                using tracing_subscriber::fmt instead.");
            }
            tracing_subscriber::fmt::layer().boxed()
        }
    }.with_filter(EnvFilter::from_default_env());
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(fmt))?;

    Ok(())
}

pub fn parse_id_from_env<T>(env: &'static str) -> Result<Id<T>> {
    let content = dotenvy::var(env)?;
    Id::<T>::from_str(&content)
        .with_context(|| format!("could not parse Discord snowflake of {env:?}"))
}

pin_project! {
    /// This type runs the future if it exists and returns the value
    /// as expected but it will yield indefinitely if this struct
    /// has no value inside.
    #[derive(Debug)]
    pub struct Optional<F> {
        #[pin]
        future: Option<F>,
    }
}

/// This trait allows to conveniently utilize [`Optional`] without
/// having to initialize it using the type itself by calling
/// `optional(...)` method.
pub trait OptionalExt {
    type Future: Future;

    fn optional(self) -> Optional<Self::Future>;
}

impl<F: Future> OptionalExt for Option<F> {
    type Future = F;

    fn optional(self) -> Optional<Self::Future> {
        Optional { future: self }
    }
}

impl<F: Future> Future for Optional<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project().future.as_pin_mut() {
            Some(f) => f.poll(cx),
            None => Poll::Pending,
        }
    }
}
